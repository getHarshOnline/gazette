//! Disposable native publication inputs and HTTP reads shared by preview boundary tests.

use gazette_cli::sdk::{build, workspace::Key};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};

pub(crate) const SITES: [&str; 2] = ["site:one", "site:other"];
pub(crate) const TEMPLATE: &str = "<h1>{{ content.title }}</h1>{{ body_html | safe }}<a href=\"{{ route.canonical_url }}\">canonical</a>{% for trail in breadcrumbs %}{% for item in trail.items %}{% if item.route %}<a href=\"{{ item.route.canonical_url }}\">{{ item.node.title }}</a>{% endif %}{% endfor %}{% endfor %}";

pub(crate) struct Fixture {
    pub(crate) root: TempDir,
    pub(crate) config: String,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let root = tempdir().expect("disposable source repository");
        let repo = Repository::init(root.path()).expect("native Git fixture");
        for (path, contents) in [
            ("theme/theme.toml", "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n".to_owned()),
            ("theme/templates/page.html", TEMPLATE.to_owned()),
            (
                "theme/templates/alias.html",
                "<a href=\"{{ route.canonical_url }}\">Moved</a>".to_owned(),
            ),
            ("one/page.md", "+++\nrelated=['collection:other']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One 🦀'\nlifecycle='published'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained one\n[[site:other.example/elsewhere/|Other]]\n[External Other](https://other.example/elsewhere/)\n[Unowned](https://unowned.example/reference/)\n".to_owned()),
            ("other/page.md", "+++\n[content]\nid='collection:other'\nsite='site:other'\ntitle='Other'\nlifecycle='archived'\nroute={path='/elsewhere/'}\nstructure={kind='collection'}\n+++\n# Retained other\n".to_owned()),
        ] {
            let target = root.path().join(path);
            fs::create_dir_all(target.parent().expect("parent")).expect("source directory");
            fs::write(target, contents).expect("fixture bytes");
        }
        let mut index = repo.index().expect("index");
        index
            .add_all(["theme", "one", "other"], IndexAddOption::DEFAULT, None)
            .expect("tracked source");
        index.write().expect("native index");
        let tree = repo
            .find_tree(index.write_tree().expect("tree ID"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("local source pin");
        let mut config = String::from(
            "[composition]\ntheme={package='theme:one'}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Preview fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{source='source:one',target='one'}]\n[[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.example/'\nauthored=[{source='source:other',target='other'}]\n",
        );
        for name in ["theme", "one", "other"] {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\n"));
            config.push_str(&format!("location={{kind='repository',root={:?},commit='{commit}',subdirectory='{name}'}}\n", root.path().to_str().expect("root")));
        }
        config.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n");
        Self { root, config }
    }

    pub(crate) fn recipe_value(&self) -> Value {
        // Native hosts supply the current year when omitted; the SDK has no clock.
        json!({"format":"gazette-local-publication-set/1","render_year":2040,"workspace":{"kind":"supplied","source":self.config},
            "sites":SITES.map(|site| json!({"site":site,"model_output":"model.json"}))})
    }

    pub(crate) fn recipe(&self) -> build::LocalPublicationRecipe {
        build::LocalPublicationRecipe::parse_json(
            &serde_json::to_vec(&self.recipe_value()).expect("recipe bytes"),
            1 << 20,
            2,
        )
        .expect("actual shared recipe")
    }

    pub(crate) fn request(&self, local: bool) -> Value {
        json!({"publication":{"recipe_json":self.recipe_value().to_string(),"local":true,"allow_read":true},
            "listen":SITES.map(|site|json!({"site":site,"address":"127.0.0.1:0"})),
            "allow_listeners":true,"local_urls":local,"mode":{"kind":"pinned"}})
    }

    pub(crate) fn replace(&self, path: &str, contents: &str) {
        let temporary = self.root.path().join("next-input");
        fs::write(&temporary, contents).expect("complete replacement bytes");
        fs::rename(temporary, self.root.path().join(path)).expect("atomic fixture edit");
    }
}

pub(crate) fn key(value: &str) -> Key {
    value.parse().expect("site key")
}

pub(crate) fn address(state: &Value, site: &str) -> SocketAddr {
    state["sites"][site]["address"]
        .as_str()
        .expect("actual bound address")
        .parse()
        .expect("socket")
}

pub(crate) fn get(socket: SocketAddr, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_secs(3))
        .expect("native HTTP connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bounded response");
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .expect("bounded request");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {socket}\r\nConnection: close\r\n\r\n"
    )
    .expect("actual request");
    let mut bytes = Vec::new();
    stream
        .take(4 << 20)
        .read_to_end(&mut bytes)
        .expect("complete HTTP response");
    assert!(
        bytes.starts_with(b"HTTP/1.1 200 "),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let start = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("headers")
        + 4;
    bytes[start..].to_vec()
}

pub(crate) fn released(sockets: &[SocketAddr]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let held = sockets
            .iter()
            .map(TcpListener::bind)
            .collect::<std::io::Result<Vec<_>>>();
        if held.is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "native listeners did not close");
        std::thread::sleep(Duration::from_millis(10));
    }
}
