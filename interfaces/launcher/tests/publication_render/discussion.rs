//! Retained source ownership feeds Theme and machine discussion metadata together.

use super::*;
use serde_json::json;

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\nresource_requirements=['module:discussion']\n";
const PLUGIN: &str = "package='fixture-plugin'\n[extensions.'fixture-plugin.discussion/v1']\nschema='discussion.schema.json'\n[resources.'module:discussion']\ninputs=['discussion.js']\nbuild={kind='browser-module',entry='discussion.js'}\n[providers.discussion]\nimplements=['fixture-plugin.discussion/v1']\n";
const TEMPLATE: &str = "<h1>{{ content.title }}</h1>{{ body_html | safe }}{% if discussion is defined %}<script type=\"application/json\" id=\"discussion\">{{ discussion | json_script }}</script>{% endif %}{% for resource in presentation.theme_resources %}<script data-site-resource=\"{{ resource.id }}\" type=\"module\" src=\"{{ resource.url }}\"></script>{% endfor %}";
const SELECTION: &str = "[composition.plugins]\n\
    'fixture-plugin'={state='enabled'}\n\
    [composition.capabilities]\n\
    'fixture-plugin.discussion/v1'={state='enabled',provider={kind='plugin',package='fixture-plugin',provider='discussion'},requirement='optional',allow_absence='unsupported'}\n\
    [composition.resources]\n\
    'module:discussion'={state='enabled',resource={owner={kind='plugin',package='fixture-plugin'},resource='module:discussion'},output='/assets/discussion.js',use='site'}\n";
const SITE_ATOM: &str = "discussion={capability='fixture-plugin.discussion/v1',repository_id='site-repo-id',category_id='site-category',options={theme='light'}}\n";

fn document(id: &str, route: &str, family: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='{id}'\nlifecycle='published'\n\
         route={{path='{route}'}}\nstructure={{kind='page',family={{{family}}}}}\n+++\nNative body.\n"
    )
}

fn fixture() -> Fixture {
    let manifest = MANIFEST.replace(
        "render={page='page.html'}",
        "render={page='page.html',alias='page.html'}",
    );
    let article = PAGE.replace(
        "[content]\n",
        "projects=[{project='project:b',kind='mentions'}]\n[content]\n",
    );
    let a = document("doc:a", "/docs/a/", "kind='documentation'").replace(
        "[content]\n",
        "projects=[{project='project:b',kind='about'}]\n[content]\n",
    );
    let b = document("doc:b", "/docs/b/", "kind='documentation'");
    let missing = document("doc:missing", "/docs/missing/", "kind='documentation'");
    let authored = document("doc:authored", "/authored-docs/", "kind='documentation'");
    let utility = document("page:utility", "/legal/", "kind='utility'");
    let index = document("page:index", "/index-page/", "kind='index'");
    let project = document("page:project", "/project-page/", "kind='project'");
    let extension = document(
        "page:business",
        "/business/",
        "kind='extension',schema='schema:business',data={}",
    );
    Fixture::new(&[
        ("one/page.md", article.as_bytes()),
        ("one/documentation.md", authored.as_bytes()),
        ("one/legal.md", utility.as_bytes()),
        ("one/index.md", index.as_bytes()),
        ("one/project.md", project.as_bytes()),
        ("one/business.md", extension.as_bytes()),
        ("project/a/guide.md", a.as_bytes()),
        ("project/b/guide.md", b.as_bytes()),
        ("project/missing/guide.md", missing.as_bytes()),
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/plugin.toml", PLUGIN.as_bytes()),
        ("theme/discussion.schema.json", b"{}"),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
        // This selected resource only proves the existing compiler/provider seam;
        // it neither calls a service nor pretends to activate a discussion widget.
        ("theme/discussion.js", b"export const fixture = 'retained';"),
    ])
}

fn configured(fixture: &Fixture) -> String {
    let mut config = fixture
        .config()
        .replacen("[[principals]]", &format!("{SELECTION}[[principals]]"), 1)
        .replacen(
            "deployable=false\n",
            &format!("deployable=false\n{SITE_ATOM}"),
            1,
        )
        .replacen(
            "public_base_url=",
            "repository='https://public.example/site'\n\
            destination={kind='git',remote='/private/discussion-delivery.git',branch='pages'}\n\
            machine={search='lexical'}\npublic_base_url=",
            1,
        );
    for name in ["a", "b", "missing"] {
        config.push_str(&format!(
            "[[projects]]\nid='project:{name}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
             repository='https://public.example/{name}'\n"
        ));
        if name != "missing" {
            config.push_str(&format!(
                "discussion={{capability='fixture-plugin.discussion/v1',repository_id='{name}-repo-id',category_id='{name}-category',options={{theme='{name}'}}}}\n"
            ));
        }
        config.push_str(&format!(
            "[[sources]]\nid='source:{name}'\n\
             location={{kind='repository',root={:?},commit='{}',subdirectory='project/{name}'}}\n\
             [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:{name}'\nrender=true\ndiscoverable=true\n\
             documents={{source='source:{name}',target='native/{name}'}}\n",
            fixture.directory.path().to_str().expect("owned source"), fixture.commit
        ));
    }
    config.push_str(
        "[[extensions]]\nid='schema:business'\nschema={type='object',additionalProperties=false}\n\
         [[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n",
    );
    config
}

fn html_binding(tree: &source::CapturedSource, page: &Value) -> Option<Value> {
    let html = std::str::from_utf8(bytes(
        tree,
        page["route"]["output"].as_str().expect("exact page output"),
    ))
    .expect("Theme HTML");
    let marker = "<script type=\"application/json\" id=\"discussion\">";
    html.split_once(marker).map(|(_, rest)| {
        let (json, tail) = rest.split_once("</script>").expect("complete JSON script");
        assert!(!tail.contains(marker), "exactly one source-owned binding");
        serde_json::from_str(json).expect("actual Theme transports same binding")
    })
}

#[test]
fn source_owned_discussions_share_theme_model_search_cli_and_retained_replay() {
    let fixture = fixture();
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete retained publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("machine model");
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("search corpus");
    let resource = &model["resources"]["module:discussion"];
    assert_eq!(resource["output"], "assets/discussion.js");
    assert_eq!(model["pages"].as_object().expect("all pages").len(), 10);
    assert_eq!(search.len(), 10);
    for (id, owner, repo, category) in [
        ("content:one", "site:one", "site", "site-category"),
        ("doc:a", "project:a", "a", "a-category"),
        ("doc:b", "project:b", "b", "b-category"),
    ] {
        let page = &model["pages"][id];
        let binding = &page["discussion"];
        assert_eq!(binding["owner"], owner);
        assert_eq!(
            binding["repository"],
            format!("https://public.example/{repo}")
        );
        assert_eq!(
            binding["binding"]["repository_id"],
            format!("{repo}-repo-id")
        );
        assert_eq!(binding["binding"]["category_id"], category);
        assert_eq!(
            binding["binding"]["capability"],
            "fixture-plugin.discussion/v1"
        );
        let theme = if repo == "site" { "light" } else { repo };
        assert_eq!(binding["binding"]["options"], json!({"theme":theme}));
        assert_eq!(
            page["presentation"]["theme_resources"][0]["id"],
            "module:discussion"
        );
        assert_eq!(html_binding(tree, page).as_ref(), Some(binding));
        let indexed = search
            .iter()
            .find(|record| record["content"]["id"] == id)
            .expect("actual search record");
        assert_eq!(&indexed["discussion"], binding);
    }
    for id in [
        "doc:missing",
        "doc:authored",
        "page:utility",
        "page:index",
        "page:project",
        "page:business",
        "collection:home",
    ] {
        let page = &model["pages"][id];
        assert!(
            page.get("discussion").is_none(),
            "{id} must not borrow a site atom"
        );
        assert!(html_binding(tree, page).is_none());
        assert!(
            search
                .iter()
                .find(|record| record["content"]["id"] == id)
                .expect("actual excluded-family record")
                .get("discussion")
                .is_none()
        );
    }
    for name in [
        "model.json",
        "search.json",
        "guide/index.html",
        "docs/a/index.html",
    ] {
        let text = std::str::from_utf8(bytes(tree, name)).expect("public text");
        assert!(!text.contains("/private/discussion-delivery.git"), "{name}");
        assert!(
            !text.contains(fixture.directory.path().to_str().expect("private source")),
            "{name}"
        );
    }
    let parent = tempdir().expect("isolated stage");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(receipt["inventory"], json!(tree.inventory()));
    let stage = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for name in [
        "model.json",
        "search.json",
        "guide/index.html",
        "docs/a/index.html",
        "docs/b/index.html",
    ] {
        assert_eq!(
            fs::read(stage.join(name)).expect("CLI staged output"),
            bytes(tree, name)
        );
    }
    fixture
        .directory
        .close()
        .expect("remove only owned input fixture");
    let repeated = render(&inputs).expect("captured replay without source/provider rereads");
    assert_eq!(
        tree.inventory().identity(),
        repeated.site().output().inventory().identity()
    );
    assert_eq!(
        bytes(tree, "model.json"),
        bytes(repeated.site().output(), "model.json")
    );
}

#[test]
fn disabled_capability_omits_discussion_without_rewriting_the_site_resource_selection() {
    let fixture = fixture();
    let config = configured(&fixture).replace(
        "state='enabled',provider={kind='plugin',package='fixture-plugin',provider='discussion'},requirement='optional',allow_absence='unsupported'",
        "state='disabled'",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("explicitly disabled discussion");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("shared search");
    for page in model["pages"].as_object().expect("all pages").values() {
        assert!(page.get("discussion").is_none());
        assert_eq!(
            page["presentation"]["theme_resources"][0]["id"],
            "module:discussion"
        );
        assert!(html_binding(tree, page).is_none());
    }
    assert!(search.iter().all(|page| page.get("discussion").is_none()));
    assert!(
        tree.file(&"assets/discussion.js".parse().expect("resource path"))
            .is_some(),
        "resource selection is independent from disabled semantic capability"
    );
    assert!(
        model["capabilities"]
            .get("fixture-plugin.discussion/v1")
            .is_none()
    );
}
