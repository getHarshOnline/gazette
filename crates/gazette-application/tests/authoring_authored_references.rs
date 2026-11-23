//! Shared authored-reference repair and reduced legal hints over real native inputs.

use gazette_application::authoring::{
    self, Analysis, AssistanceScope, Input, Limits, NavigationError,
};
use gazette_compiler::authored::AuthoredReferenceField;
use gazette_core::source::{EntryPath, Precision, Sha256Digest};

const ROOT: &str = "includes=['config/identities.toml']\r\n";
const IDENTITIES: &str = "principals=[{id=\"person:o\\u006ee\",kind='person'},{id='org:one',kind='organization'}]\r\nsites=[{id='site:one',publisher='person:one',deployable=false},{id='site:two',publisher='org:one',deployable=false}]\r\n";

fn path(value: &str) -> EntryPath {
    value.parse().unwrap()
}
fn input<'a>(path: &str, text: &'a str) -> Input<'a> {
    Input {
        path: self::path(path),
        bytes: text.as_bytes(),
    }
}
fn page(id: &str, site: &str, relations: &str) -> String {
    format!(
        "+++\r\n# é🦀 current\r\n{relations}[content]\r\nid='{id}'\r\nsite='{site}'\r\ntitle='org:one'\r\nlifecycle='draft'\r\nstructure={{kind='page',family={{kind='article'}}}}\r\nattributions=[{{principal='org:one',role='rights-holder'}}]\r\n+++\r\n# Body\r\n"
    )
}
fn analyze(files: &[(&str, &str)]) -> Analysis {
    authoring::analyze_workspace_documents_with_package_baselines(
        path("gazette.toml"),
        vec![
            input("gazette.toml", ROOT),
            input("config/identities.toml", IDENTITIES),
        ],
        files.iter().map(|(path, text)| input(path, text)),
        [],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

#[test]
fn subject_and_provider_completion_reuse_catalog_principals_without_changing_roles() {
    for (declaration, role) in [
        (
            "schema_type='AboutPage'\r\nsubject={kind='principal',principal='person:one'}",
            AuthoredReferenceField::SubjectPrincipal,
        ),
        (
            "subject={kind='service',id='service:one',name='Explicit',provider='person:one'}",
            AuthoredReferenceField::ServiceProvider,
        ),
    ] {
        let source = page("content:one", "site:one", "")
            .replace("family={kind='article'}", "family={kind='utility'}")
            .replace("[content]\r\n", &format!("[content]\r\n{declaration}\r\n"));
        let analysis = analyze(&[("one.md", &source)]);
        assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
        let completion = analysis
            .complete_authored_reference(
                &path("one.md"),
                &source,
                source.find("'person:one'").unwrap(),
                1 << 20,
            )
            .unwrap()
            .unwrap();
        assert_eq!(completion.field, role);
        assert_eq!(
            completion
                .candidates
                .iter()
                .map(|candidate| candidate.identity.as_str())
                .collect::<Vec<_>>(),
            ["person:one", "org:one"]
        );
        assert!(completion.current.is_some());
        assert_eq!(completion.preimage, "'person:one'");
    }
}

#[test]
fn native_authored_namespaces_preserve_exact_modular_declarations_and_cross_site_relatedness() {
    let page = page("content:one", "site:one", "related=['content:two']\r\n");
    let other = self::page("content:two", "site:two", "");
    let analysis = analyze(&[("owners/one.md", &page), ("owners/two.md", &other)]);
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    for (token, field, expected) in [
        (
            "'site:one'",
            AuthoredReferenceField::Site,
            vec!["site:one", "site:two"],
        ),
        (
            "'org:one'",
            AuthoredReferenceField::AttributionPrincipal,
            vec!["person:one", "org:one"],
        ),
        (
            "'content:two'",
            AuthoredReferenceField::RelatedContent,
            vec!["content:one", "content:two"],
        ),
    ] {
        let at = if field == AuthoredReferenceField::AttributionPrincipal {
            page.rfind(token).unwrap()
        } else {
            page.find(token).unwrap()
        };
        let value = analysis
            .complete_authored_reference(&path("owners/one.md"), &page, at, 1 << 20)
            .unwrap()
            .unwrap();
        assert_eq!(value.field, field);
        assert_eq!(value.preimage, token);
        assert_eq!(value.location.precision, Precision::Exact);
        assert_eq!(value.location.span.slice(&page), Some(token));
        assert_eq!(value.source_sha256, Sha256Digest::of(page.as_bytes()));
        assert_eq!(
            value
                .candidates
                .iter()
                .map(|candidate| candidate.identity.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(value.current.is_some());
        for candidate in &value.candidates {
            let source = if field == AuthoredReferenceField::RelatedContent {
                if candidate.identity.as_str() == "content:one" {
                    page.as_str()
                } else {
                    other.as_str()
                }
            } else {
                IDENTITIES
            };
            assert_eq!(candidate.source_sha256, Sha256Digest::of(source.as_bytes()));
            assert_eq!(
                candidate.location.span.slice(source),
                Some(candidate.replacement)
            );
            if field != AuthoredReferenceField::RelatedContent {
                assert_eq!(candidate.path.as_str(), "config/identities.toml");
            }
        }
        if field == AuthoredReferenceField::AttributionPrincipal {
            assert_eq!(value.candidates[0].replacement, "\"person:o\\u006ee\"");
        }
        let maximum = serde_json::to_vec(&value).unwrap().len();
        assert!(
            analysis
                .complete_authored_reference(&path("owners/one.md"), &page, at, maximum)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            analysis.complete_authored_reference(&path("owners/one.md"), &page, at, maximum - 1),
            Err(NavigationError::Limit)
        ));
    }
    let title = page.find("title='org:one'").unwrap() + "title=".len();
    assert!(
        analysis
            .complete_authored_reference(&path("owners/one.md"), &page, title, 1 << 20)
            .unwrap()
            .is_none()
    );
}

#[test]
fn current_unresolved_native_fields_offer_repairs_without_reusing_graph_or_stale_source() {
    let other = page("content:two", "site:two", "");
    for (page, token, field) in [
        (
            self::page("content:one", "site:typo", ""),
            "'site:typo'",
            AuthoredReferenceField::Site,
        ),
        (
            self::page("content:one", "site:one", "")
                .replace("principal='org:one'", "principal='org:typo'"),
            "'org:typo'",
            AuthoredReferenceField::AttributionPrincipal,
        ),
        (
            self::page("content:one", "site:one", "related=['content:typo']\r\n"),
            "'content:typo'",
            AuthoredReferenceField::RelatedContent,
        ),
    ] {
        let analysis = analyze(&[("one.md", &page), ("two.md", &other)]);
        assert!(!analysis.structurally_valid());
        let at = page.find(token).unwrap();
        let value = analysis
            .complete_authored_reference(&path("one.md"), &page, at, 1 << 20)
            .unwrap()
            .unwrap();
        assert_eq!(value.field, field);
        assert!(value.current.is_none());
        assert!(!value.candidates.is_empty());
        assert!(matches!(
            analysis.complete_authored_reference(
                &path("one.md"),
                &(page.clone() + "changed"),
                at,
                1 << 20
            ),
            Err(NavigationError::InvalidSource)
        ));
    }
    let incomplete = "+++\r\n[content]\r\nsite='site:typo'\r\ntitle=\r\n";
    let analysis = analyze(&[("one.md", incomplete)]);
    assert!(!analysis.structurally_valid());
    let at = incomplete.find("site:typo").unwrap();
    let repairs = analysis
        .complete_authored_reference(&path("one.md"), incomplete, at, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(
        repairs.candidates.len(),
        2,
        "sound native scalar assistance survives incomplete neighboring TOML"
    );
    assert_eq!(repairs.preimage, "'site:typo'");
    assert_eq!(
        repairs.location.span.slice(incomplete),
        Some(repairs.preimage)
    );
    assert_eq!(repairs.location.precision, Precision::Exact);
    assert_eq!(
        repairs.source_sha256,
        Sha256Digest::of(incomplete.as_bytes())
    );
    assert!(repairs.current.is_none());
    assert!(matches!(
        analysis.complete_authored_reference(
            &path("one.md"),
            &(incomplete.to_owned() + "changed"),
            at,
            1 << 20
        ),
        Err(NavigationError::InvalidSource)
    ));
    let exact_bytes = serde_json::to_vec(&repairs).unwrap().len();
    assert!(matches!(
        analysis.complete_authored_reference(&path("one.md"), incomplete, at, exact_bytes - 1),
        Err(NavigationError::Limit)
    ));
}

const LEGAL_WORKSPACE: &str = "principals=[{id='person:one',kind='person'}]\nsites=[{id='site:one',publisher='person:one',deployable=false}]\nsources=[{id='source:legal',location={kind='filesystem',root='.'}},{id='source:content',location={kind='filesystem',root='.'}}]\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\nauthored=[{source='source:content',target='content'}]\nlegal=[{source='source:legal',entry='terms.md',source_content='content:owner-terms',id='content:site-terms',route='/terms/',family='utility'}]\n";
const LEGAL_PATH: &str = "__legal/source:legal/terms.md";
const LEGAL: &str =
    "+++\n[content]\nid='content:owner-terms'\ntitle='Terms'\nlifecycle='draft'\n+++\n# Terms\n";
fn legal_analysis(legal: &str, article: &str, collection: &str) -> Analysis {
    authoring::analyze(
        input("workspace.toml", LEGAL_WORKSPACE),
        [
            input(LEGAL_PATH, legal),
            input("content/one/index.md", article),
            input("content/collection.md", collection),
        ],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
fn collection() -> String {
    page("collection:one", "site:one", "").replace(
        "{kind='page',family={kind='article'}}",
        "{kind='collection'}",
    )
}

#[test]
fn selected_reduced_legal_source_preserves_editorial_facts_and_composed_related_output_identity() {
    let article = page(
        "content:one",
        "site:one",
        "collections=['collection:one']\r\nrelated=['content:site-terms']\r\n",
    )
    .replace("principal='org:one'", "principal='person:one'");
    let collection = collection().replace("principal='org:one'", "principal='person:one'");
    let analysis = legal_analysis(LEGAL, &article, &collection);
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let editorial = analysis
        .complete_editorial_reference(
            &path("content/one/index.md"),
            &article,
            article.find("collection:one").unwrap(),
            1 << 20,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        editorial.candidates.len(),
        1,
        "reduced legal metadata is not parsed as an ordinary editorial declaration"
    );
    let proposal = analysis
        .complete_authored_reference(
            &path("content/one/index.md"),
            &article,
            article.find("content:site-terms").unwrap(),
            1 << 20,
        )
        .unwrap()
        .unwrap();
    let legal = proposal
        .candidates
        .iter()
        .find(|candidate| candidate.identity.as_str() == "content:site-terms")
        .unwrap();
    assert_eq!(legal.path.as_str(), LEGAL_PATH);
    assert_eq!(legal.source_sha256, Sha256Digest::of(LEGAL.as_bytes()));
    assert_eq!(legal.location.precision, Precision::Construct);
    assert_eq!(legal.replacement, "\"content:site-terms\"");
    assert!(
        !proposal
            .candidates
            .iter()
            .any(|candidate| candidate.identity.as_str() == "content:owner-terms")
    );
    let broken = article.replace("'content:site-terms'", "'content:typo'");
    let invalid = legal_analysis(LEGAL, &broken, &collection);
    assert!(
        matches!(
            invalid.complete_authored_reference(
                &path("content/one/index.md"),
                &broken,
                broken.find("content:typo").unwrap(),
                1 << 20
            ),
            Err(NavigationError::Unavailable)
        ),
        "failed derived namespace cannot silently omit the legal output"
    );
}

#[test]
fn legal_hint_profile_comes_from_selected_binding_not_filename_or_current_syntax_validity() {
    let article = page("content:one", "site:one", "")
        .replace("principal='org:one'", "principal='person:one'");
    let collection = collection().replace("principal='org:one'", "principal='person:one'");
    let incomplete = "+++\r\n# é🦀\r\n[content]\r\n";
    let analysis = legal_analysis(incomplete, &article, &collection);
    assert!(!analysis.structurally_valid());
    let help = analysis
        .assistance(&path(LEGAL_PATH), incomplete, incomplete.len(), 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(help.scope, AssistanceScope::LegalSourceFrontmatter);
    let fields = help
        .help
        .candidates
        .iter()
        .map(|value| value.label.as_str())
        .collect::<Vec<_>>();
    assert!(fields.contains(&"title") && fields.contains(&"lifecycle"));
    assert!(!fields.contains(&"site") && !fields.contains(&"structure"));
    assert_eq!(help.source_sha256, Sha256Digest::of(incomplete.as_bytes()));
    let ordinary = analyze(&[("notes/terms.md", incomplete)]);
    let help = ordinary
        .assistance(
            &path("notes/terms.md"),
            incomplete,
            incomplete.len(),
            1 << 20,
        )
        .unwrap()
        .unwrap();
    assert_eq!(help.scope, AssistanceScope::AuthoredFrontmatter);
    assert!(
        help.help
            .candidates
            .iter()
            .any(|candidate| candidate.label == "site")
    );
}

#[test]
fn browsing_reference_completion_uses_only_resolved_native_routed_indexes() {
    let page = page("content:one", "site:one", "browsing=['content:index']\r\n");
    let index = self::page("content:index", "site:one", "")
        .replace("family={kind='article'}", "family={kind='index'}")
        .replace(
            "lifecycle='draft'",
            "lifecycle='draft'\r\nroute={path='/index/'}",
        );
    let article = self::page("content:article", "site:one", "");
    let unrouted = self::page("content:unrouted", "site:one", "")
        .replace("family={kind='article'}", "family={kind='index'}");
    let workspace = format!(
        "{ROOT}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\n"
    );
    let analysis = authoring::analyze_workspace_documents_with_package_baselines(
        path("gazette.toml"),
        vec![
            input("gazette.toml", &workspace),
            input("config/identities.toml", IDENTITIES),
        ],
        [
            input("one.md", &page),
            input("index.md", &index),
            input("article.md", &article),
            input("unrouted.md", &unrouted),
        ],
        [],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let at = page.find("'content:index'").unwrap();
    let result = analysis
        .complete_authored_reference(&path("one.md"), &page, at, 1 << 20)
        .unwrap()
        .unwrap();
    assert_eq!(result.field, AuthoredReferenceField::BrowsingIndex);
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|candidate| candidate.identity.as_str())
            .collect::<Vec<_>>(),
        ["content:index"]
    );
    assert!(result.current.is_some());
}
