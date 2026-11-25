//! Real Rustdoc qualification requires two explicitly selected installed executables.
//! Set `GAZETTE_TEST_RUSTDOC_STABLE` and `GAZETTE_TEST_RUSTDOC_NIGHTLY` to absolute paths.
//! Missing tools fail this explicit qualification target; no silent skip or PATH fallback.

mod support;

use gazette_adapter_documentation::NativeGenerator;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_process::Runner;
use gazette_core::{
    documentation::{
        Artifact, Format, ProjectItem, ProjectModule, Reference, Request, Selection, Target,
        generator::{Argument, Recipe},
    },
    source::{CapturedSource, Content, Limits, Sha256Digest},
};
use gazette_ports::documentation::{DocumentationTarget as _, Generator as _};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
use support::{Job, pin};

fn job(variable: &str, json: bool) -> Job {
    let program = PathBuf::from(
        std::env::var_os(variable).expect("supply explicit Rustdoc executable for qualification"),
    );
    assert!(program.is_absolute(), "qualification never searches PATH");
    let mut arguments = vec![Argument::Input {
        path: "library.rs".parse().expect("source entry"),
    }];
    for value in [
        "--crate-name",
        "reference_fixture",
        "--crate-version",
        "0.1.0",
        "--edition",
        "2024",
        "-D",
        "warnings",
    ] {
        arguments.push(Argument::Literal {
            value: value.into(),
        });
    }
    if json {
        for value in ["-Z", "unstable-options", "--output-format", "json"] {
            arguments.push(Argument::Literal {
                value: value.into(),
            });
        }
    }
    arguments.extend([
        Argument::Literal { value: "-o".into() },
        Argument::OutputRoot,
    ]);
    let version_arguments = vec!["--version".into()];
    let version = std::process::Command::new(&program)
        .args(&version_arguments)
        .env_clear()
        .output()
        .expect("observe selected Rustdoc version");
    assert!(version.status.success(), "selected Rustdoc version failed");
    let recipe = Recipe {
        executable: pin(&program),
        program: program.to_str().expect("explicit UTF-8 tool path").into(),
        arguments,
        environment: BTreeMap::new(),
        version_arguments,
        expected_version_sha256: Sha256Digest::of(&version.stdout),
        validation_arguments: None,
        output: if json {
            Selection::File {
                path: "reference_fixture.json"
                    .parse()
                    .expect("native output file"),
            }
        } else {
            Selection::Directory {
                path: Some("reference_fixture".parse().expect("native crate directory")),
            }
        },
        format: if json {
            Format::RustdocJson
        } else {
            Format::RustdocHtml
        },
    };
    Job::new(
        &[("library.rs", include_bytes!("fixtures/rustdoc/library.rs"))],
        recipe,
    )
}

fn assemble(artifact: Artifact<'_>) -> Result<CapturedSource, gazette_core::documentation::Error> {
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    let projects = [ProjectModule {
        slug: "rust",
        title: "Rust reference",
        manual: None,
        navigation: vec![ProjectItem::Reference(Reference {
            pages: None,
            slug: "library",
            title: "Library",
            artifact,
        })],
    }];
    gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak).assemble(
        Request {
            name: "Native reference",
            public_base_url: None,
            target: &target,
            projects: &projects,
            usage: None,
            metadata: Default::default(),
        },
        Limits::default(),
    )
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_RUSTDOC_STABLE and GAZETTE_TEST_RUSTDOC_NIGHTLY provider qualification"]
fn actual_stable_html_reaches_mintlify_while_nightly_json_retains_explicit_target_refusal() {
    let native = NativeGenerator::new(&FileSystem, &Runner);
    let stable = job("GAZETTE_TEST_RUSTDOC_STABLE", false);
    let html = native
        .generate(stable.request())
        .expect("real stable Rustdoc HTML");
    assert_eq!(html.selection, stable.recipe.output);
    assert_eq!(
        html.selection
            .content(&html.output.as_input())
            .expect("native selected directory"),
        Content::Directory
    );
    assert!(
        html.output
            .file(
                &"reference_fixture/index.html"
                    .parse()
                    .expect("native index")
            )
            .is_some()
    );
    assert!(
        html.output
            .inventory()
            .entries()
            .iter()
            .any(|entry| matches!(entry.content, Content::File { .. })
                && entry.path.as_str().starts_with("static.files/")),
        "whole capture retains Rustdoc runtime companions outside the selected crate directory"
    );
    assert!(
        html.evidence
            .version_text
            .as_deref()
            .expect("actual version")
            .starts_with("rustdoc ")
    );
    let record = std::str::from_utf8(
        html.output
            .file(
                &"reference_fixture/model/struct.Record.html"
                    .parse()
                    .expect("native path"),
            )
            .expect("native struct page"),
    )
    .expect("native HTML UTF-8");
    assert!(record.contains("structfield.value"));
    assert!(record.contains("Original payload, without conversion."));
    assert!(record.contains("assert_eq!"));
    let describe = std::str::from_utf8(
        html.output
            .file(
                &"reference_fixture/fn.describe.html"
                    .parse()
                    .expect("native path"),
            )
            .expect("native function page"),
    )
    .expect("native HTML UTF-8");
    assert!(describe.contains("model/struct.Record.html"));
    assert!(describe.contains("0.1.0"));
    let target = assemble(Artifact {
        format: html.format,
        tree: html.output.as_input(),
        selection: &html.selection,
    })
    .expect("actual configured native generator output reaches supported Mintlify HTML projection");
    let text = |path: &str| {
        std::str::from_utf8(
            target
                .file(&path.parse().unwrap())
                .expect("complete target page"),
        )
        .expect("target UTF-8")
    };
    let maximum = text("rust/library/constant.MAX_RECORDS.md");
    let implementors = html
        .output
        .file(
            &"trait.impl/reference_fixture/trait.Describe.js"
                .parse()
                .unwrap(),
        )
        .expect(
            "real implementation retains its native trait companion outside selected API directory",
        );
    assert!(
        std::str::from_utf8(implementors)
            .unwrap()
            .contains("Object.fromEntries(")
    );
    let retained_implementors = implementors.to_vec();
    let trait_page = text("rust/library/trait.Describe.md");
    let record_page = text("rust/library/model/struct.Record.md");
    assert!(trait_page.contains("Describe") && trait_page.contains("Record"));
    assert!(trait_page.contains("href=\"/rust/library/model/struct.Record\""));
    for expected in [
        "id=\"method.description\"",
        "id=\"associatedtype.Output\"",
        "Returns the retained payload description through",
        "href=\"/rust/library/fn.describe\"",
    ] {
        assert!(
            record_page.contains(expected),
            "missing {expected}: {record_page}"
        );
    }
    let label = text("rust/library/static.DEFAULT_LABEL.md");
    assert!(maximum.contains("pub const MAX&#95;RECORDS: "), "{maximum}");
    assert!(
        maximum.contains("usize") && maximum.contains(" = 8;"),
        "{maximum}"
    );
    assert!(maximum.contains("Default upper bound; see"));
    // This inspects encoded Markdown/HTML bytes; the native target test also checks decoded links.
    assert!(
        maximum.contains("href=\"/rust/library/static.DEFAULT&#95;LABEL\""),
        "{maximum}"
    );
    assert!(
        label.contains("pub static DEFAULT&#95;LABEL: &amp;"),
        "{label}"
    );
    assert!(label.contains("str"), "{label}");
    assert!(label.contains("retained records."));
    assert!(
        label.contains("href=\"/rust/library/constant.MAX&#95;RECORDS\""),
        "{label}"
    );
    let macro_page = text("rust/library/macro.record.md");
    assert!(
        macro_page.contains("macro&#95;rules&#33; record"),
        "{macro_page}"
    );
    assert!(macro_page.contains("&#36;value:expr"), "{macro_page}");
    assert!(macro_page.contains("without converting its value."));
    assert!(
        macro_page.contains("href=\"/rust/library/model/struct.Record\""),
        "{macro_page}"
    );
    assert!(macro_page.contains("7&#95;u32"), "{macro_page}");
    let union = text("rust/library/union.Word.md");
    assert!(union.contains("pub union Word"), "{union}");
    for expected in [
        "pub integer:",
        "u32",
        "pub floating:",
        "f32",
        "Alternative representations of the same word, not two independent values.",
        "The caller must know which field is active before reading it.",
        "The word interpreted as an unsigned integer.",
        "The word interpreted as a floating-point value.",
        "id=\"structfield.integer\"",
        "id=\"structfield.floating\"",
    ] {
        assert!(union.contains(expected), "missing {expected}: {union}");
    }
    for page in [maximum, label, macro_page, union] {
        assert!(
            page.contains("/rust/library/sources/src/reference&#95;fixture/library.rs&#35;"),
            "{page}"
        );
    }
    let source = text("rust/library/sources/src/reference_fixture/library.rs.md");
    // Native syntax-highlighting spans separate source tokens. Full decoded source
    // equality is checked at the target boundary; here inspect the retained tokens.
    for token in ["MAX&#95;RECORDS", "DEFAULT&#95;LABEL", "\"reference\""] {
        assert!(
            source.contains(token),
            "missing source token {token}: {source}"
        );
    }
    let config: Value = serde_json::from_str(text("docs.json")).expect("native target navigation");
    let navigation = config["navigation"]["groups"][0]["pages"][0]["pages"]
        .as_array()
        .unwrap();
    assert!(navigation.contains(&Value::String("rust/library/constant.MAX_RECORDS".into())));
    assert!(navigation.contains(&Value::String("rust/library/static.DEFAULT_LABEL".into())));
    assert!(navigation.contains(&Value::String("rust/library/macro.record".into())));
    assert!(navigation.contains(&Value::String("rust/library/union.Word".into())));
    let identity = html.output.inventory().identity();
    std::fs::rename(
        stable.output.path().join("reference_fixture"),
        stable.output.path().join("moved_native_output"),
    )
    .expect("remove only disposable generated native pages from their original paths");
    let repeated = assemble(Artifact {
        format: html.format,
        tree: html.output.as_input(),
        selection: &html.selection,
    })
    .expect("retained assembly does not regenerate or reread native output");
    assert_eq!(target.inventory(), repeated.inventory());
    for entry in target.inventory().entries() {
        if matches!(entry.content, Content::File { .. }) {
            assert_eq!(target.file(&entry.path), repeated.file(&entry.path));
        }
    }
    assert_eq!(html.output.inventory().identity(), identity);
    assert_eq!(
        html.output
            .file(
                &"trait.impl/reference_fixture/trait.Describe.js"
                    .parse()
                    .unwrap()
            )
            .unwrap(),
        retained_implementors,
    );
    assert_eq!(
        std::fs::read(stable.input.path().join("library.rs")).unwrap(),
        include_bytes!("fixtures/rustdoc/library.rs")
    );

    let nightly = job("GAZETTE_TEST_RUSTDOC_NIGHTLY", true);
    let generated = native
        .generate(nightly.request())
        .expect("real nightly Rustdoc JSON");
    let bytes = generated
        .output
        .file(generated.selection.path().expect("selected JSON file"))
        .expect("complete native JSON");
    let model: Value = serde_json::from_slice(bytes).expect("actual Rustdoc JSON");
    assert_eq!(
        model["format_version"], 57,
        "requalify a changed native schema explicitly"
    );
    assert_eq!(model["crate_version"], "0.1.0");
    let items = model["index"].as_object().expect("native item index");
    let item = |name: &str, kind: &str| {
        let matches: Vec<_> = items
            .values()
            .filter(|value| value["name"] == name && value["inner"].get(kind).is_some())
            .collect();
        assert_eq!(matches.len(), 1, "exact native {kind} {name}");
        matches[0]
    };
    let record = item("Record", "struct");
    assert_eq!(
        record["inner"]["struct"]["generics"]["params"][0]["name"],
        "T"
    );
    assert!(
        record["docs"]
            .as_str()
            .expect("comments")
            .contains("assert_eq!(record.value, 3)")
    );
    let fields = record["inner"]["struct"]["kind"]["plain"]["fields"]
        .as_array()
        .expect("record field membership");
    assert_eq!(fields.len(), 1, "complete Record field membership");
    let field = items
        .get(&fields[0].to_string())
        .expect("indexed Record field");
    assert_eq!(field["name"], "value");
    assert_eq!(field["inner"]["struct_field"]["generic"], "T");
    assert_eq!(fields[0], field["id"]);
    let describe = item("describe", "function");
    assert_eq!(describe["links"]["`model::Record`"], record["id"]);
    assert!(
        describe["inner"]["function"]["sig"]["inputs"][0][1]
            .get("borrowed_ref")
            .is_some()
    );
    assert!(
        describe["inner"]["function"]["sig"]["output"]
            .get("resolved_path")
            .is_some()
    );
    assert_eq!(
        item("Selection", "enum")["inner"]["enum"]["variants"]
            .as_array()
            .expect("variants")
            .len(),
        2
    );
    let reexport = items
        .values()
        .find(|value| value["inner"]["use"]["name"] == "PublicRecord")
        .expect("public re-export");
    assert_eq!(reexport["inner"]["use"]["id"], record["id"]);
    let members = item("Describe", "trait")["inner"]["trait"]["items"]
        .as_array()
        .expect("trait members");
    let members: Vec<_> = members
        .iter()
        .map(|id| {
            let member = items.get(&id.to_string()).expect("indexed trait member");
            assert_eq!(&member["id"], id, "native trait member identity");
            member
        })
        .collect();
    assert_eq!(members.len(), 2, "complete Describe declaration membership");
    for (name, kind) in [("Output", "assoc_type"), ("description", "function")] {
        assert_eq!(
            members
                .iter()
                .filter(|value| value["name"] == name && value["inner"].get(kind).is_some())
                .count(),
            1,
            "exact native Describe member {kind} {name}"
        );
    }
    let id = record["id"].to_string();
    assert_eq!(
        model["paths"][&id]["path"],
        serde_json::json!(["reference_fixture", "model", "Record"])
    );
    assert!(
        record["span"]["filename"]
            .as_str()
            .expect("native source location")
            .ends_with("library.rs")
    );
    let error = assemble(Artifact {
        format: generated.format,
        tree: generated.output.as_input(),
        selection: &generated.selection,
    })
    .expect_err("native Rustdoc JSON still has no admitted target conversion");
    assert!(
        error
            .to_string()
            .contains("unsupported native Rustdoc format")
    );
}
