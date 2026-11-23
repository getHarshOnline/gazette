//! Test-only direct Rust SDK reference for the local Node publication fixture.

use std::io::{Read, Write};

use gazette_sdk::{build, corpus, publication::content, source, toml};
use serde_json::{Value, json};

const INPUT_BYTES: usize = 16 * 1024 * 1024;

fn failure(code: &str) -> Value {
    json!({"report": serde_json::to_string(&json!({"code":code})).expect("native failure")})
}

fn render(bytes: &[u8]) -> Value {
    let acquisition = build::PublicationSetAcquisitionLimits::default();
    let Ok(recipe) =
        build::LocalPublicationRecipe::parse_json(bytes, INPUT_BYTES, acquisition.sites)
    else {
        return failure("publication_set_input");
    };
    match recipe.with_rendered(
        toml::Limits::default(),
        acquisition,
        source::Limits::default(),
        build::PublicationSetRenderLimits::default(),
        |rendered| {
            let report = serde_json::to_string(&rendered.inspect())
                .expect("native complete or explicitly incomplete inspection");
            if !rendered.is_complete() {
                return json!({"report":report});
            }
            let sites = rendered.sites().iter().map(|(site, result)| {
                let output = result.as_ref().expect("complete set").site().output();
                let files = output.inventory().entries().iter().filter_map(|entry| {
                    if matches!(entry.content, source::Content::Directory) {
                        return None;
                    }
                    Some(json!({"path":entry.path,"bytes":output.file(&entry.path).expect("complete captured file")}))
                }).collect::<Vec<_>>();
                json!({"site":site,"files":files})
            }).collect::<Vec<_>>();
            json!({"report":report,"sites":sites})
        },
    ) {
        Ok(report) => report,
        Err(error) => failure(error.code()),
    }
}

fn review(bytes: &[u8]) -> Value {
    let acquisition = build::PublicationSetAcquisitionLimits::default();
    let recipe = build::LocalPublicationRecipe::parse_json(bytes, INPUT_BYTES, 1)
        .expect("one-site review fixture recipe");
    let build::WorkspaceInput::Supplied { source: workspace } = recipe.workspace() else {
        return failure("publication_set_input");
    };
    let resolved_recipe = match recipe.resolve(toml::Limits::default()) {
        Ok(recipe) => recipe,
        Err(error) => return failure(error.code()),
    };
    let site = resolved_recipe
        .sites()
        .next()
        .expect("selected review site");
    let inputs = match build::acquire_publication(
        workspace,
        site,
        toml::Limits::default(),
        acquisition.sources,
    ) {
        Ok(inputs) => inputs,
        Err(error) => return failure(error.code()),
    };
    let parsed = match build::parse_publication(&inputs, corpus::Limits::default()) {
        Ok(parsed) => parsed,
        Err(error) => return failure(error.code()),
    };
    let resolved = match parsed.resolve(content::Limits::default()) {
        Ok(resolved) => resolved,
        Err(error) => return failure(error.code()),
    };
    match build::review(&resolved, build::ReviewLimits::default()) {
        Ok(review) => json!({"report":serde_json::to_string(&review).expect("native review")}),
        Err(error) => failure(error.code()),
    }
}

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    assert!(
        arguments.is_empty() || arguments == ["--review"],
        "explicit test operation"
    );
    // One bounded batch avoids launching a reference process for every assertion.
    // Strings retain the exact recipe bytes, including deliberate malformed JSON.
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .expect("read synthetic fixture batch");
    assert!(bytes.len() <= INPUT_BYTES, "bounded reference batch");
    let requests: Vec<String> =
        serde_json::from_slice(&bytes).expect("array of exact recipe strings");
    assert!(requests.len() <= 32, "bounded reference request count");
    let reports = requests
        .iter()
        .map(|request| {
            if arguments.is_empty() {
                render(request.as_bytes())
            } else {
                review(request.as_bytes())
            }
        })
        .collect::<Vec<_>>();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &reports).expect("native reference reports");
    output.write_all(b"\n").expect("reference framing");
}
