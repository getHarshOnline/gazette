use std::{fs, process::Command};

use serde_json::{Value, json};

mod limits;

const WORKSPACE: &str = r#"
[composition]
theme={package='theme:one'}
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
public_base_url='https://example.invalid/'
authored=[{source='source:pages',target='pages',markdown_profile='publication-math'}]
[[sources]]
id='source:pages'
location={kind='filesystem',root='pages'}
[[sources]]
id='source:theme'
location={kind='filesystem',root='theme'}
[[theme_packages]]
package='theme:one'
source='source:theme'
manifest='theme.toml'
"#;

const PAGE: &str = "+++\nrelated=['content:missing']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nroute={path='/one/'}\nstructure={kind='page',family={kind='article'}}\n+++\nA formula: $x^2$.\n";

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::create_dir_all(root.path().join("theme/templates")).unwrap();
    fs::write(root.path().join("gazette.toml"), WORKSPACE).unwrap();
    fs::write(root.path().join("pages/one.md"), PAGE).unwrap();
    fs::write(root.path().join("theme/theme.toml"), "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n").unwrap();
    fs::write(
        root.path().join("theme/templates/page.html"),
        "{{ body_html | safe }}",
    )
    .unwrap();
    fs::write(root.path().join("theme/templates/alias.html"), "Moved").unwrap();
    root
}

#[test]
fn check_reports_the_same_broken_page_through_cli_sdk_and_mcp() {
    let root = fixture();
    let path = root.path().canonicalize().unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--workspace-root"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        !cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stdout)
    );
    let report: Value = serde_json::from_slice(&cli.stdout).unwrap();
    let diagnostic = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["message"]
                .as_str()
                .is_some_and(|message| message.contains("content:missing"))
        })
        .unwrap_or_else(|| panic!("missing reference was not reported: {report}"));
    assert_eq!(
        diagnostic["file"],
        path.join("pages/one.md").to_str().unwrap()
    );
    assert!(diagnostic["line"].as_u64().unwrap() > 0);
    let stderr = String::from_utf8(cli.stderr).unwrap();
    assert!(stderr.contains("one.md:"));
    assert!(stderr.contains("content:missing"));

    let location =
        gazette_sdk::configuration::locate_workspace(gazette_sdk::configuration::WorkspaceLookup {
            request: Some(&path),
            client: None,
            start: &path,
            default: None,
        })
        .unwrap()
        .unwrap();
    let sdk = gazette_sdk::build::check_workspace(
        location,
        Default::default(),
        gazette_sdk::build::PublicationSetAcquisitionLimits::default().sources,
        Default::default(),
    );
    assert_eq!(report, serde_json::to_value(sdk).unwrap());
    let responses = crate::mcp_carrier::exchange(vec![json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":"check_workspace","arguments":{"workspace":path}}
    })]);
    assert_eq!(responses[0]["result"]["isError"], true);
    assert_eq!(responses[0]["result"]["structuredContent"], report);

    fs::write(
        path.join("pages/one.md"),
        PAGE.replace("related=['content:missing']\n", "")
            + "\n[[site:example.invalid/missing/|Missing page]]\n",
    )
    .unwrap();
    let body = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .current_dir(&path)
        .output()
        .unwrap();
    assert!(!body.status.success());
    let body: Value = serde_json::from_slice(&body.stdout).unwrap();
    assert!(
        body["diagnostics"].as_array().unwrap().iter().any(|row| {
            row["code"] == "markdown_link_resolution"
                && row["message"]
                    .as_str()
                    .unwrap()
                    .contains("example.invalid/missing/")
        }),
        "{body}"
    );

    fs::write(
        path.join("pages/one.md"),
        PAGE.replace("title='One'", "title=["),
    )
    .unwrap();
    let syntax = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .current_dir(&path)
        .output()
        .unwrap();
    assert!(!syntax.status.success());
    let syntax: Value = serde_json::from_slice(&syntax.stdout).unwrap();
    assert!(
        syntax["diagnostics"].as_array().unwrap().iter().any(|row| {
            row["code"] == "toml_invalid"
                && row["file"] == path.join("pages/one.md").to_str().unwrap()
                && row["line"].as_u64().is_some_and(|line| line > 0)
        }),
        "{syntax}"
    );

    fs::write(
        path.join("pages/one.md"),
        PAGE.replace("related=['content:missing']\n", ""),
    )
    .unwrap();
    let repaired = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .current_dir(&path)
        .output()
        .unwrap();
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    let repaired: Value = serde_json::from_slice(&repaired.stdout).unwrap();
    assert_eq!(repaired["checked_sites"], json!(["site:one"]));
    assert_eq!(repaired["diagnostics"], json!([]));
}

#[test]
fn a_saved_default_is_used_only_when_the_local_folder_has_no_workspace() {
    let root = fixture();
    fs::write(
        root.path().join("pages/one.md"),
        PAGE.replace("related=['content:missing']\n", ""),
    )
    .unwrap();
    let preferences = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .env("XDG_CONFIG_HOME", preferences.path())
            .current_dir(unrelated.path());
        command
    };
    let saved = command()
        .args(["config", "set", "workspace"])
        .arg(root.path())
        .output()
        .unwrap();
    assert!(saved.status.success(), "{saved:?}");
    let default = command().arg("check").output().unwrap();
    assert!(default.status.success(), "{default:?}");
    let default: Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(default["workspace_location"]["found_by"], "user_default");
    fs::write(unrelated.path().join("gazette.toml"), "broken TOML [").unwrap();
    let local = command().arg("check").output().unwrap();
    assert!(!local.status.success());
    let local: Value = serde_json::from_slice(&local.stdout).unwrap();
    assert_eq!(local["workspace_location"]["found_by"], "working_directory");
}
