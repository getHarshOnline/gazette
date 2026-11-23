//! Provider catalog facts are joined to exact retained output, not guessed URLs.

use gazette_adapter_mintlify::Mintlify;
use gazette_core::{
    documentation::{
        CatalogEvidence, CatalogLimits, Format, ManualPage, ManualSection, ManualSource,
        ProjectItem, ProjectModule, Reference, Request, Target,
    },
    source::Limits,
};
use gazette_ports::documentation::DocumentationTarget;
mod support;
use support::Input;

fn target() -> Target {
    Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123456".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    }
}

fn manual_input(bytes: &[u8]) -> Input {
    use gazette_core::source::{
        CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest,
    };
    let path: EntryPath = "guide.md".parse().unwrap();
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .unwrap();
    Input {
        tree: CapturedSource::from_parts(
            inventory.finish(),
            std::collections::BTreeMap::from([(path.clone(), bytes.to_vec())]),
        )
        .unwrap(),
        selection: gazette_core::documentation::Selection::File { path },
    }
}

#[test]
fn manual_catalog_uses_actual_reserved_route_and_admitted_text_without_changing_bytes() {
    let input = manual_input(b"# Guide\n\nWords **remain** readable.\n\n## Detail {#content}\n\n<div>HTML &amp; text</div>\n");
    let route = "manual/index".parse().unwrap();
    let projects = [ProjectModule {
        slug: "manual",
        title: "Manual",
        manual: Some(ManualSource {
            source: "source:manual",
            tree: input.tree.as_input(),
            assets: vec![],
        }),
        navigation: vec![ProjectItem::Manual(ManualSection {
            title: "Guides",
            pages: vec![ManualPage {
                title: "Guide",
                route: &route,
                selection: &input.selection,
            }],
        })],
    }];
    let target = target();
    let adapter = Mintlify::new(&gazette_adapter_markdown::Comrak);
    let request = || Request {
        name: "Docs",
        public_base_url: None,
        target: &target,
        projects: &projects,
        usage: None,
    };
    let assembled = adapter
        .assemble_with_catalog(request(), Limits::default(), CatalogLimits::default())
        .unwrap();
    let legacy = adapter.assemble(request(), Limits::default()).unwrap();
    assert_eq!(assembled.output.inventory(), legacy.inventory());
    for entry in legacy.inventory().entries() {
        assert_eq!(assembled.output.file(&entry.path), legacy.file(&entry.path));
    }
    let catalog = assembled.catalog.unwrap();
    assert_eq!(catalog.entries().len(), 1);
    let page = &catalog.entries()[0];
    assert_eq!(page.route.as_str(), "manual/gazette-page-index");
    assert_eq!(page.title.as_deref(), Some("Guide"));
    assert!(
        page.body_text
            .as_deref()
            .unwrap()
            .contains("Words remain readable.")
    );
    assert_eq!(
        page.body_text
            .as_deref()
            .unwrap()
            .matches("HTML & text")
            .count(),
        1
    );
    assert!(
        page.anchors
            .as_ref()
            .unwrap()
            .iter()
            .any(|anchor| anchor.ends_with("content") && anchor != "content")
    );
    assert!(
        matches!(&page.evidence,CatalogEvidence::Physical{provider,native_markdown:None} if provider.as_str()=="manual/gazette-page-index.md")
    );
    assert!(
        adapter
            .assemble_with_catalog(
                request(),
                Limits::default(),
                CatalogLimits {
                    bytes: 4,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn typedoc_catalog_qualifies_only_its_actual_root_and_retains_artifact_proof() {
    let input = Input::new(br#"{"schemaVersion":"2.0","variant":"project","id":0,"kind":1,"name":"Library","flags":{},"children":[{"variant":"declaration","id":1,"kind":64,"name":"run","flags":{}}]}"#);
    let projects = [ProjectModule {
        slug: "library",
        title: "Library",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            slug: "reference",
            title: "API",
            pages: None,
            artifact: input.artifact(Format::TypeDocJson),
        })],
    }];
    let target = target();
    let assembled = Mintlify::new(&gazette_adapter_markdown::Comrak)
        .assemble_with_catalog(
            Request {
                name: "Docs",
                public_base_url: None,
                target: &target,
                projects: &projects,
                usage: None,
            },
            Limits::default(),
            CatalogLimits::default(),
        )
        .unwrap();
    let catalog = assembled.catalog.unwrap();
    assert_eq!(
        catalog.entries().len(),
        1,
        "symbols without qualified provider routes are not invented"
    );
    let page = &catalog.entries()[0];
    assert_eq!(page.route.as_str(), "library/reference");
    assert_eq!(page.title.as_deref(), Some("Library"));
    assert!(page.body_text.is_none() && page.anchors.is_none());
    let CatalogEvidence::QualifiedVirtual { artifacts } = &page.evidence else {
        panic!("provider root")
    };
    assert_eq!(artifacts.len(), 1);
    assert!(
        artifacts
            .iter()
            .all(|path| assembled.output.file(path).is_some())
    );
}
