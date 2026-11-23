//! Discover linked companion files only within the supplied manual captures.

use super::*;

pub(crate) struct Companion<'a> {
    pub input: EntryPath,
    pub project: usize,
    pub output: EntryPath,
    pub bytes: &'a [u8],
}

pub(crate) fn discover<'a>(
    projects: &'a [ProjectModule<'a>],
    catalogs: &mut BTreeMap<&'a str, ManualDestinations>,
    markdown: &dyn Markdown,
) -> Result<Vec<Companion<'a>>, Error> {
    let mut order: Vec<_> = projects.iter().enumerate().collect();
    // Shared captures choose a stable namespace without changing chapter order.
    order.sort_by_key(|(_, project)| project.slug);
    let mut companions = Vec::new();
    for (index, project) in order {
        let Some(manual) = &project.manual else {
            continue;
        };
        let catalog = catalogs
            .get_mut(manual.source)
            .expect("registered manual source");
        for item in &project.navigation {
            let ProjectItem::Manual(section) = item else {
                continue;
            };
            for page in &section.pages {
                let Selection::File { path } = page.selection else {
                    unreachable!("checked manual file")
                };
                let bytes = manual
                    .tree
                    .file(path)
                    .ok_or_else(|| invalid(path.as_str(), "missing selected manual page bytes"))?;
                let text = manual_source(bytes, path)?;
                let document = markdown
                    .parse(text, MANUAL_PROFILE, Limits::default())
                    .map_err(Error::Markdown)?;
                for link in &document.syntax().links {
                    let Some(input) = catalog.linked_companion(path, &link.destination)? else {
                        continue;
                    };
                    let bytes = manual.tree.file(&input).ok_or_else(|| {
                        invalid(
                            input.as_str(),
                            "linked manual companion is not a retained file",
                        )
                    })?;
                    let output: EntryPath = format!("{}/assets/{input}", project.slug)
                        .parse()
                        .map_err(|_| {
                            invalid(input.as_str(), "invalid manual companion output path")
                        })?;
                    register_asset(catalog, &input, output.as_str())?;
                    companions.push(Companion {
                        input: input.clone(),
                        project: index,
                        output,
                        bytes,
                    });
                }
            }
        }
    }
    Ok(companions)
}
