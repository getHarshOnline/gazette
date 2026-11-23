//! Missing source homes retain a useful current-document diagnostic.
use gazette_sdk::authoring::{self, Input, diagnostics};

#[test]
fn an_omitted_site_requires_a_source_binding_and_recovers_from_a_workspace_edit() {
    let source = "+++\n[content]\nid='content:one'\ntitle='One'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# One\n";
    let base = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n";
    let binding = "[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/not-opened'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:one',target='selected'}]\n";
    let path: gazette_sdk::source::EntryPath = "selected/index.md".parse().unwrap();
    for bound in [false, true] {
        let workspace = format!("{base}{}", if bound { binding } else { "" });
        let analysis = authoring::analyze(
            Input {
                path: "workspace.toml".parse().unwrap(),
                bytes: workspace.as_bytes(),
            },
            [Input {
                path: path.clone(),
                bytes: source.as_bytes(),
            }],
            Default::default(),
        )
        .unwrap();
        let rows = diagnostics(&analysis, &path, source, 1 << 20).unwrap();
        if bound {
            assert!(analysis.structurally_valid());
            assert!(rows.is_empty());
        } else {
            assert!(!analysis.structurally_valid());
            let row = rows
                .iter()
                .find(|row| row.code == "authored_site_binding_required")
                .unwrap();
            assert!(
                row.location
                    .unwrap()
                    .span
                    .slice(source)
                    .unwrap()
                    .contains("[content]")
            );
            assert!(!row.selection_wide);
        }
    }
}
