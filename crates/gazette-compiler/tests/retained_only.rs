//! Explicit retention preserves opaque evidence without granting publication use.

use gazette_compiler::{
    authored::{self, SelectionError},
    corpus::{File, Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{self, Disposition},
    },
    source::{EntryPath, Sha256Digest},
};

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("fixture coordinate")
}

fn configuration(retained: &str) -> String {
    format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={{kind='filesystem',root='/fixture/content'}}\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
         authored=[{{source='source:one',target='authored/one',static_assets={{route_prefix='/media/'}},retained_only={retained}}}]\n"
    )
}

fn page(metadata: &str, body: &str) -> Vec<u8> {
    format!(
        "+++\n{metadata}[content]\nid='content:guide'\nsite='site:one'\n\
         title='Guide'\nlifecycle='published'\nroute={{path='/guide/'}}\n\
         structure={{kind='page',family={{kind='article'}}}}\n+++\n# Guide\n{body}"
    )
    .into_bytes()
}

fn parse(files: &[(String, Vec<u8>)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained native corpus")
}

#[test]
fn image_alternatives_cannot_select_retained_only_inputs() {
    let compilation =
        workspace::compile(&configuration("['guide/dark.png']"), Default::default()).unwrap();
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page(
                "[[images]]\ndefault='light.png'\ndark='dark.png'\n",
                "![Figure](light.png)",
            ),
        ),
        ("authored/one/guide/light.png".into(), b"light".to_vec()),
        (
            "authored/one/guide/dark.png".into(),
            b"retained original".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let error = authored::resolve(&compilation, &corpus, Default::default()).unwrap_err();
    assert!(
        error.to_string().contains("explicitly retained-only"),
        "{error}"
    );
}

#[test]
fn exact_retention_preserves_bytes_and_private_policy_without_companion_or_output() {
    let compilation =
        workspace::compile(&configuration("['guide/.DS_Store']"), Default::default()).unwrap();
    let files = vec![
        ("authored/one/guide/index.md".into(), page("", "")),
        (
            "authored/one/guide/.DS_Store".into(),
            b"opaque retained metadata".to_vec(),
        ),
        ("authored/one/guide/image.png".into(), b"image".to_vec()),
        (
            "authored/one/unselected.bin".into(),
            b"unknown input".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default()).unwrap();
    let retained = path("authored/one/guide/.DS_Store");
    let Some(File::Asset(bytes)) = resolved.corpus().files().get(&retained) else {
        panic!("retained-only bytes must remain in the complete corpus")
    };
    assert_eq!(
        Sha256Digest::of(bytes),
        Sha256Digest::of(b"opaque retained metadata")
    );
    let binding = &compilation
        .workspace
        .publication_for(&key("site:one"))
        .unwrap()
        .authored[0];
    assert_eq!(
        serde_json::to_value(binding).unwrap()["retained_only"],
        serde_json::json!(["guide/.DS_Store"])
    );
    let package = resolved.page_package(&key("content:guide")).unwrap();
    assert_eq!(package.companions().len(), 1);
    assert_eq!(
        package.companions()[0].path(),
        &path("authored/one/guide/image.png")
    );
    let assets = resolved.assets_for(&key("site:one")).unwrap();
    assert!(assets.get(&retained).is_none());
    assert!(assets.get(&path("authored/one/guide/image.png")).is_some());
    assert_eq!(assets.uncompiled(), &[path("authored/one/unselected.bin")]);
}

#[test]
fn retained_entry_is_exact_not_a_global_basename_or_dotfile_filter() {
    let compilation =
        workspace::compile(&configuration("['guide/.DS_Store']"), Default::default()).unwrap();
    let files = vec![
        ("authored/one/guide/index.md".into(), page("", "")),
        ("authored/one/guide/.DS_Store".into(), b"retained".to_vec()),
        (
            "authored/one/other/.DS_Store".into(),
            b"still unsupported".to_vec(),
        ),
        ("authored/one/.hidden".into(), b"still unsupported".to_vec()),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default()).unwrap();
    assert_eq!(
        resolved.assets_for(&key("site:one")).unwrap().uncompiled(),
        &[
            path("authored/one/.hidden"),
            path("authored/one/other/.DS_Store"),
        ]
    );
}

#[test]
fn absent_directory_markdown_and_notebook_cannot_be_retained_only() {
    for (entry, extra) in [
        ("missing", None),
        ("guide", None),
        ("plain.md", Some(b"# Plain\n".as_slice())),
        (
            "analysis.ipynb",
            Some(br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#.as_slice()),
        ),
    ] {
        let compilation =
            workspace::compile(&configuration(&format!("[{entry:?}]")), Default::default())
                .unwrap();
        let mut files = vec![("authored/one/guide/index.md".into(), page("", ""))];
        if let Some(bytes) = extra {
            files.push((format!("authored/one/{entry}"), bytes.to_vec()));
        }
        let corpus = parse(&files);
        assert!(
            matches!(authored::resolve(&compilation, &corpus, content::Limits::default()),
            Err(authored::Error::RetainedOnly { path: actual, .. }) if actual == path(&format!("authored/one/{entry}")))
        );
    }
}

#[test]
fn retention_paths_are_strict_unique_literal_source_coordinates() {
    for selection in [
        "['../escape']",
        "['/absolute']",
        "['guide/']",
        "['one','one']",
    ] {
        assert!(
            workspace::compile(&configuration(selection), Default::default()).is_err(),
            "{selection}"
        );
    }
}

#[test]
fn bindings_of_one_source_cannot_disagree_and_math_full_tree_cannot_consume_retention() {
    let conflicting = configuration("['metadata.bin']").replace(
        "retained_only=['metadata.bin']}]",
        "retained_only=['metadata.bin']},{source='source:one',target='authored/two'}]",
    );
    let workspace::Error::Invalid(errors) =
        workspace::compile(&conflicting, Default::default()).unwrap_err()
    else {
        panic!("native binding diagnostic")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_retained_only_conflict")
    );
    let typesetter = format!(
        "[math_typesetter]\nsource='source:one'\nentrypoint='typeset.mjs'\nprogram='/fixture/node'\ndirectory='/fixture'\nexecutable={{bytes=1,sha256='{}'}}\n{}",
        Sha256Digest::of(b"native executable"),
        configuration("['metadata.bin']"),
    );
    let workspace::Error::Invalid(errors) =
        workspace::compile(&typesetter, Default::default()).unwrap_err()
    else {
        panic!("native tree dependency diagnostic")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_retained_only_dependency")
    );
}

#[test]
fn explicit_resource_dependency_cannot_consume_retained_only_bytes() {
    let compilation =
        workspace::compile(&configuration("['guide/dep.ts']"), Default::default()).unwrap();
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page(
                "[[resources]]\ninputs=['guide/main.ts','guide/dep.ts']\nbuild={kind='browser-module',entry='guide/main.ts'}\noutput='/guide/main.js'\n",
                "",
            ),
        ),
        (
            "authored/one/guide/main.ts".into(),
            b"import './dep.ts';".to_vec(),
        ),
        (
            "authored/one/guide/dep.ts".into(),
            b"export const answer=42;".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    assert!(
        matches!(authored::resolve(&compilation, &corpus, content::Limits::default()),
        Err(authored::Error::RetainedOnly { path: actual, .. }) if actual == path("authored/one/guide/dep.ts"))
    );
}

#[test]
fn relative_link_refuses_retained_opaque_target_instead_of_passing_it_through() {
    let compilation =
        workspace::compile(&configuration("['guide/metadata.bin']"), Default::default()).unwrap();
    let files = vec![
        (
            "authored/one/guide/index.md".into(),
            page("", "[Metadata](metadata.bin)\n"),
        ),
        (
            "authored/one/guide/metadata.bin".into(),
            b"retained".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default()).unwrap();
    let error = resolved
        .project_page(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("relative link targets an explicitly retained-only input")
    );
}

#[test]
fn complete_candidate_keeps_retained_bytes_excluded_and_cannot_reinclude_them() {
    let compilation =
        workspace::compile(&configuration("['guide/metadata.bin']"), Default::default()).unwrap();
    let files = vec![
        ("authored/one/guide/index.md".into(), page("", "")),
        (
            "authored/one/guide/metadata.bin".into(),
            b"retained".to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default()).unwrap();
    let mut selection = resolved.non_draft_selection().unwrap();
    let retained = path("authored/one/guide/metadata.bin");
    assert_eq!(selection.assets[&retained], Disposition::Exclude);
    selection
        .assets
        .insert(retained.clone(), Disposition::Include);
    assert!(
        matches!(resolved.into_selected(&selection), Err(SelectionError::Asset { path: actual, reason: "retained-only inputs must remain explicitly excluded" }) if actual == retained)
    );
    selection.assets.insert(retained, Disposition::Exclude);
    authored::resolve(&compilation, &corpus, content::Limits::default())
        .unwrap()
        .into_selected(&selection)
        .expect("complete explicit exclusion remains valid");
}
