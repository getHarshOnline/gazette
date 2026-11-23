//! Notebook cells and stored Markdown compose through the same source-bound math owner.

use gazette_adapter_markdown::Comrak;
use gazette_compiler::{
    corpus::{self, File, Input, LocatedMathResult, MathLocation, MathResult, ParsedCorpus},
    markdown,
    notebook::ParsedNotebook,
};
use gazette_core::{
    markdown::{MathBinding, Profile},
    raster,
    source::{EntryPath, Sha256Digest},
};
use serde_json::json;
use std::collections::BTreeMap;

const INLINE: &str = r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><mi>x</mi></mrow><annotation encoding="application/x-tex">x</annotation></semantics></math>"#;

fn input() -> Vec<u8> {
    serde_json::to_vec(&json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[
        {"id":"markdown","cell_type":"markdown","metadata":{},"source":["# Cell\n", "Inline $x$ and `$literal$`.\n"]},
        {"id":"code","cell_type":"code","metadata":{},"execution_count":null,"source":"print('$not_math$')", "outputs":[
            {"output_type":"stream","name":"stdout","text":"$plain$"},
            {"output_type":"display_data","metadata":{},"data":{"text/markdown":["# Output\n", "Stored $x$.\n"],"text/plain":"Literal $plain$."}}
        ]}
    ]})).unwrap()
}

fn path() -> EntryPath {
    "book.ipynb".parse().unwrap()
}

fn parse(input: &[u8], limits: corpus::Limits) -> Result<ParsedCorpus<'_>, corpus::Error> {
    ParsedCorpus::parse_with_profiles(
        [Input {
            path: path(),
            bytes: input,
        }],
        &BTreeMap::from([(path(), Profile::PublicationMath)]),
        limits,
        &Comrak,
    )
}

fn results(corpus: &ParsedCorpus<'_>) -> BTreeMap<EntryPath, Vec<LocatedMathResult>> {
    BTreeMap::from([(
        path(),
        corpus.files()[&path()]
            .math_sources()
            .map(|(location, identity, facts)| LocatedMathResult {
                location,
                result: MathResult {
                    identity,
                    bindings: facts
                        .iter()
                        .filter(|fact| fact.emitted)
                        .map(|fact| MathBinding {
                            occurrence: fact.clone(),
                            mathml: INLINE.into(),
                        })
                        .collect(),
                },
            })
            .collect(),
    )])
}

fn render(
    corpus: &ParsedCorpus<'_>,
) -> Result<gazette_compiler::notebook::RenderedNotebook, gazette_compiler::notebook::RenderError> {
    let File::Notebook(notebook) = &corpus.files()[&path()] else {
        panic!("notebook")
    };
    notebook.render_static("book", Default::default(), &raster::unavailable, &Comrak)
}

#[test]
fn selected_cells_and_stored_outputs_bind_independently_and_render_native_mathml() {
    let input = input();
    let corpus = parse(&input, Default::default()).unwrap();
    let sources: Vec<_> = corpus.files()[&path()].math_sources().collect();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].0, MathLocation::NotebookCell { cell: 0 });
    assert_eq!(
        sources[1].0,
        MathLocation::NotebookOutput { cell: 1, output: 1 }
    );
    assert_ne!(sources[0].1, sources[1].1);
    assert!(sources.iter().all(|(_, identity, facts)| identity.profile
        == Profile::PublicationMath
        && facts.len() == 1
        && facts[0].tex == "x"));
    assert!(
        render(&corpus).is_err(),
        "syntax does not invent typesetting"
    );
    let mut retained = results(&corpus);
    retained.get_mut(&path()).unwrap().reverse();
    let bound = corpus.with_math(retained).unwrap();
    let rendered = render(&bound).unwrap();
    assert_eq!(rendered.html.matches("<math ").count(), 2);
    assert!(rendered.html.contains("book-cell-0-cell"));
    assert!(rendered.html.contains("book-cell-1-output-1-output"));
    assert!(rendered.text.contains("print('$not_math$')"));
    assert!(rendered.text.contains("Literal $plain$."));
    assert_eq!(rendered.source_sha256, Sha256Digest::of(&input));
    assert_eq!(rendered, render(&bound).unwrap());
}

#[test]
fn ordinary_notebooks_keep_dollars_literal_without_math_results() {
    let input = input();
    let corpus = ParsedCorpus::parse(
        [Input {
            path: path(),
            bytes: &input,
        }],
        Default::default(),
        &Comrak,
    )
    .unwrap();
    assert!(
        corpus.files()[&path()]
            .math_sources()
            .all(
                |(_, identity, facts)| identity.profile == Profile::Publication && facts.is_empty()
            )
    );
    let output = render(&corpus.with_math(BTreeMap::new()).unwrap()).unwrap();
    assert_eq!(output.html.matches("<math ").count(), 0);
    assert!(output.text.contains("Inline $x$"));
}

#[test]
fn missing_extra_duplicate_or_stale_fragment_results_refuse_without_partial_corpus() {
    let input = input();
    let complete = results(&parse(&input, Default::default()).unwrap());
    for case in 0..8 {
        let mut changed = complete.clone();
        let selected = changed.get_mut(&path()).unwrap();
        match case {
            0 => {
                selected.pop();
            }
            1 => selected.push(selected[0].clone()),
            2 => selected[1].location = selected[0].location,
            3 => selected[1].location = MathLocation::NotebookOutput { cell: 1, output: 0 },
            4 => selected[1].result.identity.source_sha256 = Sha256Digest::of(b"stale"),
            5 => selected[1].result.identity.profile = Profile::Publication,
            6 => selected[1].result.bindings[0].occurrence.display = true,
            7 => selected[1].result.bindings.clear(),
            _ => unreachable!(),
        }
        assert!(
            parse(&input, Default::default())
                .unwrap()
                .with_math(changed)
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn notebook_math_still_uses_native_fragment_admission_and_aggregate_binding_bounds() {
    let input = input();
    let corpus = parse(&input, Default::default()).unwrap();
    let mut retained = results(&corpus);
    retained.get_mut(&path()).unwrap()[1].result.bindings[0].mathml =
        "<script>alert(1)</script>".into();
    let bound = corpus.with_math(retained).unwrap();
    assert!(
        render(&bound).is_err(),
        "source binding cannot admit active arbitrary markup"
    );
    let mut limits = corpus::Limits::default();
    limits.document.markdown.output_bytes = INLINE.len() + 1;
    let corpus = parse(&input, limits).unwrap();
    let retained = results(&corpus);
    assert!(
        corpus.with_math(retained).is_err(),
        "one notebook budget, not one allowance per cell"
    );
}

#[test]
fn notebook_markdown_nodes_charge_cells_outputs_and_the_whole_corpus() {
    let input = input();
    let notebook = ParsedNotebook::parse_with_profile(
        &input,
        Profile::PublicationMath,
        Default::default(),
        Default::default(),
        &Comrak,
    )
    .unwrap();
    let nodes = notebook.markdown_node_count();
    assert!(nodes > 2);
    let mut limits = corpus::Limits {
        markdown_nodes: nodes,
        ..corpus::Limits::default()
    };
    assert!(parse(&input, limits).is_ok());
    limits.markdown_nodes = nodes - 1;
    assert!(parse(&input, limits).is_err());
    limits.markdown_nodes = nodes;
    assert!(
        ParsedCorpus::parse_with_profiles(
            [
                Input {
                    path: path(),
                    bytes: &input
                },
                Input {
                    path: "later.md".parse().unwrap(),
                    bytes: b"# More"
                }
            ],
            &BTreeMap::from([(path(), Profile::PublicationMath)]),
            limits,
            &Comrak
        )
        .is_err()
    );
    let tiny = markdown::Limits {
        nodes: nodes - 1,
        ..Default::default()
    };
    assert!(
        ParsedNotebook::parse_with_profile(
            &input,
            Profile::PublicationMath,
            Default::default(),
            tiny,
            &Comrak
        )
        .is_err()
    );
}
