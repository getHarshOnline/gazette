//! Connected scheduler behavior through framed LSP sessions.

use serde_json::{Value, json};

use super::{
    Limits, URI, WORKSPACE, change, close, notification, open, publications, request, session,
};

fn generations(messages: &[Value]) -> Vec<u64> {
    messages
        .iter()
        .filter(|message| message["method"] == "gazette/analysisPublished")
        .map(|message| {
            message["params"]["generation"]
                .as_u64()
                .expect("numeric SDK generation")
        })
        .collect()
}

#[test]
fn burst_edits_coalesce_to_the_latest_complete_selected_set() {
    let mut messages = vec![open(URI, 1, WORKSPACE)];
    for version in 2..=33 {
        let text = if version == 33 {
            WORKSPACE.to_owned()
        } else {
            WORKSPACE.replace("owner:one", &format!("owner:{version}"))
        };
        messages.push(change(URI, version, &text));
    }
    let (result, output) = session(messages, Limits::default());
    result.expect("bounded burst session");

    let publications = publications(&output);
    assert_eq!(
        publications.len(),
        2,
        "one open publication plus one coalesced burst publication"
    );
    assert_eq!(publications[1]["version"], 33);
    assert_eq!(publications[1]["diagnostics"], json!([]));
    assert_eq!(generations(&output), [1, 2, 3]);
}

#[test]
fn close_reopen_and_shutdown_retire_prior_editor_state() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let input = [
        open(URI, 1, WORKSPACE),
        change(URI, 2, &invalid),
        change(URI, 3, WORKSPACE),
        close(URI),
        open(URI, -7, &invalid),
        request(70, "shutdown", Value::Null),
        change(URI, -6, WORKSPACE),
        notification("exit", Value::Null),
    ];
    let mut framed = vec![super::initialize(), notification("initialized", json!({}))];
    framed.extend(input);
    let mut output = Vec::new();
    let result = gazette_lsp::serve(
        std::io::Cursor::new(super::stream(&framed)),
        &mut output,
        Limits::default(),
    );
    result.expect("retired session exits cleanly");
    let output = super::decode(&output);
    let publications = publications(&output);
    assert_eq!(
        publications.last().expect("reopen publication")["version"],
        -7
    );
    assert!(
        publications.last().expect("reopen publication")["diagnostics"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
    );
    assert!(
        publications
            .iter()
            .all(|publication| publication["version"] != -6),
        "post-shutdown changes cannot publish"
    );
}

#[test]
fn independent_folder_sessions_never_share_overlays_or_generations() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let (left_result, left) = session(
        [open(URI, 1, &invalid), change(URI, 2, WORKSPACE)],
        Limits::default(),
    );
    let (right_result, right) = session([open(URI, 41, &invalid)], Limits::default());
    left_result.expect("left folder session");
    right_result.expect("right folder session");

    assert_eq!(
        publications(&left).last().expect("left current")["version"],
        2
    );
    assert_eq!(
        publications(&left).last().expect("left current")["diagnostics"],
        json!([])
    );
    let right_publications = publications(&right);
    let right_current = right_publications.last().expect("right current");
    assert_eq!(right_current["version"], 41);
    assert!(
        right_current["diagnostics"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "right folder retains its own invalid overlay"
    );
    assert_eq!(generations(&left), [1, 2, 3]);
    assert_eq!(generations(&right), [1, 2]);
}

#[test]
fn synthetic_editor_pipeline_has_bounded_publication_amplification() {
    let mut messages = vec![open(URI, 1, WORKSPACE)];
    for version in 2..=65 {
        messages.push(change(URI, version, WORKSPACE));
    }
    let started = std::time::Instant::now();
    let (result, output) = session(messages, Limits::default());
    let elapsed = started.elapsed();
    result.expect("synthetic editor pipeline");

    let edit_publications = publications(&output);
    assert!(
        edit_publications.len() <= 3,
        "64 queued edits must not amplify into per-keystroke publications"
    );
    assert_eq!(
        edit_publications.last().expect("current result")["version"],
        65
    );
    eprintln!(
        "gazette synthetic LSP characterization: 64 changes, {} edit publications, {elapsed:?}",
        edit_publications.len()
    );
}
