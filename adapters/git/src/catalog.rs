//! Bounded pinned metadata enumeration; file blobs are never opened.
use super::*;
use gazette_core::source::{ListedEntry, Listing, ListingBuilder, ListingLimits};
use gazette_ports::source::SourceFilter;

pub(super) fn list(
    root: &Path,
    subdirectory: &Subdirectory,
    commit: &CommitId,
    filter: &dyn SourceFilter,
    limits: ListingLimits,
    metadata: &mut MetadataBudget,
) -> Result<Listing, Error> {
    let (repo, oid) = open_revision(root, commit)?;
    let mut objects = Objects::new(&repo, metadata)?;
    let root_tree = objects.commit_tree(oid, root)?;
    let selected = objects.select_entry(root_tree, subdirectory, 0o040000)?;
    let tree = objects.tree(selected, Path::new(&subdirectory.to_string()))?;
    let mut listing = ListingBuilder::new(limits);
    visit(&mut objects, &tree, "", filter, &mut listing)?;
    check_head(&repo, root, oid)?;
    Ok(listing.finish())
}

fn visit(
    objects: &mut Objects<'_, '_>,
    tree: &Tree<'_>,
    prefix: &str,
    filter: &dyn SourceFilter,
    listing: &mut ListingBuilder,
) -> Result<(), Error> {
    for entry in tree {
        let name = entry_name(entry.name_bytes(), Path::new(prefix))?;
        let relative: EntryPath = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        }
        .parse()?;
        if !filter.allows(&relative)? {
            continue;
        }
        let path = Path::new(relative.as_str());
        listing.check_path(&relative)?;
        match entry.filemode() {
            0o040000 => {
                if !filter.descend(&relative) {
                    listing.insert(relative, ListedEntry::Directory)?;
                    continue;
                }
                let child = objects.tree(entry.id(), path)?;
                listing.insert(relative.clone(), ListedEntry::Directory)?;
                visit(objects, &child, relative.as_str(), filter, listing)?;
            }
            0o100644 | 0o100755 if !filter.ancestor_only(relative.as_str()) => {
                let (size, kind) = objects
                    .odb
                    .read_header(entry.id())
                    .map_err(|e| git_error("read Git metadata header", path, e))?;
                if kind != ObjectType::Blob {
                    return Err(invalid(path, "expected a file blob"));
                }
                listing.insert(
                    relative,
                    ListedEntry::File {
                        observed_bytes: size as u64,
                    },
                )?;
            }
            _ => return Err(Error::Unsupported(path.to_owned())),
        }
    }
    Ok(())
}
