//! Owner-native discussion resolution through real configuration and authored projection.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::Key;
use serde_json::{Value, json};

const ATOM: &str = "discussion={capability='discussion.render/v1',repository_id='site-repo-id',category_id='site-category',options={theme='light'}}\n";

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn configuration() -> String {
    format!(
        "[composition]\ntheme={{package='theme:one'}}\n\
         [composition.plugins]\n'discussion-plugin'={{state='enabled'}}\n\
         [composition.capabilities]\n\
         'discussion.render/v1'={{state='enabled',provider={{kind='plugin',package='discussion-plugin',provider='discussion'}},requirement='required'}}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n{ATOM}\
         [[sources]]\nid='source:one'\nlocation={{kind='repository',root='/private/not-opened',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n\
         [[theme_packages]]\npackage='theme:one'\nsource='source:one'\nmanifest='theme.toml'\n\
         [[plugin_packages]]\npackage='discussion-plugin'\nsource='source:one'\nmanifest='plugin.toml'\n\
         [[brands]]\nid='brand:one'\nname='Publication'\n\
         [[brands.publications]]\nsite='site:one'\nrepository='https://public.example/site'\n\
         destination={{kind='git',remote='/private/delivery.git',branch='pages'}}\n\
         authored=[{{source='source:one',target='authored'}}]\n\
         [[projects]]\nid='project:a'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         repository='https://public.example/a'\n\
         discussion={{capability='discussion.render/v1',repository_id='a-repo-id',category_id='a-category',options={{theme='dark'}}}}\n\
         [[projects]]\nid='project:b'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         repository='https://public.example/b'\n\
         discussion={{capability='discussion.render/v1',repository_id='b-repo-id',category_id='b-category'}}\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:a'\nrender=true\ndiscoverable=true\n\
         documents={{source='source:one',target='native/a'}}\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:b'\nrender=true\ndiscoverable=true\n\
         documents={{source='source:one',target='native/b'}}\n\
         [[extensions]]\nid='schema:business'\nschema={{type='object',additionalProperties=false}}\n"
    )
}

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, Default::default()).expect("whole discussion configuration")
}

fn invalid(source: &str, code: &str, owner: &str) {
    let workspace::Error::Invalid(errors) = workspace::compile(source, Default::default())
        .expect_err("invalid discussion configuration")
    else {
        panic!("expected semantic diagnostic");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == code && error.key.as_ref() == Some(&key(owner))),
        "{errors:?}"
    );
}

fn document(id: &str, structure: &str, relations: &str) -> String {
    format!(
        "+++\n{relations}[content]\nid='{id}'\nsite='site:one'\ntitle='{id}'\nlifecycle='published'\n\
         structure={{{structure}}}\n+++\nBody.\n"
    )
}

fn files() -> Vec<(&'static str, String)> {
    let article = "kind='page',family={kind='article'}";
    let docs = "kind='page',family={kind='documentation'}";
    vec![
        (
            "authored/article.md",
            document(
                "content:article",
                article,
                "projects=[{project='project:b',kind='mentions'}]\n",
            ),
        ),
        (
            "native/a/guide.md",
            document(
                "content:a",
                docs,
                "projects=[{project='project:b',kind='about'}]\n",
            ),
        ),
        ("native/b/guide.md", document("content:b", docs, "")),
        (
            "authored/docs.md",
            document("content:authored-docs", docs, ""),
        ),
        (
            "authored/index.md",
            document("content:index", "kind='page',family={kind='index'}", ""),
        ),
        (
            "native/a/project.md",
            document("content:project", "kind='page',family={kind='project'}", ""),
        ),
        (
            "native/a/legal.md",
            document("content:utility", "kind='page',family={kind='utility'}", ""),
        ),
        (
            "native/a/business.md",
            document(
                "content:business",
                "kind='page',family={kind='extension',schema='schema:business',data={}}",
                "",
            ),
        ),
        (
            "authored/home.md",
            document("collection:home", "kind='collection'", ""),
        ),
        (
            "authored/journey.md",
            document(
                "journey:one",
                "kind='journey',collection='collection:home',members=[{content='content:article',primary=true}]",
                "",
            ),
        ),
    ]
}

fn projected(configuration: &str) -> Value {
    let compiled = compile(configuration);
    let files = files();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().expect("source path"),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained authored syntax");
    let resolved =
        authored::resolve(&compiled, &corpus, Default::default()).expect("shared authored model");
    let discovery = resolved.discovery().expect("exact source discovery");
    let mut result = serde_json::Map::new();
    for node in resolved.content().graph().nodes() {
        let page = resolved
            .project_page(
                &node.id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("actual page projection");
        let direct = serde_json::to_value(&page).expect("direct projection");
        let public = serde_json::to_value(page.discovery_view(&discovery).expect("bound view"))
            .expect("public serializer");
        let search = serde_json::to_value(
            page.discovery_view(&discovery)
                .expect("bound view")
                .search()
                .expect("discoverable"),
        )
        .expect("search serializer");
        assert_eq!(direct.get("discussion"), public.get("discussion"));
        assert_eq!(direct.get("discussion"), search.get("discussion"));
        result.insert(node.id.as_str().into(), public);
    }
    Value::Object(result)
}

#[test]
fn complete_atoms_require_public_repositories_and_exact_selected_capabilities() {
    compile(&configuration());
    for (needle, replacement, code, owner) in [
        (
            "repository='https://public.example/site'\n",
            "",
            "workspace_discussion",
            "site:one",
        ),
        (
            "repository='https://public.example/a'\n",
            "",
            "workspace_discussion",
            "project:a",
        ),
        (
            "category_id='site-category'",
            "category_id=''",
            "workspace_discussion",
            "site:one",
        ),
        (
            "repository_id='a-repo-id'",
            "repository_id='a repo'",
            "workspace_discussion",
            "project:a",
        ),
        (
            "options={theme='light'}",
            "options={'bad key'='light'}",
            "workspace_discussion",
            "site:one",
        ),
        (
            "capability='discussion.render/v1',repository_id='a-repo-id'",
            "capability='discussion.missing/v1',repository_id='a-repo-id'",
            "workspace_discussion_capability",
            "project:a",
        ),
    ] {
        invalid(&configuration().replace(needle, replacement), code, owner);
    }
    for repository in [
        "file:///private/repo",
        "https://user:secret@host.example/repo",
        "https://example.test/white space",
    ] {
        let source = configuration().replace("https://public.example/site", repository);
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "{repository}"
        );
    }
    let missing = configuration().replace(",category_id='a-category'", "");
    assert!(
        matches!(
            workspace::compile(&missing, Default::default()),
            Err(workspace::Error::Toml(_))
        ),
        "partial atom must not deserialize as an absent/default category"
    );
}

#[test]
fn every_rendered_project_placement_requires_its_exact_capability_selection() {
    let extra = "\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n\
        [sites.composition.capabilities]\n'discussion.render/v1'={state='disabled'}\n\
        [[placements]]\nrole='mirror'\nsite='site:other'\nproject='project:a'\nrender=true\ndiscoverable=false\n\
        documents={source='source:one',target='other/a'}\n";
    compile(&(configuration() + extra));
    let omitted = configuration().replace(
        "'discussion.render/v1'={state='enabled',provider={kind='plugin',package='discussion-plugin',provider='discussion'},requirement='required'}\n",
        "",
    );
    invalid(&omitted, "workspace_discussion_capability", "site:one");
    invalid(&omitted, "workspace_discussion_capability", "project:a");
}

#[test]
fn provenance_selects_whole_owner_atoms_without_subject_or_presentation_inference() {
    let pages = projected(&configuration());
    for (id, owner, repository, repository_id, category_id, options) in [
        (
            "content:article",
            "site:one",
            "https://public.example/site",
            "site-repo-id",
            "site-category",
            json!({"theme":"light"}),
        ),
        (
            "content:a",
            "project:a",
            "https://public.example/a",
            "a-repo-id",
            "a-category",
            json!({"theme":"dark"}),
        ),
        (
            "content:b",
            "project:b",
            "https://public.example/b",
            "b-repo-id",
            "b-category",
            json!({}),
        ),
    ] {
        assert_eq!(
            pages[id]["discussion"],
            json!({
                "owner":owner, "repository":repository,
                "binding":{"capability":"discussion.render/v1","repository_id":repository_id,
                           "category_id":category_id,"options":options}
            })
        );
    }
    assert!(!pages.to_string().contains("/private/delivery"));
    for id in [
        "content:authored-docs",
        "content:index",
        "content:project",
        "content:utility",
        "content:business",
        "collection:home",
        "journey:one",
    ] {
        assert!(
            pages[id].get("discussion").is_none(),
            "excluded family {id}"
        );
    }
}

#[test]
fn absent_project_atom_never_borrows_site_and_disabled_capability_suppresses_bindings() {
    let source = configuration().replace(
        "discussion={capability='discussion.render/v1',repository_id='a-repo-id',category_id='a-category',options={theme='dark'}}\n",
        "",
    );
    let pages = projected(&source);
    assert!(pages["content:a"].get("discussion").is_none());
    assert_eq!(pages["content:article"]["discussion"]["owner"], "site:one");
    assert_eq!(pages["content:b"]["discussion"]["owner"], "project:b");
    let disabled = source.replace(
        "state='enabled',provider={kind='plugin',package='discussion-plugin',provider='discussion'},requirement='required'",
        "state='disabled'",
    );
    for page in projected(&disabled).as_object().expect("pages").values() {
        assert!(page.get("discussion").is_none());
    }
}
