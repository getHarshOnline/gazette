//! Selected native syntax and complete retained math across plain/framed corpus inputs.

use std::collections::BTreeMap;

use gazette_adapter_markdown::Comrak;
use gazette_compiler::{
    corpus::{
        self, File, FileError, Input, LocatedMathResult, MathLocation, MathResult, ParsedCorpus,
    },
    document::{self, ParsedDocument},
    markdown, toml, workspace,
};
use gazette_core::{
    markdown::{Error as AdapterError, MathBinding, Profile},
    source::{EntryPath, Sha256Digest},
};

// Small retained generator fixtures shared in meaning with markdown_math.rs.
// No generator, executable, filesystem or transport is involved in pure compilation.
const INLINE: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x</annotation></semantics></math>"#;
const DISPLAY: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><msup><mi>y</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">y^2</annotation></semantics></math>"#;
const FRAMED: &str = "+++\r\ntitle='Metadata $not_math$'\r\n+++\r\nBefore $x$ and `$literal$`.\r\n\r\n<!-- more -->\r\n\r\nAfter $$y^2$$.\r\n";
const PLAIN: &str = "Plain $x$ and $$y^2$$.\n";
const ORDINARY: &str = "Unselected $x$ and $$y^2$$.\n\n```math\ny^2\n```\n";

fn path(value: &str) -> EntryPath {
    value.parse().expect("explicit corpus path")
}

fn profiles() -> BTreeMap<EntryPath, Profile> {
    BTreeMap::from([
        (path("a-framed.md"), Profile::PublicationMath),
        (path("b-plain.md"), Profile::PublicationMath),
    ])
}

fn parse() -> ParsedCorpus<'static> {
    ParsedCorpus::parse_with_profiles(
        [
            ("a-framed.md", FRAMED.as_bytes()),
            ("b-plain.md", PLAIN.as_bytes()),
            ("c-ordinary.md", ORDINARY.as_bytes()),
            ("z.bin", &[0xff, 0, 0x80]),
        ]
        .into_iter()
        .map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        &profiles(),
        corpus::Limits::default(),
        &Comrak,
    )
    .expect("complete mixed corpus")
}

fn results(corpus: &ParsedCorpus<'_>) -> BTreeMap<EntryPath, Vec<LocatedMathResult>> {
    corpus
        .files()
        .iter()
        .filter_map(|(path, file)| {
            let body = file.body()?;
            let bindings: Vec<_> = body
                .math()
                .iter()
                .filter(|fact| fact.emitted)
                .map(|fact| {
                    let mathml = match (fact.tex.as_str(), fact.display) {
                        ("x", false) => INLINE,
                        ("y^2", true) => DISPLAY,
                        _ => panic!("fixture has no retained generator result for {fact:?}"),
                    };
                    MathBinding {
                        occurrence: fact.clone(),
                        mathml: mathml.to_owned(),
                    }
                })
                .collect();
            (!bindings.is_empty()).then(|| {
                (
                    path.clone(),
                    vec![LocatedMathResult {
                        location: MathLocation::Body,
                        result: MathResult {
                            identity: body.identity(),
                            bindings,
                        },
                    }],
                )
            })
        })
        .collect()
}

#[test]
fn mixed_profiles_keep_exact_body_coordinates_and_ordinary_dollars_literal() {
    let corpus = parse();
    let File::Document(framed) = &corpus.files()["a-framed.md"] else {
        panic!("framed role")
    };
    let direct = ParsedDocument::parse_with_profile(
        FRAMED,
        Profile::PublicationMath,
        document::Limits::default(),
        &Comrak,
    )
    .expect("same native document entry point");
    assert_eq!(framed.source(), FRAMED);
    assert_eq!(framed.source_sha256(), Sha256Digest::of(FRAMED.as_bytes()));
    assert_eq!(framed.body_span(), direct.body_span());
    assert_eq!(framed.body().math(), direct.body().math());
    assert_eq!(
        framed.body().math().len(),
        2,
        "metadata never becomes math input"
    );
    assert_eq!(framed.body().identity().profile, Profile::PublicationMath);
    assert_ne!(framed.body().source_sha256(), framed.source_sha256());
    assert_eq!(
        framed.body_span().slice(FRAMED),
        Some(framed.body().source())
    );
    for (fact, original) in framed.body().math().iter().zip(["$x$", "$$y^2$$"]) {
        assert_eq!(
            fact.location.span.slice(framed.body().source()),
            Some(original)
        );
        let full = fact
            .location
            .span
            .in_parent(FRAMED, framed.body_span())
            .expect("lift native body span");
        assert_eq!(full.slice(FRAMED), Some(original));
        assert_eq!(
            full.start,
            framed.body_span().start + fact.location.span.start
        );
    }
    let plain = corpus.files()["b-plain.md"].body().expect("plain body");
    assert_eq!(plain.source(), PLAIN);
    assert_eq!(
        plain.identity().source_sha256,
        Sha256Digest::of(PLAIN.as_bytes())
    );
    assert_eq!(plain.math()[0].location.span.slice(PLAIN), Some("$x$"));
    assert!(plain.render_html().is_err(), "parse is not typesetting");
    let ordinary = corpus.files()["c-ordinary.md"]
        .body()
        .expect("ordinary body");
    assert_eq!(ordinary.identity().profile, Profile::Publication);
    assert!(ordinary.math().is_empty());
    let html = ordinary
        .render_html()
        .expect("ordinary rendering needs no math results");
    assert!(html.contains("$x$ and $$y^2$$"));
    assert!(html.contains("class=\"language-math\""));
    assert!(!html.contains("<math"));
    assert!(matches!(corpus.files()["z.bin"], File::Asset(_)));
}

#[test]
fn complete_results_feed_body_excerpt_and_text_without_reparsing_or_cross_file_leakage() {
    let corpus = parse();
    let before = corpus.inspect().expect("retained syntax report");
    let retained = results(&corpus);
    let corpus = corpus
        .with_math(retained.clone())
        .expect("complete retained result set");
    assert_eq!(
        corpus.inspect().expect("same syntax after attachment"),
        before
    );
    let body = corpus.files()["a-framed.md"].body().expect("framed body");
    let output = body
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("pure projected body");
    assert!(output.body_html.contains(INLINE));
    assert!(output.body_html.contains(DISPLAY));
    assert!(output.body_html.contains("<code>$literal$</code>"));
    assert!(output.body_text.contains("Before x and $literal$."));
    assert!(output.body_text.contains("After y^2."));
    assert!(!output.body_text.contains("MathML"));
    let excerpt = output.summary.as_ref().expect("explicit excerpt marker");
    assert!(excerpt.html.contains(INLINE));
    assert!(!excerpt.html.contains(DISPLAY));
    assert!(excerpt.text.contains("Before x"));
    assert!(!excerpt.text.contains("After"));
    assert_eq!(
        output,
        body.render_resolving_references(|_, _, _| Ok(None))
            .expect("retained repeat")
    );
    let second = parse()
        .with_math(retained)
        .expect("independent replay of same captured results");
    assert_eq!(
        output,
        second.files()["a-framed.md"]
            .body()
            .unwrap()
            .render_resolving_references(|_, _, _| Ok(None))
            .unwrap()
    );
    let plain = corpus.files()["b-plain.md"]
        .body()
        .unwrap()
        .render_html()
        .unwrap();
    assert!(plain.contains(INLINE) && plain.contains(DISPLAY));
    assert!(!plain.contains("Before"));
    let ordinary = corpus.files()["c-ordinary.md"]
        .body()
        .unwrap()
        .render_html()
        .unwrap();
    assert!(ordinary.contains("$x$") && !ordinary.contains("<math"));
}

#[test]
fn missing_extra_unknown_asset_and_inapplicable_results_refuse_the_whole_corpus() {
    let corpus = parse();
    let complete = results(&corpus);
    for name in ["a-framed.md", "b-plain.md"] {
        let mut selected = complete.clone();
        selected.remove(&path(name));
        assert!(
            matches!(parse().with_math(selected), Err(corpus::Error::Binding(ref rejected)) if rejected == &path(name))
        );
    }
    for name in ["unknown.md", "c-ordinary.md", "z.bin"] {
        let mut selected = complete.clone();
        selected.insert(path(name), complete[&path("b-plain.md")].clone());
        assert!(
            matches!(parse().with_math(selected), Err(corpus::Error::Binding(ref rejected)) if rejected == &path(name))
        );
    }
    let mut unknown_profile = profiles();
    unknown_profile.insert(path("absent.md"), Profile::PublicationMath);
    assert!(matches!(ParsedCorpus::parse_with_profiles(
        [Input { path: path("a-framed.md"), bytes: FRAMED.as_bytes() }, Input { path: path("b-plain.md"), bytes: PLAIN.as_bytes() }],
        &unknown_profile, corpus::Limits::default(), &Comrak,
    ), Err(corpus::Error::Binding(ref rejected)) if rejected == &path("absent.md")));
}

#[test]
fn late_source_profile_and_occurrence_mismatches_never_return_a_partially_bound_corpus() {
    let complete = results(&parse());
    for correction in 0..5 {
        let mut selected = complete.clone();
        let last = &mut selected.get_mut(&path("b-plain.md")).unwrap()[0].result;
        match correction {
            0 => last.identity.source_sha256 = Sha256Digest::of(b"stale input"),
            1 => last.identity.profile = Profile::Publication,
            2 => {
                last.bindings.pop();
            }
            3 => last.bindings[1].occurrence.display = false,
            4 => last.bindings.swap(0, 1),
            _ => unreachable!(),
        }
        assert!(
            matches!(parse().with_math(selected), Err(corpus::Error::File {
            path: rejected,
            error: FileError::Markdown(markdown::Error::Adapter(AdapterError::InvalidPlan)),
        }) if rejected == path("b-plain.md")),
            "late mismatch {correction}"
        );
    }
    let mut whole_document_identity = complete;
    whole_document_identity
        .get_mut(&path("a-framed.md"))
        .unwrap()[0]
        .result
        .identity
        .source_sha256 = Sha256Digest::of(FRAMED.as_bytes());
    let body_start = ParsedDocument::parse_with_profile(
        FRAMED,
        Profile::PublicationMath,
        document::Limits::default(),
        &Comrak,
    )
    .unwrap()
    .body_span()
    .start;
    assert!(
        matches!(parse().with_math(whole_document_identity), Err(corpus::Error::File {
        path: rejected,
        error: FileError::Document(document::Error::Markdown { offset, error: markdown::Error::Adapter(AdapterError::InvalidPlan) }),
    }) if rejected == path("a-framed.md") && offset == body_start)
    );
}

#[test]
fn source_placement_configuration_owns_explicit_profile_and_omission_is_ordinary() {
    let base = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='repository',root='/not-opened/math',commit='0000000000000000000000000000000000000000',subdirectory='docs'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\n";
    for (setting, expected) in [
        ("", Profile::Publication),
        (
            ",markdown_profile='publication-math'",
            Profile::PublicationMath,
        ),
    ] {
        let source =
            format!("{base}authored=[{{source='source:one',target='authored'{setting}}}]\n");
        let compiled = workspace::compile(&source, toml::Limits::default())
            .expect("native configured placement");
        let binding = compiled.workspace.brands().next().unwrap().publications[0]
            .authored
            .first()
            .unwrap();
        assert_eq!(binding.markdown_profile, expected);
        for name in ["authored/page.md", "authored/nested/plain.markdown"] {
            assert_eq!(compiled.workspace.markdown_profile(&path(name)), expected);
        }
        for name in ["unbound/page.md", "authored-other/page.md"] {
            assert!(compiled.workspace.content_source(&path(name)).is_none());
            assert_eq!(
                compiled.workspace.markdown_profile(&path(name)),
                Profile::Publication
            );
        }
        let selected = BTreeMap::from([
            (
                path("authored/page.md"),
                compiled
                    .workspace
                    .markdown_profile(&path("authored/page.md")),
            ),
            (
                path("unbound/page.md"),
                compiled
                    .workspace
                    .markdown_profile(&path("unbound/page.md")),
            ),
        ]);
        let parsed = ParsedCorpus::parse_with_profiles(
            [
                Input {
                    path: path("authored/page.md"),
                    bytes: PLAIN.as_bytes(),
                },
                Input {
                    path: path("unbound/page.md"),
                    bytes: PLAIN.as_bytes(),
                },
            ],
            &selected,
            corpus::Limits::default(),
            &Comrak,
        )
        .expect("placement-selected syntax");
        assert_eq!(
            parsed.files()["authored/page.md"]
                .body()
                .unwrap()
                .math()
                .is_empty(),
            expected == Profile::Publication
        );
        let unbound = parsed.files()["unbound/page.md"].body().unwrap();
        assert_eq!(unbound.identity().profile, Profile::Publication);
        assert!(unbound.math().is_empty());
        assert!(unbound.render_html().unwrap().contains("$x$"));
    }
}
