//! Retained semantic catalog finalization over the existing page/link owner.

use super::*;

impl LinkProjection {
    /// Attach admitted semantic facts and exact artifacts to one existing page registration.
    /// This does not create a route or infer title/text from source paths.
    /// # Errors
    /// Reject unknown, incomplete or repeated pages and exhausted catalog bounds.
    pub fn describe(
        &mut self,
        route: &str,
        module: &str,
        title: Option<String>,
        body_text: Option<String>,
        evidence: crate::documentation::CatalogEvidence,
        limits: crate::documentation::CatalogLimits,
    ) -> Result<(), Error> {
        if module.is_empty()
            || module.chars().any(char::is_control)
            || self
                .pages
                .get(route)
                .is_none_or(|page| page.kind == PageKind::Incomplete)
            || self.descriptions.contains_key(route)
        {
            return Err(invalid(
                route,
                "catalog description requires one existing complete page",
            ));
        }
        if self.pages.len() > limits.pages {
            return Err(invalid(route, "catalog page bound exceeded"));
        }
        let bytes = route
            .len()
            .checked_add(module.len())
            .and_then(|n| n.checked_add(title.as_ref().map_or(0, String::len)))
            .and_then(|n| n.checked_add(body_text.as_ref().map_or(0, String::len)))
            .and_then(|n| {
                crate::documentation::catalog::evidence_paths(&evidence)
                    .try_fold(n, |n, path| n.checked_add(path.as_str().len()))
            })
            .and_then(|n| self.description_bytes.checked_add(n))
            .filter(|n| *n <= limits.bytes)
            .ok_or_else(|| invalid(route, "catalog byte bound exceeded"))?;
        self.description_bytes = bytes;
        self.descriptions.insert(
            route.to_owned(),
            crate::documentation::catalog::Description {
                module: module.to_owned(),
                title,
                body_text,
                evidence,
            },
        );
        Ok(())
    }

    /// Close every registered page and artifact against the exact retained target output.
    /// Native companions remain evidence for one reading route, and incomplete roots
    /// never become usable pages. Anchor IDs follow the existing target projection.
    /// # Errors
    /// Reject missing descriptions/artifacts, duplicate target routes, broken links or bounds.
    pub fn validate_catalog(
        self,
        output: &crate::source::CapturedSource,
        limits: crate::documentation::CatalogLimits,
    ) -> Result<crate::documentation::ValidatedCatalog, Error> {
        self.check_links(
            |route| {
                [format!("{route}.md"), format!("{route}.mdx")]
                    .iter()
                    .filter_map(|path| path.parse().ok())
                    .any(|path| output.file(&path).is_some())
            },
            |path| {
                path.parse()
                    .ok()
                    .is_some_and(|path| output.file(&path).is_some())
            },
        )?;
        if self.pages.len() > limits.pages {
            return Err(invalid("catalog", "catalog page bound exceeded"));
        }
        let Self {
            pages,
            mut descriptions,
            target_pages,
            target_anchors,
            description_bytes,
            ..
        } = self;
        let mut remaining = limits
            .bytes
            .checked_sub(description_bytes)
            .ok_or_else(|| invalid("catalog", "catalog byte bound exceeded"))?;
        let mut anchors_remaining = limits.anchors;
        let mut entries = Vec::new();
        let mut incomplete = BTreeSet::new();
        let mut routes = BTreeSet::new();
        for (logical, page) in pages {
            let target = target_pages.get(&logical).unwrap_or(&logical);
            remaining = remaining
                .checked_sub(target.len())
                .ok_or_else(|| invalid(&logical, "catalog route byte bound exceeded"))?;
            let route: EntryPath = target.parse()?;
            if !routes.insert(route.clone()) {
                return Err(invalid(
                    route.to_string(),
                    "duplicate catalog reading route",
                ));
            }
            if page.kind == PageKind::Incomplete {
                incomplete.insert(route);
                continue;
            }
            let description = descriptions.remove(&logical).ok_or_else(|| {
                invalid(&logical, "complete catalog page lacks admitted description")
            })?;
            {
                let mut paths =
                    crate::documentation::catalog::evidence_paths(&description.evidence).peekable();
                if paths.peek().is_none() || paths.any(|path| output.file(path).is_none()) {
                    return Err(invalid(&logical, "catalog provider artifact is absent"));
                }
            }
            match (&description.evidence, page.kind) {
                (
                    crate::documentation::CatalogEvidence::Physical {
                        provider,
                        native_markdown,
                    },
                    PageKind::Physical,
                ) if provider
                    .as_str()
                    .strip_suffix(".mdx")
                    .or_else(|| provider.as_str().strip_suffix(".md"))
                    == Some(route.as_str())
                    && native_markdown.as_ref().is_none_or(|native| {
                        native.as_str().strip_suffix(".md") == Some(route.as_str())
                    }) => {}
                (
                    crate::documentation::CatalogEvidence::QualifiedVirtual { .. },
                    PageKind::QualifiedVirtual,
                ) => {}
                _ => {
                    return Err(invalid(
                        &logical,
                        "catalog evidence does not match the admitted provider route",
                    ));
                }
            }
            let anchors =
                page.anchors
                    .map(|ids| {
                        anchors_remaining = anchors_remaining
                            .checked_sub(ids.len())
                            .ok_or_else(|| invalid(&logical, "catalog anchor bound exceeded"))?;
                        let projection = target_anchors
                            .as_ref()
                            .filter(|(_, routes)| routes.contains(&logical))
                            .map(|(projection, _)| projection);
                        ids.into_iter()
                            .map(|id| {
                                let bytes = if let Some(projection) = projection
                                    && (id.starts_with(projection.prefix)
                                        || projection.reserved.contains(&id.as_str()))
                                {
                                    id.len().checked_add(projection.prefix.len()).ok_or_else(
                                        || invalid(&logical, "catalog anchor byte bound exceeded"),
                                    )?
                                } else {
                                    id.len()
                                };
                                remaining = remaining.checked_sub(bytes).ok_or_else(|| {
                                    invalid(&logical, "catalog anchor byte bound exceeded")
                                })?;
                                let id = projection
                                    .map_or_else(|| id.clone(), |projection| projection.id(&id));
                                Ok(id)
                            })
                            .collect::<Result<BTreeSet<_>, Error>>()
                    })
                    .transpose()?;
            entries.push(crate::documentation::CatalogEntry {
                module: description.module,
                route,
                kind: page.kind,
                title: description.title,
                body_text: description.body_text,
                anchors,
                evidence: description.evidence,
            });
        }
        entries.sort_by(|a, b| a.route.cmp(&b.route));
        Ok(crate::documentation::ValidatedCatalog::new(
            output.inventory().identity(),
            entries,
            incomplete,
        ))
    }
}
