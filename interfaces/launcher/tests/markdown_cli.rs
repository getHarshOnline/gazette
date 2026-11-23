//! Direct executable Markdown transport and Rust SDK parity.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use gazette_sdk::markdown::{self, Limits};
use gazette_sdk::{
    document,
    source::{Precision, Sha256Digest},
};
use serde_json::Value;
use tempfile::tempdir;

fn run(input: &[u8], args: &[&str]) -> Output {
    let cwd = tempdir().expect("independent working directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-markdown")
        .args(args)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("input");
    let output = child.wait_with_output().expect("Gazette exit");
    assert!(
        std::fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "no hidden workspace files"
    );
    output
}

#[test]
fn executable_uses_the_same_parser_and_projections_as_the_sdk() {
    let source = "# Title\n\n[link](https://example.invalid/) and `code`\n";
    let output = run(source.as_bytes(), &["--html"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("report");
    let parsed = markdown::parse(source, Limits::default()).expect("SDK parse");
    assert_eq!(
        report["inspection"],
        serde_json::to_value(parsed.inspect().expect("SDK inspection")).expect("JSON")
    );
    assert_eq!(report["html"], parsed.render_html().expect("SDK render"));
    assert_eq!(run(source.as_bytes(), &["--html"]).stdout, output.stdout);
    let no_html: Value =
        serde_json::from_slice(&run(source.as_bytes(), &[]).stdout).expect("inspection only");
    assert!(no_html.get("html").is_none());
}

#[test]
fn invalid_input_and_late_projection_failures_never_emit_a_partial_report() {
    for (source, args, code) in [
        (b"\xff".as_slice(), vec![], "markdown_input"),
        (b"abcdef", vec!["--max-source-bytes", "2"], "markdown_limit"),
        (b"# heading", vec!["--max-nodes", "1"], "markdown_limit"),
        (b"**deep**", vec!["--max-depth", "1"], "markdown_limit"),
        (
            b"visible <script>bad</script>",
            vec!["--html"],
            "markdown_raw_html_unavailable",
        ),
        (
            b"[x](javascript:bad)",
            vec!["--html"],
            "markdown_unsafe_url",
        ),
        (
            b"text",
            vec!["--html", "--max-output-bytes", "5"],
            "markdown_limit",
        ),
    ] {
        let output = run(source, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "no partial output: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
    }
}

#[test]
fn help_discloses_effects_and_unsupported_flags_are_usage_errors() {
    let output = run(b"", &["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help");
    assert!(help.contains("stdin"));
    assert!(help.contains("no writes, network, admission or release"));
    assert!(help.contains("--tree-identity"));
    assert!(help.contains("--max-output-bytes"));
    assert_eq!(run(b"", &["--publish"]).status.code(), Some(2));
}

#[test]
fn explicit_toml_document_preserves_sdk_parity_and_source_coordinate_boundaries() {
    let source = "+++\r\ntitle = 'metadata, not the heading'\r\nfuture = { mode = 'opaque' }\r\n+++\r\n\r\n# Body é\r\n\r\n[ref](/docs)\r\n";
    let args = ["--front-matter", "toml", "--html"];
    let output = run(source.as_bytes(), &args);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete report");
    let parsed = document::parse(source, document::Limits::default()).expect("SDK document");
    let inspection = parsed.inspect().expect("SDK inspection");
    assert_eq!(
        report["document"],
        serde_json::to_value(&inspection).expect("SDK JSON")
    );
    assert_eq!(report["html"], parsed.render_html().expect("SDK HTML"));
    assert_eq!(
        inspection.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(inspection.metadata.keys, ["title", "future"]);
    assert_eq!(inspection.markdown.headings[0].text, "Body é");
    assert_eq!(
        inspection.markdown.headings[0].precision,
        Precision::Construct
    );
    let heading = inspection.markdown.headings[0].span;
    assert_eq!(
        heading
            .in_parent(source, inspection.body_span)
            .expect("checked body-to-document coordinates")
            .slice(source),
        Some("# Body é")
    );
    assert!(!report["html"].as_str().expect("HTML").contains("metadata"));
    assert_eq!(run(source.as_bytes(), &args).stdout, output.stdout);
    let inspect_only = run(source.as_bytes(), &["--front-matter", "toml"]);
    assert!(inspect_only.status.success());
    let inspect_only: Value = serde_json::from_slice(&inspect_only.stdout).expect("no HTML");
    assert_eq!(inspect_only["document"], report["document"]);
    assert!(inspect_only.get("html").is_none());

    // No autodetection: the body-only operation continues to treat all bytes as Markdown.
    let body_only = run(source.as_bytes(), &[]);
    assert!(body_only.status.success());
    let body_only: Value = serde_json::from_slice(&body_only.stdout).expect("body report");
    assert!(body_only.get("document").is_none());
    assert_eq!(
        body_only["inspection"],
        serde_json::to_value(
            markdown::parse(source, Limits::default())
                .expect("whole input as body")
                .inspect()
                .expect("body inspection")
        )
        .expect("JSON")
    );
}

#[test]
fn document_transport_rejects_bad_framing_metadata_and_late_html_without_partial_json() {
    for (input, extra, code) in [
        ("# no fence", vec![], "document_framing"),
        ("---\ntitle: YAML\n---\n", vec![], "document_framing"),
        (
            "+++\ntitle='one'\ntitle='two'\n+++\n# body\n",
            vec![],
            "toml_invalid",
        ),
        (
            "+++\ntitle='one'\n+++\n",
            vec!["--max-source-bytes", "2"],
            "document_limit",
        ),
        (
            "+++\ntitle='one'\n+++\n",
            vec!["--max-metadata-bytes", "1"],
            "toml_limit",
        ),
        (
            "+++\ntitle='one'\n+++\n",
            vec!["--max-metadata-nodes", "1"],
            "toml_limit",
        ),
        (
            "+++\na.b='one'\n+++\n",
            vec!["--max-metadata-depth", "1"],
            "toml_limit",
        ),
        (
            "+++\n+++\n# body\n",
            vec!["--max-nodes", "1"],
            "markdown_limit",
        ),
        (
            "+++\n+++\n**body**\n",
            vec!["--max-depth", "1"],
            "markdown_limit",
        ),
        (
            "+++\n+++\ntext",
            vec!["--html", "--max-output-bytes", "5"],
            "markdown_limit",
        ),
        (
            "+++\nraw_html=true\n+++\n<script>bad()</script>\n",
            vec!["--html"],
            "markdown_raw_html_unavailable",
        ),
        (
            "+++\n+++\n[x](javascript:bad)",
            vec!["--html"],
            "markdown_unsafe_url",
        ),
    ] {
        let mut args = vec!["--front-matter", "toml"];
        args.extend(extra);
        let output = run(input.as_bytes(), &args);
        assert_eq!(output.status.code(), Some(1), "{input:?}: {output:?}");
        assert!(output.stdout.is_empty(), "no partial document: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
    }
}

#[test]
fn metadata_options_require_explicit_format_and_help_names_the_boundary() {
    for args in [
        vec!["--front-matter", "yaml"],
        vec!["--max-metadata-bytes", "1"],
        vec!["--max-metadata-nodes", "1"],
        vec!["--max-metadata-depth", "1"],
    ] {
        assert_eq!(run(b"", &args).status.code(), Some(2), "{args:?}");
    }
    let output = run(b"", &["--help"]);
    let help = String::from_utf8(output.stdout).expect("help");
    assert!(help.contains("--front-matter"));
    assert!(help.contains("--max-metadata-bytes"));
    assert!(help.contains("Markdown body only"));
}
