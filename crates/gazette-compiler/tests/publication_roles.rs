//! Explicit publication roles survive the native compiler and all scoped projections.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, ParsedWorkspace},
};
use gazette_core::publication::Key;
use serde_json::{Value, json};

fn key(value: &str) -> Key {
    value.parse().expect("literal fixture identity")
}

fn configuration(roles: &str) -> String {
    format!(
        "[[principals]]\nid='principal:publisher'\nkind='organization'\nname='Publisher'\n\
         [[principals]]\nid='principal:a'\nkind='organization'\nname='Exact controller'\n\
         [[principals]]\nid='principal:b'\nkind='person'\nname='Independent operator'\n\
         [[principals]]\nid='principal:unused'\nkind='person'\nname='Unrelated roster canary'\n\
         [[sites]]\nid='site:one'\npublisher='principal:publisher'\ndeployable=false\n{roles}\n\
         [[sites]]\nid='site:two'\npublisher='principal:publisher'\ndeployable=false\n"
    )
}

const ROLES: &str = "roles=[\
    {principal='principal:b',role='site-operator'},\
    {principal='principal:a',role='data-processor'},\
    {principal='principal:a',role='data-controller'},\
    {principal='principal:b',role='canon-authority'},\
    {principal='principal:b',role='data-controller'}]";

#[test]
fn exact_role_pairs_normalize_without_inheriting_publisher_or_sibling_roles() {
    let source = configuration(ROLES);
    let parsed = ParsedWorkspace::parse(&source, Default::default()).expect("explicit roles");
    let workspace = &parsed.compilation().workspace;
    let site = workspace.site(&key("site:one")).expect("first site");
    assert_eq!(
        serde_json::to_value(&site.roles).expect("native normalized pairs"),
        json!([
            {"principal":"principal:a","role":"data-controller"},
            {"principal":"principal:a","role":"data-processor"},
            {"principal":"principal:b","role":"canon-authority"},
            {"principal":"principal:b","role":"data-controller"},
            {"principal":"principal:b","role":"site-operator"}
        ])
    );
    assert_eq!(site.publisher, key("principal:publisher"));
    assert!(
        workspace
            .site(&key("site:two"))
            .expect("sibling")
            .roles
            .is_empty()
    );
    let reversed = "roles=[\
        {principal='principal:b',role='data-controller'},\
        {principal='principal:a',role='data-controller'},\
        {principal='principal:b',role='canon-authority'},\
        {principal='principal:a',role='data-processor'},\
        {principal='principal:b',role='site-operator'}]";
    let reordered = workspace::compile(&configuration(reversed), Default::default())
        .expect("same explicit set in another order");
    assert_eq!(*workspace, reordered.workspace);
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Site,
            identity: key("site:one"),
            field: vec![
                PathSegment::Key("roles".into()),
                PathSegment::Index(0),
                PathSegment::Key("principal".into()),
            ],
        })
        .expect("original unsorted field location");
    assert_eq!(&source[origin.span.start..origin.span.end], "'principal:b'");
    for roles in ["", "roles=[]"] {
        let plain = workspace::compile(&configuration(roles), Default::default())
            .expect("roles remain optional");
        assert!(plain.workspace.sites().all(|site| site.roles.is_empty()));
    }
}

#[test]
fn invalid_roles_refuse_complete_compilation_without_silent_deduplication() {
    for roles in [
        "roles=[{principal='principal:a',role='data-controller'},{principal='principal:a',role='data-controller'}]",
        "roles=[{principal='principal:missing',role='data-controller'}]",
        "roles=[{principal='site:two',role='site-operator'}]",
    ] {
        assert!(
            matches!(
                workspace::compile(&configuration(roles), Default::default()),
                Err(workspace::Error::Invalid(_))
            ),
            "semantic role error accepted: {roles}"
        );
    }
    for roles in [
        "roles=[{principal='principal:a',role='publisher'}]",
        "roles=[{principal='principal:a',role='data_controller'}]",
        "roles=[{principal='principal:a',role='author'}]",
        "roles=[{principal='principal:a',role='data-controller',inherited=true}]",
        "roles=[{role='data-controller'}]",
        "roles=[{principal='principal:a'}]",
        "roles='data-controller'",
    ] {
        assert!(
            matches!(
                workspace::compile(&configuration(roles), Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "malformed role declaration accepted: {roles}"
        );
    }
}

#[test]
fn roles_share_principal_identity_across_page_graph_and_json_ld_without_authority_inference() {
    let compilation = workspace::compile(&configuration(ROLES), Default::default())
        .expect("explicit shared role model");
    let documents = ["one", "two"].map(|site| format!(
        "+++\n[content]\nid='content:{site}'\nsite='site:{site}'\ntitle='Role fixture'\nlifecycle='draft'\nstructure={{kind='page',family={{kind='article'}}}}\n+++\nRetained body.\n"
    ));
    let corpus = ParsedCorpus::parse(
        documents.iter().enumerate().map(|(index, source)| Input {
            path: format!("page-{index}.md")
                .parse()
                .expect("native logical path"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native authored parser");
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("single resolved corpus");
    for site in ["one", "two"] {
        let site_id = key(&format!("site:{site}"));
        let declared = compilation
            .workspace
            .site(&site_id)
            .expect("configured site");
        let page = resolved
            .project_page(
                &key(&format!("content:{site}")),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("actual page projection");
        let graph = resolved
            .project_site_graph(&site_id, Default::default())
            .expect("bounded scoped graph");
        assert_eq!(page.data.site.roles.len(), declared.roles.len());
        for (view, binding) in page.data.site.roles.iter().zip(&declared.roles) {
            assert_eq!(view.role, binding.role);
            assert_eq!(view.principal.id, binding.principal);
            assert!(std::ptr::eq(
                view.principal,
                compilation
                    .workspace
                    .principal(&binding.principal)
                    .expect("sole native principal owner")
            ));
        }
        let page_json = serde_json::to_value(&page).expect("Theme page payload");
        let search = serde_json::to_value(page.search()).expect("shared search payload");
        let graph = serde_json::to_value(graph).expect("graph payload");
        let linked = serde_json::to_value(page.json_ld()).expect("JSON-LD payload");
        assert_eq!(page_json["site"]["roles"], search["site"]["roles"]);
        assert_eq!(
            graph["roles"],
            serde_json::to_value(&declared.roles).expect("original pairs")
        );
        let roles = linked["gazette:siteRoles"]["@list"]
            .as_array()
            .expect("explicit JSON-LD list");
        assert_eq!(roles.len(), declared.roles.len());
        for ((role, view), binding) in roles.iter().zip(&page.data.site.roles).zip(&declared.roles)
        {
            assert_eq!(
                role["gazette:role"],
                serde_json::to_value(binding.role).expect("exact role")
            );
            assert_eq!(
                role["gazette:principal"]["identifier"],
                binding.principal.as_str()
            );
            assert_eq!(role["gazette:site"]["identifier"], site_id.as_str());
            assert_eq!(
                role["gazette:principal"]["name"],
                view.principal.name.as_deref().expect("configured name")
            );
            assert_eq!(
                graph["principals"][binding.principal.as_str()],
                serde_json::to_value(view.principal).expect("same principal")
            );
            assert_eq!(role.as_object().expect("closed role projection").len(), 3);
        }
        assert_eq!(linked["publisher"]["identifier"], "principal:publisher");
        assert!(linked.get("author").is_none());
        assert!(linked.get("copyrightHolder").is_none());
        assert!(linked.get("memberOf").is_none());
        assert_eq!(linked["gazette:attributions"], json!({"@list":[]}));
        let expected_principals = if site == "one" {
            vec!["principal:a", "principal:b", "principal:publisher"]
        } else {
            vec!["principal:publisher"]
        };
        assert_eq!(
            graph["principals"]
                .as_object()
                .expect("scoped roster")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            expected_principals
        );
        for value in [&page_json, &search, &graph, &linked] {
            assert!(!value.to_string().contains("Unrelated roster canary"));
        }
        if site == "one" {
            let repeated: Vec<&Value> = roles
                .iter()
                .filter(|role| role["gazette:principal"]["identifier"] == "principal:a")
                .collect();
            assert_eq!(repeated.len(), 2);
            assert_eq!(
                repeated[0]["gazette:principal"],
                repeated[1]["gazette:principal"]
            );
            assert_ne!(repeated[0]["gazette:role"], repeated[1]["gazette:role"]);
        }
    }
}
