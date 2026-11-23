//! Explicit taxonomy catalogs; membership does not establish hierarchy, routes or admission.

use gazette_core::publication::Taxonomy;

use super::{Diagnostic, resolve::report};

pub(super) fn publications(
    brands: &std::collections::BTreeMap<super::Key, super::Brand>,
    taxonomies: &std::collections::BTreeMap<super::Key, Taxonomy>,
    errors: &mut Vec<Diagnostic>,
) {
    for publication in brands.values().flat_map(|brand| &brand.publications) {
        let field = format!(
            "brands.publications[{:?}].taxonomy_pages",
            publication.site.as_str()
        );
        if !publication.taxonomy_pages.is_empty() && publication.public_base_url.is_none() {
            report(
                errors,
                "workspace_taxonomy_base",
                &field,
                Some(&publication.site),
            );
        }
        let mut identities = std::collections::BTreeSet::new();
        let mut outputs = std::collections::BTreeSet::new();
        for page in &publication.taxonomy_pages {
            match taxonomies.get(&page.taxonomy) {
                None => report(
                    errors,
                    "workspace_taxonomy_reference",
                    &field,
                    Some(&page.taxonomy),
                ),
                Some(taxonomy) => {
                    if let Some(term) = &page.term
                        && taxonomy.term(term).is_none()
                    {
                        report(
                            errors,
                            "workspace_taxonomy_term_reference",
                            &field,
                            Some(term),
                        );
                    }
                }
            }
            if !identities.insert((&page.taxonomy, &page.term)) {
                report(
                    errors,
                    "workspace_taxonomy_page_duplicate",
                    &field,
                    Some(&page.taxonomy),
                );
            }
            if !outputs.insert(page.route.output().as_str()) {
                report(
                    errors,
                    "workspace_taxonomy_page_output",
                    &field,
                    Some(&page.taxonomy),
                );
            }
        }
        for output in &outputs {
            let mut ancestor = *output;
            while let Some((parent, _)) = ancestor.rsplit_once('/') {
                if outputs.contains(parent) {
                    report(
                        errors,
                        "workspace_taxonomy_page_output",
                        &field,
                        Some(&publication.site),
                    );
                    break;
                }
                ancestor = parent;
            }
        }
    }
}

pub(super) fn normalize(taxonomies: &mut [Taxonomy], errors: &mut Vec<Diagnostic>) {
    for taxonomy in taxonomies {
        let field = format!("taxonomies[{:?}]", taxonomy.id.as_str());
        if invalid_label(&taxonomy.label) {
            report(
                errors,
                "workspace_taxonomy_label",
                &format!("{field}.label"),
                Some(&taxonomy.id),
            );
        }
        taxonomy.terms.sort_by(|left, right| left.id.cmp(&right.id));
        for term in &taxonomy.terms {
            if invalid_label(&term.label) {
                report(
                    errors,
                    "workspace_taxonomy_term_label",
                    &format!("{field}.terms.label"),
                    Some(&term.id),
                );
            }
        }
        for pair in taxonomy.terms.windows(2) {
            if pair[0].id == pair[1].id {
                report(
                    errors,
                    "workspace_duplicate",
                    &format!("{field}.terms"),
                    Some(&pair[0].id),
                );
            }
        }
    }
}

fn invalid_label(value: &str) -> bool {
    value.trim().is_empty() || value.chars().any(char::is_control)
}
