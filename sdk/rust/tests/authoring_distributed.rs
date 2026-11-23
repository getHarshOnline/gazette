//! Corpus-scale authoring preserves mounted logical paths and physical client custody.

use std::{fs, path::Path};

use gazette_sdk::{
    authoring::{
        self,
        session::{Error, Limits, Session, Source},
    },
    source::Sha256Digest,
};
use tempfile::{TempDir, tempdir};

const ROOT: &str = "includes=['config/sites.toml']\n";
const SITES: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Selected publication'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\n";
const PAGES: usize = 220;

struct Fixture {
    workspace: TempDir,
    owners: [TempDir; 3],
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            workspace: tempdir().expect("selected workspace root"),
            owners: std::array::from_fn(|_| tempdir().expect("independent owner root")),
        };
        fs::create_dir(fixture.workspace.path().join("config")).unwrap();
        fs::write(fixture.workspace.path().join("gazette.toml"), ROOT).unwrap();
        fs::write(fixture.workspace.path().join("config/sites.toml"), SITES).unwrap();
        for number in 0..PAGES {
            let owner = number % fixture.owners.len();
            fs::write(fixture.owners[owner].path().join(format!("item-{number}.md")), format!(
                "+++\n[content]\nid='content:item-{number}'\nsite='site:one'\ntitle='Item {number}'\nlifecycle='draft'\nroute={{path='/blog/item-{number}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Item {number}\n"
            )).unwrap();
        }
        fixture
    }

    fn source(root: &Path, entry: &str, logical: &str) -> Source {
        let physical = root.join(entry);
        Source {
            id: format!("file://{}", physical.display()),
            path: logical.parse().unwrap(),
            text: fs::read_to_string(physical).expect("caller captures explicitly selected file"),
        }
    }

    fn selection(&self, limits: Limits) -> Result<Session, Error> {
        Session::new_with_workspace_documents(
            Self::source(self.workspace.path(), "gazette.toml", "gazette.toml"),
            [Self::source(
                self.workspace.path(),
                "config/sites.toml",
                "config/sites.toml",
            )],
            (0..PAGES).map(|number| {
                let owner = number % self.owners.len();
                Self::source(
                    self.owners[owner].path(),
                    &format!("item-{number}.md"),
                    &format!("owner-{owner}/content/item-{number}.md"),
                )
            }),
            [],
            limits,
        )
    }
}

fn assert_native_parity(session: &Session) {
    fn input(document: &gazette_sdk::authoring::session::Document) -> authoring::Input<'_> {
        authoring::Input {
            path: document.path().clone(),
            bytes: document.text().as_bytes(),
        }
    }
    let documents = session.files().values().collect::<Vec<_>>();
    let native = authoring::analyze_workspace_documents_with_package_baselines(
        "gazette.toml".parse().unwrap(),
        documents
            .iter()
            .filter(|document| document.path().as_str().ends_with(".toml"))
            .map(|document| input(document))
            .collect(),
        documents
            .iter()
            .filter(|document| document.path().as_str().ends_with(".md"))
            .map(|document| input(document)),
        [],
        Default::default(),
    )
    .expect("complete exact native compiler analysis");
    let current = session.analysis().as_ref().expect("current shared result");
    assert_eq!(current.inputs(), native.inputs());
    assert_eq!(current.structurally_valid(), native.structurally_valid());
}

#[test]
fn distributed_corpus_uses_one_bounded_analysis_and_preserves_overlay_custody() {
    let fixture = Fixture::new();
    let mut session = fixture
        .selection(Default::default())
        .expect("all selected owner files");
    assert_eq!(session.files().len(), PAGES + 2);
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
    assert_native_parity(&session);
    let uri = format!(
        "file://{}",
        fixture.owners[0].path().join("item-0.md").display()
    );
    let baseline = session.files()[&uri].text().to_owned();
    assert_eq!(
        session.files()[&uri].path().as_str(),
        "owner-0/content/item-0.md"
    );
    let conflict = baseline.replace("id='content:item-0'", "id='content:item-1'");
    assert!(session.open(&uri, 7, conflict.clone()).unwrap());
    assert!(!session.analysis().as_ref().unwrap().structurally_valid());
    assert_native_parity(&session);
    assert_eq!(
        session.analysis().as_ref().unwrap().inputs()["owner-0/content/item-0.md"],
        Sha256Digest::of(conflict.as_bytes())
    );
    assert!(
        !session.change(&uri, 7, baseline.clone()).unwrap(),
        "stale version cannot restore old success"
    );
    fs::write(
        fixture.owners[0].path().join("item-0.md"),
        "unselected later disk mutation",
    )
    .unwrap();
    assert!(session.close(&uri).unwrap());
    assert_eq!(
        session.files()[&uri].text(),
        baseline,
        "close restores capture, not live disk"
    );
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
    assert_native_parity(&session);
}

#[test]
fn distributed_selection_remains_atomic_at_explicit_aggregate_limits() {
    let fixture = Fixture::new();
    let baseline = fixture.selection(Default::default()).unwrap();
    let identifier_bytes = baseline.files().keys().map(String::len).sum();
    let source_bytes = baseline
        .files()
        .values()
        .map(|document| document.text().len())
        .sum();
    let exact = Limits {
        documents: PAGES + 2,
        identifier_bytes,
        source_bytes,
        ..Default::default()
    };
    assert!(
        fixture
            .selection(exact)
            .unwrap()
            .analysis()
            .as_ref()
            .unwrap()
            .structurally_valid()
    );
    for (limits, reason) in [
        (
            Limits {
                documents: PAGES + 1,
                ..exact
            },
            "selected documents",
        ),
        (
            Limits {
                identifier_bytes: identifier_bytes - 1,
                ..exact
            },
            "selected identifier bytes",
        ),
        (
            Limits {
                source_bytes: source_bytes - 1,
                ..exact
            },
            "selected source bytes",
        ),
    ] {
        assert!(
            matches!(fixture.selection(limits), Err(Error::Limit(actual)) if actual == reason),
            "no partial selection after {reason} exhaustion"
        );
    }
}
