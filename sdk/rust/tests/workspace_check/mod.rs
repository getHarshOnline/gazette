//! Shared capture and content diagnostics survive independent presentation failures.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use gazette_core::source::{CapturedSource, EntryPath, Error, Inventory, Limits, Subdirectory};
use gazette_ports::source::{SourceCapture, SourceFilter, SourceReader};

use super::{WorkspaceCheck, check_with};

#[derive(Default)]
struct CountingReader(RefCell<BTreeMap<PathBuf, usize>>);

impl CountingReader {
    fn record(&self, root: &Path) {
        *self.0.borrow_mut().entry(root.to_owned()).or_default() += 1;
    }
}

impl SourceReader for CountingReader {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("checking must use retained source bytes")
    }
}

impl SourceCapture for CountingReader {
    fn capture(
        &self,
        root: &Path,
        subtree: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        self.record(root);
        gazette_adapter_fs::FileSystem.capture(root, subtree, limits)
    }

    fn capture_filtered(
        &self,
        root: &Path,
        subtree: Option<&Subdirectory>,
        exclude: &[EntryPath],
        filter: &dyn SourceFilter,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        self.record(root);
        gazette_adapter_fs::FileSystem.capture_filtered(root, subtree, exclude, filter, limits)
    }
}

const PAGE: &str = "+++\nrelated=['content:missing']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nroute={path='/one/'}\nstructure={kind='page',family={kind='article'}}\n+++\nA formula: $x^2$.\n";

fn fixture(failed_themes: usize, page: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("pages/one.md"), page).unwrap();
    let mut config = String::from("[[principals]]\nid='owner:one'\nkind='person'\n");
    for (index, site) in ["one", "two"].into_iter().enumerate() {
        config.push_str(&format!("[[sites]]\nid='site:{site}'\npublisher='owner:one'\ndeployable=false\n[sites.composition]\ntheme={{package='theme:{site}'}}\n"));
        config.push_str(&format!("[[sources]]\nid='source:{site}'\nlocation={{kind='filesystem',root='theme-{site}'}}\n[[theme_packages]]\npackage='theme:{site}'\nsource='source:{site}'\nmanifest='theme.toml'\n"));
        if index >= failed_themes {
            let theme = root.path().join(format!("theme-{site}"));
            fs::create_dir_all(theme.join("templates")).unwrap();
            fs::write(theme.join("theme.toml"), format!("package='theme:{site}'\nnamespace='{site}'\ntemplates='templates'\nbrowser_theme_colors={{light='#ffffff',dark='#000000'}}\nrender={{page='page.html'}}\n")).unwrap();
            fs::write(theme.join("templates/page.html"), "{{ body_html | safe }}").unwrap();
        }
    }
    config.push_str("[[sources]]\nid='source:pages'\nlocation={kind='filesystem',root='pages'}\n[[brands]]\nid='brand:one'\nname='Example'\n");
    for site in ["one", "two"] {
        config.push_str(&format!("[[brands.publications]]\nsite='site:{site}'\npublic_base_url='https://{site}.invalid/'\n"));
        if site == "one" {
            config.push_str("authored=[{source='source:pages',target='pages',markdown_profile='publication-math'}]\n");
        }
    }

    fs::write(root.path().join("gazette.toml"), config).unwrap();
    root
}

fn check(root: &Path) -> WorkspaceCheck {
    let location = crate::configuration::locate_workspace(crate::configuration::WorkspaceLookup {
        request: Some(root),
        client: None,
        start: root,
        default: None,
    })
    .unwrap()
    .unwrap();
    let reader = CountingReader::default();
    let report = check_with(
        &reader,
        &gazette_adapter_git::Git,
        location,
        Default::default(),
        crate::build::PublicationSetAcquisitionLimits::default().sources,
        Default::default(),
    );
    let calls = reader.0.into_inner();
    let expected = ["pages", "theme-one", "theme-two"]
        .into_iter()
        .map(|name| root.join(name))
        .filter(|path| path.is_dir())
        .map(|path| (path, 1))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        calls, expected,
        "each available source is captured once; missing Theme roots fail before capture: {report:?}"
    );
    report
}

#[test]
fn two_sites_capture_their_union_once_even_when_presentations_fail() {
    for failed_themes in 0..=2 {
        let root = fixture(failed_themes, PAGE);
        let root = root.path().canonicalize().unwrap();
        let report = check(&root);
        assert_eq!(report.checked_sites.len(), 2 - failed_themes, "{report:?}");
        let content = report
            .diagnostics
            .iter()
            .filter(|row| row.message.contains("content:missing"))
            .collect::<Vec<_>>();
        assert_eq!(content.len(), 1, "{report:?}");
        assert_eq!(
            content[0].file.as_deref(),
            Some(root.join("pages/one.md").as_path())
        );
        assert_eq!(content[0].line, Some(2));
        assert!(content[0].column.is_some());
        for site in ["one", "two"].into_iter().take(failed_themes) {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|row| row.message.starts_with(&format!("site:{site}: "))),
                "{report:?}"
            );
        }
        assert_eq!(report.diagnostics.len(), failed_themes + 1, "{report:?}");
    }
}

#[test]
fn all_failed_presentations_keep_syntax_and_body_reference_locations() {
    for (page, code) in [
        (PAGE.replace("title='One'", "title=["), "toml_invalid"),
        (
            PAGE.replace("related=['content:missing']\n", "")
                + "\n[[site:one.invalid/missing/|Missing]]\n",
            "markdown_link_resolution",
        ),
    ] {
        let root = fixture(2, &page);
        let root = root.path().canonicalize().unwrap();
        let report = check(&root);
        assert!(report.checked_sites.is_empty());
        let rows = report
            .diagnostics
            .iter()
            .filter(|row| row.code == code)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1, "{report:?}");
        assert_eq!(
            rows[0].file.as_deref(),
            Some(root.join("pages/one.md").as_path())
        );
        assert!(rows[0].line.is_some());
        assert!(rows[0].column.is_some());
        assert_eq!(report.diagnostics.len(), 3, "{report:?}");
    }
}

#[test]
fn valid_two_site_content_needs_no_renderer_or_typesetter() {
    let root = fixture(0, &PAGE.replace("related=['content:missing']\n", ""));
    let report = check(&root.path().canonicalize().unwrap());
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.checked_sites.len(), 2);
}
