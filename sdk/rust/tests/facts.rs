//! The native local composition refuses external selectors explicitly.

use gazette_sdk::{facts, toml, workspace};

const EXTERNAL: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[fact_sources]]\ntarget={kind='principal-name',principal='owner:one'}\nsource={kind='external',binding='input:external'}\n";

#[test]
fn local_composition_distinguishes_unconfigured_from_unsupported() {
    let compilation = workspace::compile(EXTERNAL, toml::Limits::default()).expect("workspace");
    for (provider, expected) in [
        (
            facts::LocalProvider::Unconfigured,
            "publication_facts_unconfigured",
        ),
        (
            facts::LocalProvider::Unsupported,
            "publication_facts_unsupported",
        ),
    ] {
        let error = facts::acquire(&provider, &compilation.workspace, facts::Limits::default())
            .expect_err("explicit local refusal");
        assert_eq!(error.code(), expected);
        assert_eq!(
            compilation
                .workspace
                .principals()
                .next()
                .expect("principal")
                .name,
            None
        );
    }
}
