//! Bound documentation evidence keeps project ownership and explicit serving coordinates.

use super::*;
use gazette_core::{
    documentation::{
        CatalogEvidence,
        projection::{LinkProjection, PageKind},
    },
    source::{CapturedSource, Content, InventoryBuilder},
};

const WORKSPACE: &str = r##"
[[principals]]
id = "owner:publisher"
kind = "person"
[[sites]]
id = "site:docs"
publisher = "owner:publisher"
deployable = false
[[projects]]
id = "project:library"
lifecycle = "active"
visibility = "public"
availability = "available"
[[sources]]
id = "source:library"
location = { kind = "repository", root = "/not-opened/library", commit = "0000000000000000000000000000000000000000", subdirectory = "retained" }
[[brands]]
id = "brand:library"
name = "Library"
[[brands.publications]]
site = "site:docs"
documentation_repository = { name = "library_doc", path = "/not-opened/library_doc", public_base_url = "https://docs.example.test/v2/", build = { target = { kind = "mintlify", theme = "mint", primary_color = "#123456" }, modules = [
 {project = "project:library", slug = "manual", title = "Manual", navigation = [
  {kind = "reference", slug = "reference", title = "API", input = { kind = "artifact", source = "source:library", selection = {kind = "file", path = "generated.md"}, format = "markdown" }}
 ]}
]}}
"##;
fn inputs(source: &str) -> AcquiredInputs {
    inputs_with_files(source, &[("generated.md", b"source")])
}
fn inputs_with_files(source: &str, files: &[(&str, &[u8])]) -> AcquiredInputs {
    let mut inventory = InventoryBuilder::new(Default::default());
    let mut retained = BTreeMap::new();
    for (name, bytes) in files {
        let path: gazette_core::source::EntryPath = name.parse().unwrap();
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .unwrap();
        retained.insert(path, bytes.to_vec());
    }
    let captured = CapturedSource::from_parts(inventory.finish(), retained).unwrap();
    let configuration = crate::configuration::ResolvedWorkspace::supplied(
        gazette_compiler::workspace::compile(source, Default::default()).unwrap(),
    );
    AcquiredInputs::from_documentation_sources(
        configuration,
        "site:docs".parse().unwrap(),
        BTreeMap::from([(
            "source:library".parse().unwrap(),
            crate::source::AcquiredInput::Filesystem(captured),
        )]),
        BTreeMap::new(),
    )
}
fn catalog(module: &str) -> (ValidatedCatalog, gazette_core::source::Inventory) {
    catalog_page(module, PageKind::Physical, Some("Guide"))
}

fn catalog_page(
    module: &str,
    kind: PageKind,
    title: Option<&str>,
) -> (ValidatedCatalog, gazette_core::source::Inventory) {
    let path: gazette_core::source::EntryPath = "guide.md".parse().unwrap();
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: 1,
                sha256: Sha256Digest::of(b"x"),
            },
        )
        .unwrap();
    let output =
        CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, b"x".to_vec())]))
            .unwrap();
    let mut links = LinkProjection::default();
    links
        .register_provider("guide", kind, Some(Default::default()))
        .unwrap();
    links
        .describe(
            "guide",
            module,
            title.map(str::to_owned),
            Some("Documented words".into()),
            if kind == PageKind::Physical {
                CatalogEvidence::Physical {
                    provider: "guide.md".parse().unwrap(),
                    native_markdown: None,
                }
            } else {
                CatalogEvidence::QualifiedVirtual {
                    artifacts: BTreeSet::from(["guide.md".parse().unwrap()]),
                }
            },
            Default::default(),
        )
        .unwrap();
    (
        links.validate_catalog(&output, Default::default()).unwrap(),
        output.into_parts().0,
    )
}
#[test]
fn bound_catalog_uses_configured_owner_and_independent_preview_base() {
    let mut inputs = inputs(WORKSPACE);
    let (catalog, identity) = catalog("manual");
    let catalog = Arc::new(DocumentationCatalog::bind(&inputs, catalog, &identity, &[]).unwrap());
    let entry = &catalog.entries()[0];
    assert_eq!(entry.project.as_str(), "project:library");
    assert_eq!(
        catalog.canonical_url(entry).unwrap().as_deref(),
        Some("https://docs.example.test/v2/guide")
    );
    let preview = "http://127.0.0.1:48001/docs/".parse().unwrap();
    assert_eq!(
        catalog.page_url(entry, Some(&preview)).unwrap().as_deref(),
        Some("http://127.0.0.1:48001/docs/guide")
    );
    assert_eq!(
        catalog.canonical_url(entry).unwrap().as_deref(),
        Some("https://docs.example.test/v2/guide")
    );
    inputs
        .install_documentation_catalog(catalog.clone())
        .unwrap();
    assert_eq!(
        inputs.documentation_catalog().unwrap().output_identity(),
        identity.identity()
    );
    assert!(inputs.install_documentation_catalog(catalog).is_err());
}
#[test]
fn unknown_module_and_changed_configuration_refuse_attachment() {
    let original = inputs(WORKSPACE);
    let (unknown, identity) = catalog("invented");
    assert!(DocumentationCatalog::bind(&original, unknown, &identity, &[]).is_err());
    let (known, identity) = catalog("manual");
    let known = Arc::new(DocumentationCatalog::bind(&original, known, &identity, &[]).unwrap());
    let mut changed = inputs(&WORKSPACE.replace("Library", "Changed library"));
    assert!(changed.install_documentation_catalog(known).is_err());
}

#[test]
fn bound_catalog_keeps_exact_incomplete_roots_visible() {
    let inputs = inputs(WORKSPACE);
    let output = CapturedSource::from_parts(
        InventoryBuilder::new(Default::default()).finish(),
        BTreeMap::<gazette_core::source::EntryPath, Vec<u8>>::new(),
    )
    .unwrap();
    let mut links = LinkProjection::default();
    links
        .register_provider("manual/reference", PageKind::Incomplete, None)
        .unwrap();
    let checked = links.validate_catalog(&output, Default::default()).unwrap();
    let bound = DocumentationCatalog::bind(&inputs, checked, output.inventory(), &[]).unwrap();
    assert!(!bound.complete());
    assert!(bound.entries().is_empty());
    assert!(bound.incomplete_roots().contains("manual/reference"));
    let mut project = inputs
        .compilation()
        .workspace
        .project(&"project:library".parse().unwrap())
        .unwrap()
        .clone();
    project.resources = Some(gazette_core::publication::ProjectResources {
        code_documentation: Some("https://docs.example.test/v2/manual/reference".into()),
        ..Default::default()
    });
    assert!(crate::build::documentation_reading::project_work(Some(&bound), &project).is_err());
}

#[test]
fn qualified_project_documentation_uses_exact_owner_route_and_optional_title() {
    let inputs = inputs(WORKSPACE);
    let mut project = inputs
        .compilation()
        .workspace
        .project(&"project:library".parse().unwrap())
        .unwrap()
        .clone();
    let url = "https://docs.example.test/v2/guide";
    project.resources = Some(gazette_core::publication::ProjectResources {
        code_documentation: Some(url.into()),
        ..Default::default()
    });
    for title in [None, Some("Provider title")] {
        let (checked, identity) = catalog_page("manual", PageKind::QualifiedVirtual, title);
        let bound = DocumentationCatalog::bind(&inputs, checked, &identity, &[]).unwrap();
        let work = crate::build::documentation_reading::project_work(Some(&bound), &project)
            .unwrap()
            .unwrap();
        assert_eq!(work.public_url, url);
        assert_eq!(work.title.as_deref(), title);
        for destination in [
            format!("{url}#unknown"),
            format!("{url}/"),
            format!("{url}?other=1"),
        ] {
            let mut altered = project.clone();
            altered.resources.as_mut().unwrap().code_documentation = Some(destination);
            assert!(
                crate::build::documentation_reading::project_work(Some(&bound), &altered)
                    .unwrap()
                    .is_none()
            );
        }
        let mut other = project.clone();
        other.id = "project:other".parse().unwrap();
        assert!(
            crate::build::documentation_reading::project_work(Some(&bound), &other)
                .unwrap()
                .is_none()
        );
    }
    assert!(
        crate::build::documentation_reading::project_work(None, &project)
            .unwrap()
            .is_none()
    );
}

#[test]
fn shared_source_containment_accepts_extra_publication_entries_but_refuses_missing_or_changed_bytes()
 {
    let original = inputs(WORKSPACE);
    let (checked, identity) = catalog("manual");
    let bound = Arc::new(DocumentationCatalog::bind(&original, checked, &identity, &[]).unwrap());
    let mut superset = inputs_with_files(
        WORKSPACE,
        &[("generated.md", b"source"), ("unrelated.md", b"extra")],
    );
    assert_ne!(
        original.sources()[&"source:library".parse().unwrap()]
            .inventory()
            .identity(),
        superset.sources()[&"source:library".parse().unwrap()]
            .inventory()
            .identity()
    );
    superset
        .install_documentation_catalog(bound.clone())
        .unwrap();
    for files in [
        vec![("generated.md", b"changed".as_slice())],
        vec![("other.md", b"source".as_slice())],
    ] {
        let mut changed = inputs_with_files(WORKSPACE, &files);
        assert!(
            changed
                .install_documentation_catalog(bound.clone())
                .is_err()
        );
    }
    assert!(
        bound
            .protected_roots()
            .contains(&std::path::PathBuf::from("/not-opened/library"))
    );
}

#[test]
fn catalog_digest_binds_admitted_text_even_when_output_bytes_are_identical() {
    let inputs = inputs(WORKSPACE);
    let (first, identity) = catalog("manual");
    let first = DocumentationCatalog::bind(&inputs, first, &identity, &[]).unwrap();
    let (same, identity) = catalog("manual");
    let same = DocumentationCatalog::bind(&inputs, same, &identity, &[]).unwrap();
    assert_eq!(first.evidence_sha256(), same.evidence_sha256());
    let mut projection = LinkProjection::default();
    projection
        .register_provider("guide", PageKind::Physical, Some(Default::default()))
        .unwrap();
    projection
        .describe(
            "guide",
            "manual",
            Some("Guide".into()),
            Some("Different admitted text".into()),
            CatalogEvidence::Physical {
                provider: "guide.md".parse().unwrap(),
                native_markdown: None,
            },
            Default::default(),
        )
        .unwrap();
    let mut inventory = InventoryBuilder::new(Default::default());
    let path: gazette_core::source::EntryPath = "guide.md".parse().unwrap();
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: 1,
                sha256: Sha256Digest::of(b"x"),
            },
        )
        .unwrap();
    let output =
        CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, b"x".to_vec())]))
            .unwrap();
    let changed = DocumentationCatalog::bind(
        &inputs,
        projection
            .validate_catalog(&output, Default::default())
            .unwrap(),
        &identity,
        &[],
    )
    .unwrap();
    assert_eq!(first.output_identity(), changed.output_identity());
    assert_ne!(first.evidence_sha256(), changed.evidence_sha256());
    assert!(super::evidence::digest(&"larger than limit", 1).is_err());
}

#[test]
fn supplied_catalog_charges_target_and_catalog_budgets_and_retains_generator_roots() {
    let inputs = inputs(WORKSPACE);
    let (catalog, inventory) = catalog("manual");
    let stage = std::path::PathBuf::from("/not-opened/generation-stage");
    let bound =
        DocumentationCatalog::bind(&inputs, catalog, &inventory, std::slice::from_ref(&stage))
            .unwrap();
    assert!(bound.protected_roots().contains(&stage));
    for mut limits in [
        gazette_core::source::Limits {
            entries: 0,
            ..Default::default()
        },
        gazette_core::source::Limits {
            total_bytes: 0,
            ..Default::default()
        },
        gazette_core::source::Limits {
            file_bytes: 0,
            ..Default::default()
        },
        gazette_core::source::Limits {
            depth: 0,
            ..Default::default()
        },
    ] {
        let mut remaining = usize::MAX;
        assert!(bound.charge(&mut limits, &mut remaining).is_err());
    }
    assert!(bound.charge(&mut Default::default(), &mut 0).is_err());
    let mut output = gazette_core::source::Limits {
        entries: 1,
        total_bytes: 1,
        ..Default::default()
    };
    let mut catalog_bytes = bound.retained_bytes;
    bound.charge(&mut output, &mut catalog_bytes).unwrap();
    assert_eq!(output.entries, 0);
    assert_eq!(output.total_bytes, 0);
    assert_eq!(catalog_bytes, 0);
}
