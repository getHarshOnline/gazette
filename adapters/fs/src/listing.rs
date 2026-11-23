//! Metadata catalogs use the same confined traversal as full captures.

use gazette_core::source::{ListedEntry, Listing, ListingBuilder, ListingLimits};
use gazette_ports::source::SourceListing;

use super::*;

impl SourceListing for FileSystem {
    fn list(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: ListingLimits,
    ) -> Result<Listing, Error> {
        let directory = open_selected_directory(root, subdirectory)?;
        list(&directory, Selection::Excluding(&[]), limits)
    }

    fn list_including(
        &self,
        root: &Path,
        include: &[EntryPath],
        limits: ListingLimits,
    ) -> Result<Listing, Error> {
        selection::validate(root, include)?;
        let directory = open_directory(root)?;
        let listing = list(&directory, Selection::Including(include), limits)?;
        for selected in include {
            if !listing.entries().contains_key(selected) {
                return Err(Error::InvalidInventory {
                    path: selected.as_str().into(),
                    reason: "required source selection is missing",
                });
            }
        }
        Ok(listing)
    }

    fn list_excluding(
        &self,
        root: &Path,
        exclude: &[EntryPath],
        limits: ListingLimits,
    ) -> Result<Listing, Error> {
        selection::validate(root, exclude)?;
        let directory = open_directory(root)?;
        list(&directory, Selection::Excluding(exclude), limits)
    }
}

pub(super) fn list(
    directory: &File,
    selection: Selection<'_>,
    limits: ListingLimits,
) -> Result<Listing, Error> {
    let mut visitor = MetadataVisitor(ListingBuilder::new(limits));
    walk::walk(
        directory,
        "",
        &mut visitor,
        ReadProfile::Source,
        None,
        selection,
    )?;
    Ok(visitor.0.finish())
}

struct MetadataVisitor(ListingBuilder);

impl walk::Visitor for MetadataVisitor {
    fn check_path(&self, path: &EntryPath) -> Result<(), Error> {
        self.0.check_path(path)
    }

    fn directory(&mut self, path: EntryPath) -> Result<(), Error> {
        self.0.insert(path, ListedEntry::Directory)
    }

    fn file(
        &mut self,
        path: EntryPath,
        _file: &mut File,
        metadata: &Metadata,
    ) -> Result<(), Error> {
        self.0.insert(
            path,
            ListedEntry::File {
                observed_bytes: metadata.len(),
            },
        )
    }
}
