//! Retained owner inputs exercise editable notes without another acquisition.

use std::{collections::BTreeMap, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{
    authoring::{
        self,
        session::{PackageSource, PageInputSource, Session, Source},
    },
    build, corpus,
    source::{self, Sha256Digest},
};
use serde_json::{Value, json};

// Explicit finite transport selection, not a changed server or analysis default.
const FRAME_BYTES: usize = 2 << 20;
const OUTPUT_BYTES: usize = 2 << 20;

fn source(value: &Value) -> Source {
    Source {
        id: value["id"].as_str().unwrap().to_owned(),
        path: value["path"].as_str().unwrap().parse().unwrap(),
        text: value["text"].as_str().unwrap().to_owned(),
    }
}

fn call(send: &mut dyn FnMut(Value) -> Value, id: usize, name: &str, arguments: Value) -> Value {
    let response = send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}}));
    assert!(
        response.get("error").is_none(),
        "{name}: transport error {:?}",
        response["error"]
    );
    assert_eq!(
        response["result"]["isError"], false,
        "{name}: {}",
        response["result"]["structuredContent"]["code"]
    );
    response["result"]["structuredContent"].clone()
}

fn status(send: &mut dyn FnMut(Value) -> Value, id: usize, handle: &str, session: &Session) {
    let actual = call(send, id, "authoring_status", json!({"session":handle}));
    assert_eq!(actual["generation"], session.generation());
    assert_eq!(
        actual["structurally_valid"],
        session.analysis().as_ref().unwrap().structurally_valid()
    );
    assert_eq!(actual["files"], json!(session.files().iter().map(|(id, document)| {
        json!({"document":id,"path":document.path(),"manual":false,
            "version":document.version(),"source_sha256":Sha256Digest::of(document.text().as_bytes())})
    }).collect::<Vec<_>>()));
    assert_eq!(
        actual["page_inputs"],
        json!(
            session
                .page_inputs()
                .iter()
                .map(|(id, input)| {
                    json!({"document":id,"path":input.path(),"source_sha256":input.source_sha256()})
                })
                .collect::<Vec<_>>()
        )
    );
}

pub(super) fn prove(
    inputs: &build::AcquiredPublicationSet,
    root: &source::EntryPath,
    configuration: &BTreeMap<source::EntryPath, String>,
) {
    // The native build parser determines which retained files are text documents.
    // Source bindings own the reverse mapping; no directory scan or copy occurs.
    let retained = inputs
        .sites()
        .values()
        .find_map(|site| site.as_ref().ok())
        .unwrap();
    let parsed = retained
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained complete publication syntax for authoring");
    let workspace = &inputs.compilation().workspace;
    let mut documents = Vec::new();
    let mut page_inputs = Vec::new();
    for (path, file) in parsed.corpus().files() {
        let binding = workspace
            .content_bindings()
            .chain(workspace.legal_sources())
            .find(|binding| binding.entry(path).is_some())
            .expect("exact retained source binding");
        let entry: source::EntryPath = binding.entry(path).unwrap().parse().unwrap();
        let bytes = inputs.sources()[binding.source_key()].file(&entry).unwrap();
        assert_eq!(file.source_sha256(), Sha256Digest::of(bytes));
        let id = format!("captured:{}/{entry}", binding.source_key());
        match file {
            corpus::File::Document(_)
            | corpus::File::Landing(_)
            | corpus::File::Markdown(_)
            | corpus::File::Notebook(_) => {
                documents
                    .push(json!({"id":id,"path":path,"text":std::str::from_utf8(bytes).unwrap()}));
            }
            corpus::File::Asset(_) => {
                page_inputs
                    .push(json!({"id":id,"path":path,"bytes_base64":STANDARD.encode(bytes)}));
            }
        }
    }
    let root_source =
        json!({"id":format!("workspace:{root}"),"path":root,"text":configuration[root]});
    let modules = configuration
        .iter()
        .filter(|(path, _)| *path != root)
        .map(|(path, text)| json!({"id":format!("workspace:{path}"),"path":path,"text":text}))
        .collect::<Vec<_>>();
    assert!(
        !modules.is_empty(),
        "original root/include organization is exercised"
    );
    let package: gazette_sdk::workspace::ThemePackageId = super::NEUTRAL.parse().unwrap();
    let package_binding = workspace
        .theme_packages()
        .find(|binding| binding.package == package)
        .expect("neutral binding");
    let package_text = std::str::from_utf8(
        inputs.sources()[&package_binding.source]
            .file(&package_binding.manifest)
            .unwrap(),
    )
    .unwrap();
    let package_id = format!(
        "captured:{}/{}",
        package_binding.source, package_binding.manifest
    );
    let package_source = json!({"kind":"theme","package":package,"id":package_id,
        "path":package_binding.manifest,"text":package_text});
    let mut sdk = Session::new_with_workspace_documents_manuals_page_inputs(
        source(&root_source),
        modules.iter().map(source),
        documents.iter().map(source),
        [],
        page_inputs.iter().map(|input| PageInputSource {
            id: input["id"].as_str().unwrap().to_owned(),
            path: input["path"].as_str().unwrap().parse().unwrap(),
            bytes: STANDARD
                .decode(input["bytes_base64"].as_str().unwrap())
                .unwrap(),
        }),
        [PackageSource {
            package: authoring::PackageBaselineKey::Theme(package),
            input: source(&package_source),
        }],
        Default::default(),
    )
    .expect("bounded retained modular authoring session");
    let analysis = sdk
        .analysis()
        .as_ref()
        .expect("complete retained authoring analysis");
    assert!(
        analysis.structurally_valid(),
        "retained baseline failure classes: {:?}",
        analysis
            .failures()
            .iter()
            .map(std::mem::discriminant)
            .collect::<Vec<_>>()
    );
    let notes = documents
        .iter()
        .filter_map(|input| {
            let id = input["id"].as_str().unwrap();
            let document = &sdk.files()[id];
            let ownership = analysis
                .page_package_ownership(
                    document.path(),
                    Sha256Digest::of(document.text().as_bytes()),
                )
                .unwrap()?;
            ownership
                .package
                .notes()
                .filter(|notes| notes.path() == document.path())
                .map(|_| id.to_owned())
        })
        .collect::<Vec<_>>();
    assert!(
        !notes.is_empty(),
        "actual selected notes remain editable documents"
    );
    assert!(notes.iter().all(|id| !sdk.page_inputs().contains_key(id)));
    let document_id = &notes[0];
    let baseline = sdk.files()[document_id].text().to_owned();
    let baseline_digest = Sha256Digest::of(baseline.as_bytes());
    let overlay = format!("{baseline}\n\n[[site:|Owner corpus label retained]]\n");
    let offset = overlay.rfind("site:").unwrap();
    let arguments = json!({"workspace":root_source,"workspace_documents":modules,
        "documents":documents,"page_inputs":page_inputs,"package_baselines":[package_source]});
    let frame = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"authoring_start","arguments":arguments}});
    let frame_bytes = serde_json::to_vec(&frame).unwrap().len();
    eprintln!(
        "retained authoring: documents={} notes={} opaque={} request_bytes={frame_bytes} frame_limit={FRAME_BYTES}",
        documents.len(),
        notes.len(),
        page_inputs.len()
    );
    assert!(
        frame_bytes <= FRAME_BYTES,
        "actual retained authoring_start frame {frame_bytes} exceeds explicitly selected {FRAME_BYTES}; no truncated corpus"
    );
    let transport = gazette_mcp::TransportLimits::new(FRAME_BYTES, OUTPUT_BYTES)
        .expect("finite explicit corpus transport budgets");
    super::mcp_carrier::initialized_interact_with_transport_limits(
        "2026-07-28",
        Duration::from_secs(60),
        transport,
        |initialized, send| {
            assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
            let created = call(send, 1, "authoring_start", arguments.clone());
            let handle = created["session"].as_str().unwrap();
            status(send, 2, handle, &sdk);
            assert!(sdk.open(document_id, 7, overlay.clone()).unwrap());
            let opened = call(
                send,
                3,
                "authoring_open_document",
                json!({"session":handle,"generation":1,
            "document":document_id,"version":7,"text":overlay}),
            );
            assert_eq!(opened["generation"], sdk.generation());
            status(send, 4, handle, &sdk);
            let analysis = sdk.analysis().as_ref().unwrap();
            let document = &sdk.files()[document_id];
            let completion = analysis
                .complete_symbolic_reference(
                    document.analysis_path(),
                    document.text(),
                    offset,
                    OUTPUT_BYTES,
                )
                .expect("complete bounded current candidate set")
                .expect("notes AST selector slot");
            assert_eq!(completion.preimage, "site:");
            assert_eq!(
                completion.location.span.slice(document.text()),
                Some("site:")
            );
            assert!(completion.current.is_none());
            assert!(
                completion.candidates.iter().any(|candidate| notes
                    .iter()
                    .any(|id| sdk.files()[id].path() == &candidate.path)),
                "retained notes headings are candidates under their article identities"
            );
            let proposal = call(
                send,
                5,
                "authoring_complete_reference",
                json!({"session":handle,"generation":2,
            "document":document_id,"offset":offset}),
            );
            assert_eq!(proposal["value"], json!(completion));
            assert_eq!(
                proposal["source_sha256"],
                json!(Sha256Digest::of(overlay.as_bytes()))
            );
            assert_eq!(proposal["version"], 7);
            let ownership = analysis
                .page_package_ownership(
                    document.path(),
                    Sha256Digest::of(document.text().as_bytes()),
                )
                .unwrap()
                .unwrap();
            let owning_site = &completion
                .candidates
                .iter()
                .find(|candidate| &candidate.content == ownership.content)
                .unwrap()
                .site;
            let target = completion
                .candidates
                .iter()
                .find(|candidate| {
                    &candidate.site != owning_site && candidate.replacement.contains('#')
                })
                .expect("real selected cross-site heading target")
                .clone();
            let target_document = sdk
                .files()
                .values()
                .find(|source| source.path() == &target.path)
                .unwrap();
            assert_eq!(
                target.source_sha256,
                Sha256Digest::of(target_document.text().as_bytes())
            );
            assert!(
                target
                    .definition
                    .span
                    .slice(target_document.text())
                    .is_some()
            );
            let mut repaired = overlay.clone();
            repaired.replace_range(
                completion.location.span.start..completion.location.span.end,
                &target.replacement,
            );
            assert!(repaired.ends_with("|Owner corpus label retained]]\n"));
            assert!(sdk.change(document_id, 8, repaired.clone()).unwrap());
            let changed = call(
                send,
                6,
                "authoring_change_document",
                json!({"session":handle,"generation":2,
            "document":document_id,"version":8,"text":repaired}),
            );
            assert_eq!(changed["generation"], sdk.generation());
            let current = sdk.analysis().as_ref().unwrap();
            assert!(
                current.structurally_valid(),
                "reviewed destination resolves through current native model"
            );
            let diagnostics = call(
                send,
                7,
                "authoring_diagnostics",
                json!({"session":handle,"generation":3,"document":document_id}),
            );
            assert_eq!(
                diagnostics["value"],
                json!(
                    authoring::diagnostics(
                        current,
                        sdk.files()[document_id].path(),
                        &repaired,
                        OUTPUT_BYTES
                    )
                    .unwrap()
                )
            );
            let stale = send(json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
            "params":{"name":"authoring_change_document","arguments":{"session":handle,"generation":2,
                "document":document_id,"version":9,"text":baseline}}}));
            assert_eq!(
                stale["result"]["structuredContent"]["code"],
                "authoring_stale_generation"
            );
            status(send, 9, handle, &sdk);
            assert!(sdk.close(document_id).unwrap());
            let closed = call(
                send,
                10,
                "authoring_close_document",
                json!({"session":handle,"generation":3,"document":document_id}),
            );
            assert_eq!(closed["generation"], sdk.generation());
            assert_eq!(sdk.files()[document_id].version(), None);
            assert_eq!(
                Sha256Digest::of(sdk.files()[document_id].text().as_bytes()),
                baseline_digest
            );
            assert!(sdk.analysis().as_ref().unwrap().structurally_valid());
            status(send, 11, handle, &sdk);
            call(
                send,
                12,
                "authoring_close_session",
                json!({"session":handle,"generation":4}),
            );
        },
    );
}
