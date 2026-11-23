//! Route derivation failures are actionable in the same editor analysis.
use gazette_sdk::authoring::{self, Input, diagnostics};

#[test]
fn missing_slug_diagnostic_recovers_from_the_current_document_edit() {
    let workspace = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/not-opened'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\nrouting={articles='/blog/'}\nauthored=[{source='source:one',target='selected'}]\n";
    let path: gazette_sdk::source::EntryPath = "selected/index.md".parse().unwrap();
    for slug in ["", "slug='one'"] {
        let source = format!(
            "+++\n[content]\nid='content:one'\ntitle='One'\nlifecycle='draft'\n{slug}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# One\n"
        );
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
        let rows = diagnostics(&analysis, &path, &source, 1 << 20).unwrap();
        if slug.is_empty() {
            let row = rows
                .iter()
                .find(|row| row.code == "content_route_slug")
                .unwrap();
            assert!(row.message.contains("content.slug"));
            assert!(
                row.location
                    .unwrap()
                    .span
                    .slice(&source)
                    .unwrap()
                    .contains("[content]")
            );
        } else {
            assert!(analysis.structurally_valid());
            assert!(rows.is_empty());
        }
    }
}
