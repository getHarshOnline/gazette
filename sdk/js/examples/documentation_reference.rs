//! Independent direct Rust SDK producer for the local Node documentation fixture.

use std::io::{Read, Write};

use gazette_sdk::{build, source, workspace};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Request {
    source: String,
    site: workspace::Key,
}

fn assemble(request: &Request) -> Value {
    let failure = |code| json!({"error": code});
    let inputs = match build::acquire_documentation(
        &request.source,
        &request.site,
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    ) {
        Ok(inputs) => inputs,
        Err(error) => return failure(error.code()),
    };
    let assembly = match build::assemble_documentation(&inputs, Default::default()) {
        Ok(assembly) => assembly,
        Err(error) => return failure(error.code()),
    };
    let report = build::documentation_output::Report {
        preview_link_bases: assembly.preview_link_bases(),
        status: build::documentation_output::Status::AssembledLocal,
        inputs: inputs.inspect(),
        repository: assembly.repository(),
        manual: assembly.manual(),
        unselected_manual_pages: assembly.unselected_manual_pages(),
        unselected_manual_pages_incomplete: assembly.unselected_manual_pages_incomplete(),
        artifacts: assembly.artifacts(),
        inventory: assembly.output().inventory(),
        directory: None,
        jobs: &[],
    };
    let files = assembly.output().inventory().entries().iter().filter_map(|entry| {
        if matches!(entry.content, source::Content::Directory) { return None; }
        Some(json!({"path":entry.path,"bytes":assembly.output().file(&entry.path).expect("native complete file")}))
    }).collect::<Vec<_>>();
    json!({"report":serde_json::to_string(&report).expect("native complete report"),"files":files})
}

fn main() {
    const MAXIMUM: usize = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAXIMUM + 1) as u64)
        .read_to_end(&mut bytes)
        .expect("fixture batch");
    assert!(bytes.len() <= MAXIMUM, "bounded input batch");
    let requests: Vec<Request> = serde_json::from_slice(&bytes).expect("exact fixture inputs");
    assert!(requests.len() <= 32, "bounded request count");
    let reports = requests.iter().map(assemble).collect::<Vec<_>>();
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, &reports).expect("reference results");
    output.write_all(b"\n").expect("reference framing");
}
