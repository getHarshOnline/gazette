//! Page-package ownership consumes only complete source-adapter captures.

use std::{fs, os::unix::fs::symlink, path::Path};

use gazette_sdk::{build, source};
use git2::{IndexAddOption, Oid, Repository, Signature};
use tempfile::{TempDir, tempdir};

const PAGE: &str = "+++\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n";

struct Fixture {
    directory: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempdir().expect("disposable page-package repository");
        let root = directory.path();
        fs::create_dir_all(root.join("theme/templates")).expect("Theme tree");
        fs::write(
            root.join("theme/theme.toml"),
            "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n",
        )
        .expect("Theme manifest");
        fs::write(
            root.join("theme/templates/page.html"),
            "{{ body_html | safe }}",
        )
        .expect("Theme template");
        fs::create_dir_all(root.join("content/guide")).expect("page-package root");
        fs::write(root.join("content/guide/index.md"), PAGE).expect("page document");
        fs::write(root.join("content/guide/image.png"), b"retained image").expect("page companion");
        let repository = Repository::init(root).expect("native fixture repository");
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["theme", "content"], IndexAddOption::DEFAULT, None)
            .expect("fixture sources");
        index.write().expect("fixture index write");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree identity"))
            .expect("fixture tree");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature(),
                &signature(),
                "page package fixture",
                &tree,
                &[],
            )
            .expect("fixture commit");
        drop(tree);
        drop(repository);
        Self { directory, commit }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn workspace(&self, commit: Oid) -> String {
        format!(
            "[composition]\ntheme={{package='theme:one'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[brands]]\nid='brand:one'\nname='Fixture'\n\
             [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.invalid/'\nauthored=[{{source='source:content',target='content'}}]\n\
             [[sources]]\nid='source:theme'\nlocation={{kind='repository',root={root:?},commit='{commit}',subdirectory='theme'}}\n\
             [[sources]]\nid='source:content'\nlocation={{kind='repository',root={root:?},commit='{commit}',subdirectory='content'}}\n\
             [[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
            root = self.root().to_str().expect("UTF-8 fixture root"),
        )
    }

    fn acquire(&self, commit: Oid) -> Result<build::AcquiredInputs, build::CompositionError> {
        build::acquire_publication(
            &self.workspace(commit),
            &"site:one".parse().expect("site key"),
            Default::default(),
            source::PreparationLimits {
                sources: 2,
                entries: 32,
                bytes: 1 << 20,
                per_source: source::RevisionLimits::default(),
            },
        )
    }

    fn commit_gitlink(&self) -> Oid {
        let repository = Repository::open(self.root()).expect("fixture repository");
        let parent = repository
            .find_commit(self.commit)
            .expect("fixture parent commit");
        let root = parent.tree().expect("fixture root tree");
        let content = repository
            .find_tree(root.get_name("content").expect("content tree").id())
            .expect("content tree");
        let guide = repository
            .find_tree(content.get_name("guide").expect("guide tree").id())
            .expect("guide tree");
        let mut guide_builder = repository.treebuilder(Some(&guide)).expect("guide builder");
        guide_builder
            .insert("nested", self.commit, 0o160000)
            .expect("real gitlink entry");
        let guide = guide_builder.write().expect("gitlink guide tree");
        let mut content_builder = repository
            .treebuilder(Some(&content))
            .expect("content builder");
        content_builder
            .insert("guide", guide, 0o040000)
            .expect("updated guide tree");
        let content = content_builder.write().expect("updated content tree");
        let mut root_builder = repository.treebuilder(Some(&root)).expect("root builder");
        root_builder
            .insert("content", content, 0o040000)
            .expect("updated content tree");
        let tree = repository
            .find_tree(root_builder.write().expect("gitlink root tree"))
            .expect("gitlink root tree");
        repository
            .commit(
                Some("HEAD"),
                &signature(),
                &signature(),
                "gitlink fixture",
                &tree,
                &[&parent],
            )
            .expect("gitlink-bearing commit")
    }
}

fn signature() -> Signature<'static> {
    Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
        .expect("fixed signature")
}

fn assert_unsupported(error: build::CompositionError, expected: &str) {
    assert!(matches!(
        error,
        build::CompositionError::Source { binding, error: source::Error::Unsupported(path) }
            if binding.as_str() == "source:content" && path == Path::new(expected)
    ));
}

#[test]
fn retained_page_package_acceptance_refuses_links_and_nested_gitlinks_before_resolution() {
    let fixture = Fixture::new();
    let acquired = fixture
        .acquire(fixture.commit)
        .expect("complete retained source acquisition");
    let parsed = build::parse_publication(&acquired, Default::default())
        .expect("retained page-package syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("compiler-owned page-package resolution");
    let package =
        &resolved.authored().page_packages()[&"content:guide".parse().expect("content identity")];
    assert_eq!(package.document().as_str(), "content/guide/index.md");
    assert_eq!(package.companions().len(), 1);
    assert_eq!(
        package.companions()[0].path().as_str(),
        "content/guide/image.png"
    );

    symlink("image.png", fixture.root().join("content/guide/linked.png"))
        .expect("disposable source symlink");
    assert_unsupported(
        fixture
            .acquire(fixture.commit)
            .expect_err("source adapter must reject a page-package symlink"),
        "guide/linked.png",
    );
    fs::remove_file(fixture.root().join("content/guide/linked.png"))
        .expect("remove only disposable symlink");

    let gitlink_commit = fixture.commit_gitlink();
    assert_unsupported(
        fixture
            .acquire(gitlink_commit)
            .expect_err("Git adapter must reject nested page-package gitlinks"),
        "guide/nested",
    );
}
