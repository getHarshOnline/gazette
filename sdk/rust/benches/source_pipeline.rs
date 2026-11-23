//! Synthetic scaling evidence for real SDK operations, not publication certification.

use gazette_sdk::{document, source};
use serde_json::json;
use std::{collections::BTreeMap, fs, hint::black_box, path::Path, time::Instant};
use tempfile::tempdir;

const PARAGRAPHS: usize = 16;
const PARAGRAPH: &str = "A bounded paragraph with **emphasis**, `code` and a [reference](https://example.invalid/).\n\n";
const RENDERED_PARAGRAPH: &str = "<p>A bounded paragraph with <strong>emphasis</strong>, <code>code</code> and a <a href=\"https://example.invalid/\">reference</a>.</p>\n";
const ASSET_BYTES: usize = 4096;

fn main() {
    let supplied = std::env::args().skip(1).collect::<Vec<_>>();
    let cargo_test = supplied
        .iter()
        .any(|argument| argument.starts_with("--test-threads="));
    let args: Vec<_> = supplied
        .into_iter()
        .filter(|argument| argument != "--bench" && !argument.starts_with("--test-threads="))
        .collect();
    let (scale_args, retain) = match args.last().map(String::as_str) {
        Some("--retain") => (&args[..args.len() - 1], true),
        _ => (args.as_slice(), false),
    };
    let scales: Vec<usize> = match scale_args {
        [] if cargo_test => vec![1],
        [] => vec![1, 10, 100],
        [scale] => match scale.as_str() {
            "1" => vec![1],
            "10" => vec![10],
            "100" => vec![100],
            _ => usage(),
        },
        _ => usage(),
    };
    for scale in scales {
        run(scale, retain);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: source_pipeline [1|10|100] [--retain]; creates disposable sources and stages only"
    );
    std::process::exit(2);
}

fn run(scale: usize, retain: bool) {
    let authored = tempdir().expect("disposable authored root");
    let documents = 10 * scale;
    let paths = corpus(authored.path(), documents);
    let bounds = source::Limits::default();
    let original =
        source::inspect(authored.path(), None, bounds).expect("complete fixture inventory");
    assert_eq!(original.entries().len(), 2 * documents + 3);
    let prepared = source::prepare(
        Vec::new(),
        source::PreparationLimits {
            sources: 0,
            entries: 0,
            bytes: 0,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("explicit authored-only composition");
    let context = source::StageContext {
        workspace: "benchmark-workspace".into(),
        site: "benchmark-site".into(),
        placements: BTreeMap::new(),
    };
    let mut samples = Vec::new();
    let mut expected_html_bytes = None;
    for sample in 0..3 {
        let parent = tempdir().expect("disjoint disposable staging parent");
        let (checked, check_us) = measured(|| {
            source::check_stage(&prepared, &context, authored.path(), parent.path(), bounds)
                .expect("read-only complete check")
        });
        assert_eq!(checked, original);
        let (stage, stage_us) = measured(|| {
            source::stage(
                &prepared,
                &context,
                authored.path(),
                source::StageLocation::Fresh(parent.path()),
                bounds,
            )
            .expect("fresh native stage and read-back")
        });
        assert_eq!(stage.inventory(), &checked);
        let (captured, capture_us) = measured(|| {
            source::capture_pinned(stage.path(), checked.identity(), bounds)
                .expect("independently pinned complete capture")
        });
        let (html_bytes, compile_us) = measured(|| compile(&captured, &paths));
        assert_eq!(*expected_html_bytes.get_or_insert(html_bytes), html_bytes);
        let (retain_us, cleanup_us) = if retain {
            let (retained, retain_us) =
                measured(|| stage.retain().expect("synchronized retention"));
            assert_eq!(retained.inventory(), &checked);
            let path = retained.path().to_owned();
            drop(retained);
            assert_eq!(
                source::inspect(&path, None, bounds).expect("independent retained read-back"),
                checked
            );
            // The fixture owns this disposable parent. RetainedStage grants no
            // cleanup authority; this is not the product's checked close path.
            let ((), cleanup_us) = measured(|| parent.close().expect("fixture-owned cleanup"));
            (Some(retain_us), cleanup_us)
        } else {
            let ((), cleanup_us) =
                measured(|| stage.close().expect("verified cleanup of owned stage"));
            assert!(
                fs::read_dir(parent.path())
                    .expect("parent unchanged")
                    .next()
                    .is_none()
            );
            parent.close().expect("empty fixture parent cleanup");
            (None, cleanup_us)
        };
        samples.push(json!({
            "sample": sample,
            "check_us": check_us,
            "stage_us": stage_us,
            "capture_us": capture_us,
            "parse_inspect_render_us": compile_us,
            "retain_us": retain_us,
            "cleanup_us": cleanup_us,
            "html_bytes": html_bytes,
        }));
    }
    assert_eq!(
        source::inspect(authored.path(), None, bounds).expect("source preserved"),
        original
    );
    println!(
        "{}",
        json!({
            "corpus": "gazette.source-pipeline-benchmark.v1",
            "retained": retain,
            "cleanup_kind": if retain { "fixture_parent" } else { "checked_stage_close" },
            "scale": scale,
            "documents": documents,
            "assets": documents,
            "asset_bytes_each": ASSET_BYTES,
            "tree_identity": original.identity(),
            "entries": original.entries().len(),
            "source_bytes": original.total_bytes(),
            "samples": samples,
        })
    );
}

fn corpus(root: &Path, documents: usize) -> Vec<source::EntryPath> {
    fs::create_dir(root.join("docs")).expect("docs");
    fs::create_dir(root.join("assets")).expect("assets");
    fs::write(
        root.join(".fixture"),
        b"synthetic non-publication fixture\n",
    )
    .expect("hidden metadata");
    (0..documents)
        .map(|index| {
            let name = format!("docs/{index:04}.md");
            let text = format!(
                "+++\ntitle = 'Opaque metadata {index}'\n+++\n# Document {index}\n\n{}",
                PARAGRAPH.repeat(PARAGRAPHS)
            );
            fs::write(root.join(&name), text).expect("fixture document");
            fs::write(
                root.join(format!("assets/{index:04}.bin")),
                [0x81; ASSET_BYTES],
            )
            .expect("binary asset");
            name.parse().expect("literal document selector")
        })
        .collect()
}

fn compile(captured: &source::CapturedSource, paths: &[source::EntryPath]) -> usize {
    let mut html_bytes = 0;
    for (index, path) in paths.iter().enumerate() {
        let bytes = captured
            .file(path)
            .expect("every explicit document retained");
        let parsed = document::ParsedDocument::parse(
            std::str::from_utf8(bytes).expect("UTF8"),
            document::Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("same production document parser");
        let inspection = parsed.inspect().expect("shared inspection");
        assert_eq!(inspection.markdown.headings.len(), 1);
        assert_eq!(
            inspection.markdown.headings[0].text,
            format!("Document {index}")
        );
        assert_eq!(
            inspection.markdown.headings[0].id,
            format!("document-{index}")
        );
        assert_eq!(inspection.markdown.links.len(), PARAGRAPHS);
        let html = parsed.render_html().expect("shared fragment renderer");
        assert_eq!(
            html,
            format!(
                "<h1 id=\"document-{index}\">Document {index}</h1>\n{}",
                RENDERED_PARAGRAPH.repeat(PARAGRAPHS)
            )
        );
        html_bytes += html.len();
        black_box((inspection, html));
    }
    html_bytes
}

fn measured<T>(operation: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let result = operation();
    (result, started.elapsed().as_micros())
}
