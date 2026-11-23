//! Public authoring façade uses the same current-input compiler operation.

use gazette_sdk::{
    authoring::{self, Input, Limits},
    source::Sha256Digest,
};

#[test]
fn sdk_selection_and_current_semantics_match_the_shared_application() {
    let workspace = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let page = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n# One\n";
    let input = |path: &str, bytes| Input {
        path: path.parse().expect("valid selected fixture path"),
        bytes,
    };
    let public = authoring::analyze(
        input("workspace.toml", workspace.as_bytes()),
        [input("one.md", page.as_bytes())],
        Limits::default(),
    )
    .expect("SDK analyzes the selected workspace and page");
    let native = gazette_application::authoring::analyze(
        input("workspace.toml", workspace.as_bytes()),
        [input("one.md", page.as_bytes())],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native application analyzes the same selected inputs");
    assert!(public.structurally_valid());
    assert!(native.structurally_valid());
    assert_eq!(public.workspace(), native.workspace());
    assert_eq!(public.inputs(), native.inputs());
    assert_eq!(public.inputs()["one.md"], Sha256Digest::of(page.as_bytes()));
    let key = "one.md".parse().expect("valid selected page path");
    assert_eq!(
        public
            .headings(&key, page)
            .expect("SDK headings match current page")
            .values,
        native
            .headings(&key, page)
            .expect("native headings match current page")
            .values
    );
    assert_eq!(
        public
            .headings(&key, page)
            .expect("SDK body span matches current page")
            .body_span,
        native
            .headings(&key, page)
            .expect("native body span matches current page")
            .body_span
    );
}
