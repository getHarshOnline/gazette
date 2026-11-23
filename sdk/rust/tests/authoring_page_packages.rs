//! Page-package editor evidence comes from the compiler over exact selected bytes.

use gazette_sdk::{
    authored::PageCompanionKind,
    authoring::{
        PagePackageRole,
        session::{AssetObservationSource, PageInputSource, Session, Source},
    },
};

const WORKSPACE: &str = r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
public_base_url='https://example.test/'
authored=[{source='source:content',target='content'}]
[[sources]]
id='source:content'
location={kind='filesystem',root='/not-opened/content'}
"#;

const PAGE: &str = "+++\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n";

const PAGE_WITH_RESOURCE: &str = "+++\n[[resources]]\ninputs=['guide/main.ts','guide/lib/dep.ts']\nbuild={kind='browser-module',entry='guide/main.ts'}\noutput='/guide/main.js'\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n";

fn source(id: &str, path: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path.parse().unwrap(),
        text: text.into(),
    }
}

#[test]
fn opaque_companions_have_digest_bound_ownership_without_editor_overlays() {
    let image = vec![0, 159, 146, 150, 255];
    let mut session = Session::new_with_manuals_page_inputs_and_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "content/guide/index.md", PAGE)],
        [],
        [PageInputSource {
            id: "image".into(),
            path: "content/guide/image.png".parse().unwrap(),
            bytes: image,
        }],
        [],
        Default::default(),
    )
    .expect("exact opaque input session");
    let analysis = session.analysis().as_ref().expect("complete analysis");
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());

    let owner = session
        .page_package("page")
        .expect("current analysis")
        .expect("owning index package");
    assert!(matches!(owner.role, PagePackageRole::Document));
    assert_eq!(owner.package.document().as_str(), "content/guide/index.md");
    assert_eq!(
        owner.package.companions()[0].kind(),
        PageCompanionKind::Static
    );

    let companion = session
        .page_package("image")
        .expect("current analysis")
        .expect("retained companion package");
    assert!(matches!(companion.role, PagePackageRole::Companion { .. }));
    assert!(!session.open("image", 1, "not bytes".into()).unwrap());

    assert!(session.open("page", 1, PAGE.into()).unwrap());
    assert!(session.page_package("page").unwrap().is_some());
}

#[test]
fn authored_resource_scalars_resolve_to_selected_companions_without_output_fiction() {
    let session = Session::new_with_manuals_page_inputs_and_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "content/guide/index.md", PAGE_WITH_RESOURCE)],
        [],
        [
            PageInputSource {
                id: "main".into(),
                path: "content/guide/main.ts".parse().unwrap(),
                bytes: b"import './lib/dep.ts';".to_vec(),
            },
            PageInputSource {
                id: "dependency".into(),
                path: "content/guide/lib/dep.ts".parse().unwrap(),
                bytes: b"export const answer = 42;".to_vec(),
            },
        ],
        [],
        Default::default(),
    )
    .expect("exact page-resource session");
    let offset = PAGE_WITH_RESOURCE
        .find("'guide/lib/dep.ts'")
        .expect("dependency input")
        + 2;
    let navigation = session
        .page_resource_navigation("page", offset)
        .expect("current native navigation")
        .expect("declared resource input");
    assert_eq!(navigation.input.as_str(), "guide/lib/dep.ts");
    assert_eq!(
        navigation.companion.path().as_str(),
        "content/guide/lib/dep.ts"
    );
    assert_eq!(
        navigation
            .compiled_destinations
            .iter()
            .map(|route| route.as_str())
            .collect::<Vec<_>>(),
        ["/guide/main.js"]
    );
    assert_eq!(
        navigation.occurrence.span.slice(PAGE_WITH_RESOURCE),
        Some("'guide/lib/dep.ts'")
    );
    assert!(
        session
            .page_resource_navigation("page", PAGE_WITH_RESOURCE.find("content:guide").unwrap(),)
            .unwrap()
            .is_none(),
        "unrelated authored strings never become page-resource navigation"
    );
}

fn observed(id: &str, path: &str, bytes: u64) -> AssetObservationSource {
    AssetObservationSource {
        id: id.into(),
        path: path.parse().unwrap(),
        observed_bytes: bytes,
    }
}

#[test]
fn metadata_only_companions_survive_edits_without_becoming_captured_sources() {
    let mut session = Session::new(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "content/guide/index.md", PAGE)],
        Default::default(),
    )
    .unwrap()
    .with_asset_observations([observed("film", "content/guide/film.mp4", 64_u64 << 30)])
    .unwrap();
    let analysis = session.analysis().as_ref().unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let owner = session.page_package("film").unwrap().unwrap();
    let PagePackageRole::Companion { companion } = owner.role else {
        panic!("observed companion")
    };
    assert_eq!(companion.source_sha256(), None);
    assert_eq!(companion.observed_bytes(), Some(64_u64 << 30));
    assert!(!analysis.inputs().contains_key("content/guide/film.mp4"));
    let snapshot = session.snapshot();
    assert_eq!(snapshot.sources.len(), 2);
    assert_eq!(
        snapshot.asset_observations["film"].observed_bytes(),
        64_u64 << 30
    );
    assert!(
        session
            .source_chunk(session.generation(), "film", 0, 1)
            .is_err()
    );
    assert!(!session.open("film", 1, "not asset bytes".into()).unwrap());
    assert!(session.open("workspace", 1, "invalid = [".into()).unwrap());
    assert!(!session.analysis().as_ref().unwrap().structurally_valid());
    assert!(session.change("workspace", 2, WORKSPACE.into()).unwrap());
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
    assert!(session.page_package("film").unwrap().is_some());
}

#[test]
fn observed_resource_navigation_keeps_declared_routes_without_hashes() {
    let session = Session::new(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "content/guide/index.md", PAGE_WITH_RESOURCE)],
        Default::default(),
    )
    .unwrap()
    .with_asset_observations([
        observed("main", "content/guide/main.ts", 20),
        observed("dep", "content/guide/lib/dep.ts", 200),
    ])
    .unwrap();
    let analysis = session.analysis().as_ref().unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let navigation = session
        .page_resource_navigation(
            "page",
            PAGE_WITH_RESOURCE.find("'guide/lib/dep.ts'").unwrap() + 2,
        )
        .unwrap()
        .unwrap();
    assert_eq!(navigation.companion.observed_bytes(), Some(200));
    assert_eq!(navigation.companion.source_sha256(), None);
    assert_eq!(
        navigation.compiled_destinations[0].as_str(),
        "/guide/main.js"
    );
}

#[test]
fn observed_sources_share_selection_limits_and_refuse_duplicate_roles() {
    for observation in [
        observed("page", "content/guide/image.png", 1),
        observed("image", "content/guide/index.md", 1),
    ] {
        let session = Session::new(
            source("workspace", "workspace.toml", WORKSPACE),
            [source("page", "content/guide/index.md", PAGE)],
            Default::default(),
        )
        .unwrap();
        assert!(session.with_asset_observations([observation]).is_err());
    }
    let session = Session::new(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("page", "content/guide/index.md", PAGE)],
        gazette_sdk::authoring::session::Limits {
            documents: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        session
            .with_asset_observations([observed("image", "content/guide/image.png", 1)])
            .is_err()
    );
}
