//! Candidate selection reaches native capture, actual resource adapters and all outputs.

use gazette_sdk::{
    authored,
    publication::{Key, content::Disposition},
};

use super::{Fixture, PAGE, Value, acquire, build, bytes, fs, render, source, tempdir};

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn decisions(publication: &build::ResolvedPublication<'_, '_>) -> authored::Selection {
    authored::Selection {
        content: publication
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| {
                let decision = if ["collection:home", "content:kept"].contains(&node.id.as_str()) {
                    Disposition::Include
                } else {
                    Disposition::Exclude
                };
                (node.id.clone(), decision)
            })
            .collect(),
        assets: publication
            .authored()
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| {
                matches!(
                    file,
                    gazette_sdk::corpus::File::Asset(_) | gazette_sdk::corpus::File::Notebook(_)
                )
            })
            .map(|(path, _)| {
                (
                    path.clone(),
                    if path.as_str() == "one/public.txt" {
                        Disposition::Include
                    } else {
                        Disposition::Exclude
                    },
                )
            })
            .collect(),
    }
}

fn selected_render<'s>(
    publication: &build::SelectedPublication<'_, 's>,
) -> Result<build::PublicationRender<'s>, build::PublicationRenderError> {
    build::render_selected_publication(
        publication,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("machine model")),
            graph: Some(build::GraphRenderOptions {
                output: &"graph.json".parse().expect("graph output"),
                limits: Default::default(),
            }),
            limits: Default::default(),
        },
        Default::default(),
    )
}

#[test]
fn selected_candidate_reaches_all_outputs_without_excluded_programs_or_cached_assets() {
    let bad_page = PAGE.replacen("[content]\n", "[[resources]]\ninputs=['bad.ts']\nbuild={kind='browser-module',entry='bad.ts'}\noutput='/bad.js'\n[content]\n", 1);
    let kept = "+++\ncollections=['collection:home']\n[content]\nid='content:kept'\nsite='site:one'\ntitle='Kept archive'\nlifecycle='archived'\nroute={path='/kept/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained archive\n[Public attachment](public.txt)\n";
    let template = "<h1>{{ content.title }}</h1>{{ body_html | safe }}{% if json_ld is defined %}<script type='application/ld+json'>{{ json_ld | json_script }}</script>{% endif %}";
    let fixture = Fixture::new(&[
        ("one/page.md", bad_page.as_bytes()),
        ("one/kept.md", kept.as_bytes()),
        ("one/bad.ts", b"export const = invalid program"),
        ("one/private.txt", b"EXCLUDED ASSET BODY"),
        ("one/public.txt", b"included asset"),
        ("theme/templates/graph.html", b"{{ __tera_context }}"),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = fixture.config()
        .replace("authored=[{source='source:one',target='one'}]", "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]")
        .replacen("public_base_url=", &format!("machine={{sitemap=true,{},search='lexical',json_ld=true,feeds={{identity_base='https://identity.example/',channels=[{{id='feed:json',channel='collection:home',path='/feed.json',format='json',scope={{kind='site'}}}}]}}}}\npublic_base_url=", super::curated_llms("collection:home", &[("Retained reading", &["collection:home", "content:kept"])])), 1);
    let inputs = acquire(&config);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed.resolve(Default::default()).expect("whole graph");
    let review = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("private review before selection");
    let private = serde_json::to_string(review.request()).expect("private review values");
    assert!(private.contains("content:one"));
    assert!(private.contains("private.txt"));
    // Prove the excluded program is genuinely invalid at the production adapter.
    assert!(
        render(&inputs).is_err(),
        "unfiltered rendering must not silently skip invalid resource compilation"
    );
    let selection = decisions(&resolved);
    let selection_sha256 = source::Sha256Digest::of(
        &serde_json::to_vec(&selection).expect("complete include/exclude intent"),
    );
    let selected = resolved.into_selected(selection).expect("closed candidate");
    assert!(selected.authored().file(&key("content:one")).is_none());
    let selected_review = build::review_selected(&selected, Default::default())
        .expect("review uses the selected native projections without compiling excluded programs");
    assert_eq!(review.request().selection_sha256(), None);
    assert_eq!(
        selected_review.request().selection_sha256(),
        Some(selection_sha256)
    );
    let selected_surfaces = serde_json::to_string(selected_review.request().surfaces())
        .expect("selected review surfaces");
    assert!(selected_surfaces.contains("content:kept"));
    assert!(selected_surfaces.contains("Retained archive"));
    assert!(selected_surfaces.contains("public.txt"));
    for absent in [
        "content:one",
        "content:other",
        "private.txt",
        "bad.ts",
        "bad.js",
    ] {
        assert!(
            !selected_surfaces.contains(absent),
            "selected review retained excluded material {absent}"
        );
    }
    assert_ne!(review.sha256(), selected_review.sha256());
    let rendered = selected_render(&selected).expect("all selected outputs");
    assert!(std::ptr::eq(rendered.inputs(), &inputs));
    assert_eq!(
        rendered.selection().expect("private intent").content[&key("content:one")],
        Disposition::Exclude
    );
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("same page model");
    let search: Value =
        serde_json::from_slice(bytes(tree, "search.json")).expect("same search set");
    let feed: Value = serde_json::from_slice(bytes(tree, "feed.json"))
        .expect("feed uses selected graph identity");
    assert_eq!(model["pages"].as_object().expect("page map").len(), 2);
    assert_eq!(search.as_array().expect("search documents").len(), 2);
    assert_eq!(feed["items"].as_array().expect("feed items").len(), 1);
    assert_eq!(
        feed["items"][0]["_gazette"]["content"],
        model["pages"]["content:kept"]["content"]
    );
    assert_eq!(
        model["pages"]["content:kept"]["content"]["lifecycle"],
        "archived"
    );
    assert_eq!(bytes(tree, "assets/public.txt"), b"included asset");
    for path in [
        "guide/index.html",
        "old.html",
        "assets/private.txt",
        "bad.js",
        "elsewhere/index.html",
    ] {
        assert!(
            tree.file(&path.parse().expect("excluded path")).is_none(),
            "excluded output {path}"
        );
    }
    for entry in tree.inventory().entries() {
        let Some(output) = tree.file(&entry.path) else {
            continue;
        };
        let text = String::from_utf8_lossy(output);
        for absent in [
            "content:one",
            "content:other",
            "EXCLUDED ASSET BODY",
            "bad.js",
            "private.txt",
        ] {
            assert!(!text.contains(absent), "{} leaked {absent}", entry.path);
        }
    }
    assert_eq!(
        std::str::from_utf8(bytes(tree, "sitemap.xml"))
            .expect("XML")
            .matches("<loc>")
            .count(),
        2
    );
    assert_eq!(
        std::str::from_utf8(bytes(tree, "llms.txt"))
            .expect("discovery")
            .lines()
            .filter(|line| line.starts_with("- ["))
            .count(),
        2
    );
    let stage_parent = tempdir().expect("private staging parent");
    let staged = build::stage_publication(
        &rendered,
        stage_parent.path(),
        source::Limits::default(),
        16 << 10,
    )
    .expect("native selected tree stage");
    drop(staged);
    fs::write(
        fixture.directory.path().join("one/home.md"),
        "changed native bytes after capture",
    )
    .expect("disposable mutation");
    let repeated = selected_render(&selected).expect("retained candidate rebuild");
    assert_eq!(
        tree.inventory().identity(),
        repeated.site().output().inventory().identity()
    );
    let repeated_review = build::review_selected(&selected, Default::default())
        .expect("review only retained selected input after native mutation");
    assert_eq!(selected_review.sha256(), repeated_review.sha256());
    assert_eq!(
        serde_json::to_vec(selected_review.request()).expect("original private request"),
        serde_json::to_vec(repeated_review.request()).expect("repeated private request")
    );
}

#[test]
fn all_included_review_binds_candidate_intent_without_changing_surfaces_or_accepting_old_response()
{
    let published = PAGE.replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = Fixture::new(&[("one/page.md", published.as_bytes())]);
    let inputs = acquire(&fixture.config());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("native retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("native content graph");
    let original = build::review(&resolved, Default::default()).expect("unselected review");
    let mut selection = decisions(&resolved);
    selection
        .content
        .values_mut()
        .for_each(|value| *value = Disposition::Include);
    selection
        .assets
        .values_mut()
        .for_each(|value| *value = Disposition::Include);
    let selection_sha256 = source::Sha256Digest::of(
        &serde_json::to_vec(&selection).expect("full no-op candidate intent"),
    );
    let selected = resolved
        .into_selected(selection)
        .expect("all included candidate");
    let review = build::review_selected(&selected, Default::default()).expect("selected review");
    let error = build::review_selected(
        &selected,
        build::ReviewLimits {
            bytes: 0,
            ..Default::default()
        },
    )
    .expect_err("full selection encoding is bounded before any surface projection");
    assert!(matches!(
        error,
        build::ReviewError::Encoding(gazette_sdk::render::Error::Limit("review selection bytes"))
    ));
    assert_eq!(original.request().selection_sha256(), None);
    assert_eq!(review.request().selection_sha256(), Some(selection_sha256));
    assert_eq!(
        serde_json::to_value(original.request().surfaces()).expect("original projections"),
        serde_json::to_value(review.request().surfaces()).expect("same selected projections")
    );
    assert_ne!(
        original.sha256(),
        review.sha256(),
        "selection is bound even with identical surfaces"
    );

    let policy = build::ReviewPolicy {
        id: key("policy:fixture"),
        revision: key("revision:fixture"),
        source_sha256: source::Sha256Digest::of(b"synthetic unverified policy fixture"),
    };
    let decisions = original
        .request()
        .surfaces()
        .iter()
        .enumerate()
        .flat_map(|(surface, projection)| {
            projection.uses().map(move |usage| {
                serde_json::json!({"surface":surface,"pointer":usage.pointer,
                "outcome":"allow","evidence":["evidence:fixture"]})
            })
        })
        .collect::<Vec<_>>();
    let response = serde_json::to_vec(&serde_json::json!({
        "format":"gazette-review-response/1", "request_sha256":original.sha256(),
        "policy":policy, "decisions":decisions
    }))
    .expect("complete preselection response");
    let binding = build::ReviewResponseBinding {
        policy,
        response_sha256: source::Sha256Digest::of(&response),
    };
    let matched = original
        .match_response(&response, &binding, Default::default())
        .expect("positive control: response covers every original use");
    assert_eq!(
        serde_json::to_value(matched).expect("matched report")["status"],
        "matched-unverified"
    );
    let error = review
        .match_response(&response, &binding, Default::default())
        .expect_err("identical surfaces do not authorize replay under a new selection binding");
    assert!(matches!(
        error,
        build::ReviewResponseError::Binding("review request")
    ));

    let uses = review
        .request()
        .surfaces()
        .iter()
        .map(|surface| surface.uses().count())
        .sum::<usize>();
    let pointers = review
        .request()
        .surfaces()
        .iter()
        .flat_map(|surface| surface.uses())
        .map(|usage| usage.pointer.len())
        .sum::<usize>();
    let bytes = serde_json::to_vec(review.request())
        .expect("complete selected request")
        .len()
        .max(pointers);
    let exact = build::ReviewLimits {
        bytes,
        uses,
        graph: Default::default(),
    };
    assert_eq!(
        build::review_selected(&selected, exact)
            .expect("exact complete budgets")
            .sha256(),
        review.sha256()
    );
    for limits in [
        build::ReviewLimits {
            bytes: bytes - 1,
            ..exact
        },
        build::ReviewLimits {
            uses: uses - 1,
            ..exact
        },
    ] {
        assert_eq!(
            build::review_selected(&selected, limits)
                .expect_err("no successful truncated selection review")
                .code(),
            "publication_review_limit"
        );
    }
}

#[test]
fn empty_candidate_remains_an_explicit_site_failure() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("syntax");
    let resolved = parsed.resolve(Default::default()).expect("graph");
    let mut selection = decisions(&resolved);
    selection
        .content
        .values_mut()
        .for_each(|decision| *decision = Disposition::Exclude);
    let selected = resolved
        .into_selected(selection)
        .expect("empty structural candidate");
    let error =
        selected_render(&selected).expect_err("requested empty site cannot silently vanish");
    assert_eq!(error.code(), "site_empty");
    assert!(
        selected
            .authored()
            .content()
            .graph()
            .nodes()
            .next()
            .is_none()
    );
}
