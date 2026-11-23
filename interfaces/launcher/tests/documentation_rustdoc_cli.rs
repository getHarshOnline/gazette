//! Real stable Rustdoc through configured CLI generation, Mintlify assembly and private staging.
//! Requires absolute `GAZETTE_TEST_RUSTDOC_STABLE`; never installs, searches PATH or hosts docs.

use gazette_sdk::source;
use git2::{Repository, Signature};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const LIBRARY: &[u8] =
    include_bytes!("../../../adapters/documentation/tests/fixtures/rustdoc/library.rs");

fn text(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).expect("complete staged UTF-8 output")
}

#[test]
#[ignore = "requires explicit GAZETTE_TEST_RUSTDOC_STABLE provider qualification"]
fn native_rustdoc_cli_preserves_library_reference_companions_and_brand_destination() {
    let root = tempfile::tempdir().expect("owned qualification workspace");
    let source_root = root.path().join("source");
    let jobs = root.path().join("jobs");
    let stages = root.path().join("stages");
    let destination = root.path().join("brand-selected-domain-doc");
    for directory in [&source_root, &jobs, &stages] {
        fs::create_dir(directory).expect("separate owned fixture directory");
    }
    // Native stage receipts use canonical parents, including macOS /var aliases.
    let jobs = fs::canonicalize(jobs).expect("canonical generator parent");
    let stages = fs::canonicalize(stages).expect("canonical assembly parent");
    let program = PathBuf::from(std::env::var_os("GAZETTE_TEST_RUSTDOC_STABLE")
        .expect("supply an installed stable Rustdoc via absolute GAZETTE_TEST_RUSTDOC_STABLE; no install or skip"));
    assert!(program.is_absolute(), "qualification never searches PATH");
    assert!(
        fs::symlink_metadata(&program)
            .expect("selected native Rustdoc exists")
            .is_file(),
        "select the exact regular Rustdoc executable, not a rustup proxy symlink"
    );
    let executable = fs::read(&program).expect("complete native executable pin");
    let executable_sha256 = source::Sha256Digest::of(&executable);
    let version = Command::new(&program)
        .arg("--version")
        .env_clear()
        .output()
        .expect("observe selected Rustdoc version");
    assert!(version.status.success(), "selected Rustdoc version failed");
    let expected_version_sha256 = source::Sha256Digest::of(&version.stdout);
    let repository = Repository::init(&source_root).expect("owned exact-pin repository");
    fs::create_dir(source_root.join("inputs")).expect("explicit selected subtree");
    fs::write(source_root.join("inputs/library.rs"), LIBRARY)
        .expect("existing native library fixture");
    let mut index = repository.index().expect("fixture index");
    index
        .add_path(Path::new("inputs/library.rs"))
        .expect("exact source entry");
    index.write().expect("persist fixture index");
    let tree = repository
        .find_tree(index.write_tree().expect("source identity"))
        .expect("fixture tree");
    let signature = Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
        .expect("fixed fixture identity");
    let commit = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Rustdoc fixture",
            &tree,
            &[],
        )
        .expect("exact source commit");
    drop(tree);
    let original = source::inspect(&source_root, None, Default::default())
        .expect("original source and Git bytes");

    let mut arguments = vec!["{kind='input',path='library.rs'}".to_owned()];
    for value in [
        "--crate-name",
        "reference_fixture",
        "--crate-version",
        "0.1.0",
        "--edition",
        "2024",
        "-D",
        "warnings",
        "-o",
    ] {
        arguments.push(format!("{{kind='literal',value={value:?}}}"));
    }
    arguments.push("{kind='output-root'}".into());
    let config = format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
        [[projects]]\nid='project:rust'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
        [[sources]]\nid='source:rust'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='inputs'}}\n\
        [[brands]]\nid='brand:one'\nname='Configured Rust publication'\n\
        [[brands.publications]]\nsite='site:one'\n\
        [brands.publications.documentation_repository]\nname='owner-selected-docs'\npath={:?}\n\
        [brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n\
        [[brands.publications.documentation_repository.build.modules]]\nproject='project:rust'\nslug='rust'\ntitle='Rust library'\n\
        [[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Native reference'\n\
        input={{kind='generated',source='source:rust',recipe={{program={:?},executable={{bytes={},sha256='{executable_sha256}'}},arguments=[{}],environment={{}},version_arguments=['--version'],expected_version_sha256='{expected_version_sha256}',output={{kind='directory',path='reference_fixture'}},format='rustdoc-html'}}}}\n",
        source_root.to_str().expect("source path"),
        destination.to_str().expect("configured destination"),
        program.to_str().expect("native executable path"),
        executable.len(),
        arguments.join(","),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "assemble-docs",
            "--local",
            "--site",
            "site:one",
            "--allow-execute",
            "--generator-parent",
        ])
        .arg(&jobs)
        .arg("--output-parent")
        .arg(&stages)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual Gazette executable");
    child
        .stdin
        .take()
        .expect("configuration carrier")
        .write_all(config.as_bytes())
        .expect("complete configured input");
    let result = child.wait_with_output().expect("native CLI completion");
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let report: Value =
        serde_json::from_slice(&result.stdout).expect("complete native CLI receipt");
    assert_eq!(report["status"], "documentation-assembled-local");
    assert_eq!(report["repository"]["name"], "owner-selected-docs");
    assert_eq!(report["repository"]["path"], destination.to_str().unwrap());
    assert!(
        !destination.exists(),
        "configured publication destination is not a staging directory"
    );
    assert_eq!(report["jobs"].as_array().expect("actual jobs").len(), 1);
    assert_eq!(
        report["artifacts"]
            .as_array()
            .expect("exact selected references")
            .len(),
        1
    );
    let job = &report["jobs"][0];
    let artifact = &report["artifacts"][0];
    assert_eq!(job["module"], 0);
    assert_eq!(job["reference"], 0);
    assert_eq!(
        job["selection"],
        serde_json::json!({"kind":"directory","path":"reference_fixture"})
    );
    assert_eq!(artifact["format"], "rustdoc-html");
    assert_eq!(artifact["project"], "project:rust");
    assert_eq!(artifact["source"], "source:rust");
    assert_eq!(artifact["selection"], job["selection"]);
    assert_eq!(artifact["tree"], job["output_identity"]);
    assert_eq!(artifact["generation"], job["evidence"]);
    let version = job["evidence"]["version_text"]
        .as_str()
        .expect("native version observation");
    assert!(
        version.starts_with("rustdoc ") && !version.contains("nightly"),
        "stable provider required: {version}"
    );
    assert_eq!(job["evidence"]["generation"]["exit"]["code"], 0);
    let input = PathBuf::from(job["input"].as_str().expect("retained input locator"));
    let generated = PathBuf::from(
        job["output"]
            .as_str()
            .expect("retained complete native output"),
    );
    assert!(input.starts_with(&jobs) && generated.starts_with(&jobs));
    assert_ne!(input, generated);
    assert_eq!(fs::read(input.join("library.rs")).unwrap(), LIBRARY);
    assert_eq!(
        fs::read(source_root.join("inputs/library.rs")).unwrap(),
        LIBRARY
    );
    let generated_inventory = source::inspect(&generated, None, Default::default())
        .expect("independent complete native read-back");
    assert_eq!(
        serde_json::to_value(generated_inventory.identity()).unwrap(),
        job["output_identity"]
    );
    assert!(
        generated_inventory
            .entries()
            .iter()
            .any(|entry| entry.path.as_str().starts_with("static.files/")),
        "capture preserves runtime companions outside the selected crate directory"
    );
    let trait_companion = text(&generated, "trait.impl/reference_fixture/trait.Describe.js");
    assert!(
        trait_companion.contains("Object.fromEntries("),
        "native trait companion retained"
    );
    assert!(
        text(&generated, "reference_fixture/model/struct.Record.html")
            .contains("structfield.value")
    );

    let staged = PathBuf::from(
        report["directory"]
            .as_str()
            .expect("actual private target stage"),
    );
    assert!(staged.starts_with(&stages));
    let inventory =
        source::inspect(&staged, None, Default::default()).expect("independent target read-back");
    assert_eq!(
        serde_json::to_value(&inventory).unwrap(),
        report["inventory"]
    );
    let captured = source::capture_pinned(&staged, inventory.identity(), Default::default())
        .expect("independently bound complete assembled target");
    let absent = std::collections::BTreeMap::from([(
        "rule:private".parse().unwrap(),
        "GAZETTE_PRIVATE_REFERENCE_NEEDLE".into(),
    )]);
    let ignores = text(&staged, ".mintignore");
    let mut expected_ignores = vec!["artifacts/**".to_owned()];
    let mut native_companions: Vec<_> = captured
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| entry.path.as_str().strip_suffix(".mdx"))
        .map(|route| {
            let native = format!("{route}.md");
            assert!(captured.file(&native.parse().unwrap()).is_some());
            format!("/{native}")
        })
        .collect();
    native_companions.sort();
    assert_eq!(native_companions.len(), 15);
    expected_ignores.extend(native_companions);
    assert_eq!(ignores.lines().collect::<Vec<_>>(), expected_ignores);
    let media = std::collections::BTreeMap::from([(
        ".mintignore".parse().unwrap(),
        gazette_sdk::render::MediaType::Text,
    )]);
    let qualified =
        gazette_sdk::build::check_disclosure(&captured, &media, &absent, Default::default())
            .expect("real generated Rustdoc Markdown has complete supported structural coverage");
    assert_eq!(
        qualified
            .files()
            .iter()
            .filter(|file| matches!(
                file.decoding,
                gazette_sdk::build::DisclosureDecoding::MarkdownSyntax { .. }
            ))
            .count(),
        15
    );
    assert_eq!(
        qualified
            .files()
            .iter()
            .filter(|file| matches!(
                file.decoding,
                gazette_sdk::build::DisclosureDecoding::GeneratedMdxLiteralSyntax { .. }
            ))
            .count(),
        15
    );
    let present =
        std::collections::BTreeMap::from([("rule:example".parse().unwrap(), "assert_eq".into())]);
    assert!(
        matches!(
            gazette_sdk::build::check_disclosure(&captured, &media, &present, Default::default(),),
            Err(gazette_sdk::build::DisclosureProbeError::Findings { .. })
        ),
        "entity-escaped code examples must be inspected as native HTML inside Markdown"
    );
    assert!(
        !staged.join("static.files").exists(),
        "native runtime companions are not blindly published"
    );
    assert!(
        !staged.join("trait.impl").exists(),
        "target adaptation does not export executable native companions"
    );
    let record = text(&staged, "rust/library/model/struct.Record.md");
    for expected in [
        "Original payload, without conversion.",
        "structfield.value",
        "assert&#95;eq",
        "associatedtype.Output",
        "method.description",
        "href=\"/rust/library/fn.describe\"",
    ] {
        assert!(record.contains(expected), "missing {expected}: {record}");
    }
    let function = text(&staged, "rust/library/fn.describe.md");
    assert!(
        function.contains("&amp;") && function.contains("Display"),
        "borrowed generic signature: {function}"
    );
    assert!(
        function.contains("href=\"/rust/library/model/struct.Record\""),
        "selected cross-page type link: {function}"
    );
    assert!(function.contains("0.1.0"), "version attribution");
    let trait_page = text(&staged, "rust/library/trait.Describe.md");
    assert!(
        trait_page.contains("href=\"/rust/library/model/struct.Record\""),
        "retained trait companion contributes its implementation link"
    );
    assert!(
        record.contains("/rust/library/sources/src/reference&#95;fixture/library.rs&#35;"),
        "source link retained"
    );
    let native_source = text(
        &staged,
        "rust/library/sources/src/reference_fixture/library.rs.md",
    );
    for token in ["MAX&#95;RECORDS", "DEFAULT&#95;LABEL", "\"reference\""] {
        assert!(
            native_source.contains(token),
            "retained source token {token}"
        );
    }
    let navigation: Value =
        serde_json::from_str(&text(&staged, "docs.json")).expect("assembled target navigation");
    let pages = navigation["navigation"]["groups"][0]["pages"][0]["pages"]
        .as_array()
        .expect("configured module/reference pages");
    for page in [
        "rust/library/index",
        "rust/library/fn.describe",
        "rust/library/trait.Describe",
    ] {
        assert!(
            pages.contains(&Value::String(page.into())),
            "navigation missing {page}"
        );
        assert!(
            staged.join(format!("{page}.md")).is_file(),
            "navigation resolves to staged target"
        );
    }
    let model = pages
        .iter()
        .find(|page| page["group"] == "model")
        .expect("native module remains a distinct navigation group");
    assert!(
        model["pages"]
            .as_array()
            .expect("model module pages")
            .contains(&Value::String("rust/library/model/struct.Record".into()))
    );
    assert!(staged.join("rust/library/model/struct.Record.md").is_file());
    assert_eq!(
        fs::read_dir(&jobs).unwrap().count(),
        2,
        "one retained input/output pair, no silent retry"
    );
    assert_eq!(
        fs::read_dir(&stages).unwrap().count(),
        1,
        "one exact target stage"
    );
    assert_eq!(
        source::inspect(&source_root, None, Default::default()).unwrap(),
        original,
        "source and Git state unchanged"
    );
    assert_eq!(
        source::Sha256Digest::of(&fs::read(&program).unwrap()),
        executable_sha256,
        "selected native executable unchanged"
    );
    assert!(!destination.exists());
}
