//! One configured brand repository composes every installed native-reference profile.
//! Provider paths are explicit: this fixture never installs tools, searches `PATH`, fabricates
//! generated artifacts, or substitutes a shell copier for native generation.

use gazette_sdk::source;
use git2::{IndexAddOption, Repository, Signature};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

const SPHINX: &[(&str, &[u8])] = &[
    (
        "conf.py",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/conf.py"),
    ),
    (
        "model.py",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/model.py"),
    ),
    (
        "api.py",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/api.py"),
    ),
    (
        "index.rst",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/index.rst"),
    ),
    (
        "model.rst",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/model.rst"),
    ),
    (
        "api.rst",
        include_bytes!("../../../adapters/documentation/tests/fixtures/sphinx/api.rst"),
    ),
];
const TYPEDOC: &[(&str, &[u8])] = &[
    (
        "package.json",
        include_bytes!("../../../adapters/documentation/tests/fixtures/typedoc/package.json"),
    ),
    (
        "tsconfig.json",
        include_bytes!("../../../adapters/documentation/tests/fixtures/typedoc/tsconfig.json"),
    ),
    (
        "typedoc.json",
        include_bytes!("../../../adapters/documentation/tests/fixtures/typedoc/typedoc.json"),
    ),
    (
        "src/api.ts",
        include_bytes!("../../../adapters/documentation/tests/fixtures/typedoc/src/api.ts"),
    ),
    (
        "src/model.ts",
        include_bytes!("../../../adapters/documentation/tests/fixtures/typedoc/src/model.ts"),
    ),
];
const GOMARKDOC: &[(&str, &[u8])] = &[
    (
        "go.mod",
        include_bytes!("../../../adapters/documentation/tests/fixtures/gomarkdoc/go.mod"),
    ),
    (
        "library.go",
        include_bytes!("../../../adapters/documentation/tests/fixtures/gomarkdoc/library.go"),
    ),
    (
        "example_test.go",
        include_bytes!("../../../adapters/documentation/tests/fixtures/gomarkdoc/example_test.go"),
    ),
];
const SWAGGER: &[(&str, &[u8])] = &[
    (
        "go.mod",
        include_bytes!("../../../adapters/documentation/tests/fixtures/swagger/go.mod"),
    ),
    (
        "api.go",
        include_bytes!("../../../adapters/documentation/tests/fixtures/swagger/api.go"),
    ),
];
const RUSTDOC: &[u8] =
    include_bytes!("../../../adapters/documentation/tests/fixtures/rustdoc/library.rs");
const MANUAL: &[(&str, &[u8])] = &[
    (
        "docs/start.md",
        b"# Gazette manual\n\n[Install](install.md#install)\n\n[Python reference](/python/library)\n\n[Rust reference](/rust/library/fn.describe)\n\n[TypeScript reference](/typescript/library)\n\n[Go reference](/go/library)\n\n[HTTP reference](/http/library)\n",
    ),
    (
        "docs/install.md",
        b"# Install\n\n[Start](start.md#gazette-manual)\n",
    ),
];

struct Tools {
    sphinx: PathBuf,
    rustdoc: PathBuf,
    node: PathBuf,
    typedoc: PathBuf,
    gomarkdoc: PathBuf,
    swagger: PathBuf,
    go_root: PathBuf,
}

impl Tools {
    fn required() -> Self {
        let mut errors = Vec::new();
        let (sphinx, rustdoc, node, typedoc, gomarkdoc, swagger, go_root) = {
            let mut get = |name: &str, directory: bool| {
                let Some(value) = std::env::var_os(name) else {
                    errors.push(format!("{name} is unset"));
                    return None;
                };
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    errors.push(format!("{name} must be absolute: {path:?}"));
                    return None;
                }
                if directory {
                    match fs::canonicalize(&path) {
                        Ok(path) if path.is_dir() => Some(path),
                        Ok(path) => {
                            errors.push(format!("{name} has the wrong path type: {path:?}"));
                            None
                        }
                        Err(error) => {
                            errors.push(format!("{name} cannot be resolved at {path:?}: {error}"));
                            None
                        }
                    }
                } else {
                    match fs::symlink_metadata(&path) {
                        Ok(metadata) if metadata.is_file() => Some(path),
                        Ok(_) => {
                            errors.push(format!(
                                "{name} must name the exact regular executable, not a symlink or special file: {path:?}"
                            ));
                            None
                        }
                        Err(error) => {
                            errors.push(format!("{name} cannot be resolved at {path:?}: {error}"));
                            None
                        }
                    }
                }
            };
            (
                get("GAZETTE_TEST_SPHINX_PYTHON", false),
                get("GAZETTE_TEST_RUSTDOC_STABLE", false),
                get("GAZETTE_TEST_NODE", false),
                get("GAZETTE_TEST_TYPEDOC", false),
                get("GAZETTE_TEST_GOMARKDOC", false),
                get("GAZETTE_TEST_SWAGGER", false),
                get("GAZETTE_TEST_GO_ROOT", true),
            )
        };
        if let Some(root) = &go_root
            && !root.join("bin/go").is_file()
        {
            errors.push(format!(
                "GAZETTE_TEST_GO_ROOT/bin/go is unavailable under {root:?}"
            ));
        }
        assert!(
            errors.is_empty(),
            "installed generated-documentation dependencies unavailable: {}",
            errors.join("; ")
        );
        Self {
            sphinx: sphinx.expect("validated Sphinx Python"),
            rustdoc: rustdoc.expect("validated stable Rustdoc"),
            node: node.expect("validated Node"),
            typedoc: typedoc.expect("validated TypeDoc entrypoint"),
            gomarkdoc: gomarkdoc.expect("validated gomarkdoc"),
            swagger: swagger.expect("validated go-swagger"),
            go_root: go_root.expect("validated Go root"),
        }
    }
}

struct Provider {
    program: PathBuf,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    version_arguments: Vec<String>,
    validation_arguments: Option<Vec<String>>,
    output: String,
    format: &'static str,
    executable_bytes: u64,
    executable_sha256: source::Sha256Digest,
    version_text: String,
    version_sha256: source::Sha256Digest,
}

struct ProviderRecipe {
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    version_arguments: Vec<String>,
    validation_arguments: Option<Vec<String>>,
    output: &'static str,
    format: &'static str,
}

impl Provider {
    fn new(
        program: PathBuf,
        expected_executable_variable: &str,
        expected_version_variable: &str,
        recipe: ProviderRecipe,
    ) -> Self {
        let ProviderRecipe {
            arguments,
            environment,
            version_arguments,
            validation_arguments,
            output,
            format,
        } = recipe;
        let expected_executable_sha256 = expected_digest(expected_executable_variable);
        let expected_version_sha256 = expected_digest(expected_version_variable);
        let executable = fs::read(&program).expect("explicit provider executable");
        assert_eq!(
            source::Sha256Digest::of(&executable),
            expected_executable_sha256,
            "provider executable does not match {expected_executable_variable}"
        );
        let observed = Command::new(&program)
            .args(&version_arguments)
            .env_clear()
            .envs(&environment)
            .output()
            .expect("installed provider version command");
        assert!(
            observed.status.success() && !observed.stdout.is_empty(),
            "provider version failed for {program:?}: stderr={} stdout={}",
            String::from_utf8_lossy(&observed.stderr),
            String::from_utf8_lossy(&observed.stdout)
        );
        let version_text = String::from_utf8(observed.stdout).expect("UTF-8 provider version");
        assert_eq!(
            source::Sha256Digest::of(version_text.as_bytes()),
            expected_version_sha256,
            "provider version output does not match {expected_version_variable}"
        );
        Self {
            program,
            arguments,
            environment,
            version_arguments,
            validation_arguments,
            output: output.into(),
            format,
            executable_bytes: executable.len() as u64,
            executable_sha256: expected_executable_sha256,
            version_sha256: expected_version_sha256,
            version_text,
        }
    }

    fn recipe(&self) -> String {
        let validation = self
            .validation_arguments
            .as_ref()
            .map_or_else(String::new, |args| {
                format!(",validation_arguments=[{}]", args.join(","))
            });
        format!(
            "{{program={:?},executable={{bytes={},sha256='{}'}},arguments=[{}],environment={},version_arguments=[{}],expected_version_sha256='{}'{validation},output={},format='{}'}}",
            self.program.to_str().expect("UTF-8 provider path"),
            self.executable_bytes,
            self.executable_sha256,
            self.arguments.join(","),
            environment(&self.environment),
            strings(&self.version_arguments),
            self.version_sha256,
            self.output,
            self.format,
        )
    }
}

fn expected_digest(variable: &str) -> source::Sha256Digest {
    std::env::var(variable)
        .unwrap_or_else(|_| panic!("{variable} is unset"))
        .parse()
        .unwrap_or_else(|error| panic!("{variable} must be one lowercase SHA-256 digest: {error}"))
}

fn strings(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn environment(values: &BTreeMap<String, String>) -> String {
    format!(
        "{{{}}}",
        values
            .iter()
            .map(|(key, value)| format!("{key}={value:?}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn lit(value: &str) -> String {
    format!("{{kind='literal',value={value:?}}}")
}
fn input(value: &str) -> String {
    format!("{{kind='input',path={value:?}}}")
}
fn input_root() -> String {
    "{kind='input-root'}".into()
}
fn output() -> String {
    "{kind='output'}".into()
}
fn output_root() -> String {
    "{kind='output-root'}".into()
}

fn go_environment(root: &Path, cache: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::from([
        (
            "PATH".into(),
            root.join("bin").to_str().expect("UTF-8 Go bin").into(),
        ),
        (
            "GOROOT".into(),
            root.to_str().expect("UTF-8 Go root").into(),
        ),
        ("GOTOOLCHAIN".into(), "local".into()),
        ("GOENV".into(), "off".into()),
        ("GOWORK".into(), "off".into()),
        ("GOPROXY".into(), "off".into()),
        ("GOSUMDB".into(), "off".into()),
        ("CGO_ENABLED".into(), "0".into()),
    ]);
    for (key, name) in [
        ("GOCACHE", "build"),
        ("GOMODCACHE", "modules"),
        ("GOPATH", "gopath"),
        ("TMPDIR", "tmp"),
    ] {
        let path = cache.join(name);
        fs::create_dir_all(&path).expect("isolated Go state");
        values.insert(key.into(), path.to_str().expect("UTF-8 Go state").into());
    }
    values
}

fn write_files(root: &Path, files: &[(&str, &[u8])]) {
    for &(name, bytes) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
        fs::write(path, bytes).expect("owned fixture bytes");
    }
}

struct Expected {
    version: String,
    digest: source::Sha256Digest,
    validates: bool,
}

struct Fixture {
    source: TempDir,
    jobs: TempDir,
    outputs: TempDir,
    _cache: TempDir,
    _destination_owner: TempDir,
    destination: PathBuf,
    source_before: source::Inventory,
    destination_before: source::Inventory,
    expected: Vec<Expected>,
    config: String,
}

impl Fixture {
    fn new() -> Self {
        let tools = Tools::required();
        let source = tempfile::tempdir().expect("exact-pin source repository");
        let jobs = tempfile::tempdir().expect("private generator stages");
        let outputs = tempfile::tempdir().expect("private assembly stages");
        let cache = tempfile::tempdir().expect("isolated provider state");
        let destination_owner = tempfile::tempdir().expect("destination owner");
        let destination = destination_owner.path().join("brand-owned-domain-doc");
        fs::create_dir(&destination).expect("configured destination");
        fs::write(destination.join("sentinel"), b"owner-controlled\n")
            .expect("destination sentinel");

        let root = source.path().join("inputs");
        write_files(&root.join("sphinx"), SPHINX);
        write_files(&root.join("rustdoc"), &[("library.rs", RUSTDOC)]);
        write_files(&root.join("typedoc"), TYPEDOC);
        write_files(&root.join("gomarkdoc"), GOMARKDOC);
        write_files(&root.join("swagger"), SWAGGER);
        write_files(&root.join("manual"), MANUAL);
        let repository = Repository::init(source.path()).expect("fixture repository");
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["inputs"], IndexAddOption::DEFAULT, None)
            .expect("index fixtures");
        index.write().expect("write fixture index");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree id"))
            .expect("fixture tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("fixture commit");
        drop(tree);

        let sphinx = Provider::new(
            tools.sphinx,
            "GAZETTE_TEST_SPHINX_PYTHON_SHA256",
            "GAZETTE_TEST_SPHINX_VERSION_SHA256",
            ProviderRecipe {
                arguments: ["-B", "-s", "-m", "sphinx", "-b", "json", "-W", "-n", "-E", "-N", "."]
                    .into_iter()
                    .map(lit)
                    .chain(std::iter::once(output_root()))
                    .chain(["index.rst", "model.rst", "api.rst"].into_iter().map(input))
                    .collect(),
                environment: BTreeMap::from([
                    ("PYTHONNOUSERSITE".into(), "1".into()),
                    ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
                ]),
                version_arguments: [
                    "-B",
                    "-s",
                    "-c",
                    "import sphinx; assert sphinx.__version__ == '8.2.3', sphinx.__version__; print('Sphinx ' + sphinx.__version__)",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
                validation_arguments: None,
                output: "{kind='directory'}",
                format: "sphinx-json",
            },
        );
        let rustdoc = Provider::new(
            tools.rustdoc,
            "GAZETTE_TEST_RUSTDOC_STABLE_SHA256",
            "GAZETTE_TEST_RUSTDOC_VERSION_SHA256",
            ProviderRecipe {
                arguments: std::iter::once(input("library.rs"))
                    .chain(
                        [
                            "--crate-name",
                            "reference_fixture",
                            "--crate-version",
                            "0.1.0",
                            "--edition",
                            "2024",
                            "-D",
                            "warnings",
                            "-o",
                        ]
                        .into_iter()
                        .map(lit),
                    )
                    .chain(std::iter::once(output_root()))
                    .collect(),
                environment: BTreeMap::new(),
                version_arguments: vec!["--version".into()],
                validation_arguments: None,
                output: "{kind='directory',path='reference_fixture'}",
                format: "rustdoc-html",
            },
        );
        let typedoc_path = tools.typedoc.to_str().expect("UTF-8 TypeDoc").to_owned();
        let source_link_template =
            format!("https://github.com/example/reference/blob/{commit}/{{path}}#L{{line}}");
        let typedoc = Provider::new(
            tools.node,
            "GAZETTE_TEST_NODE_SHA256",
            "GAZETTE_TEST_TYPEDOC_VERSION_SHA256",
            ProviderRecipe {
                arguments: vec![
                    lit(&typedoc_path),
                    lit("--options"),
                    input("typedoc.json"),
                    lit("--sourceLinkTemplate"),
                    lit(&source_link_template),
                    lit("--json"),
                    output(),
                ],
                environment: BTreeMap::new(),
                version_arguments: vec![typedoc_path, "--version".into()],
                validation_arguments: None,
                output: "{kind='file',path='typedoc.json'}",
                format: "typedoc-json",
            },
        );
        let gomarkdoc_args = [
            "--format",
            "github",
            "--repository.url",
            "https://github.com/example/reference",
            "--repository.default-branch",
            "1111111111111111111111111111111111111111",
            "--repository.path",
            "/",
            "--output",
        ]
        .into_iter()
        .map(lit)
        .chain([output(), input_root()])
        .collect::<Vec<_>>();
        let mut gomarkdoc_check = gomarkdoc_args.clone();
        gomarkdoc_check.insert(gomarkdoc_check.len() - 1, lit("--check"));
        let gomarkdoc = Provider::new(
            tools.gomarkdoc,
            "GAZETTE_TEST_GOMARKDOC_SHA256",
            "GAZETTE_TEST_GOMARKDOC_VERSION_SHA256",
            ProviderRecipe {
                arguments: gomarkdoc_args,
                environment: go_environment(&tools.go_root, &cache.path().join("gomarkdoc")),
                version_arguments: vec!["--version".into()],
                validation_arguments: Some(gomarkdoc_check),
                output: "{kind='file',path='api.md'}",
                format: "markdown",
            },
        );
        let swagger = Provider::new(
            tools.swagger,
            "GAZETTE_TEST_SWAGGER_SHA256",
            "GAZETTE_TEST_SWAGGER_VERSION_SHA256",
            ProviderRecipe {
                arguments: ["generate", "spec", "--scan-models", "--output"]
                    .into_iter()
                    .map(lit)
                    .chain([output(), input("api.go")])
                    .collect(),
                environment: go_environment(&tools.go_root, &cache.path().join("swagger")),
                version_arguments: vec!["version".into()],
                validation_arguments: Some(vec![lit("validate"), output()]),
                output: "{kind='file',path='swagger.json'}",
                format: "swagger-json",
            },
        );
        let providers = [sphinx, rustdoc, typedoc, gomarkdoc, swagger];
        let expected = providers
            .iter()
            .enumerate()
            .map(|(index, provider)| Expected {
                version: provider.version_text.clone(),
                digest: provider.version_sha256,
                validates: index >= 3,
            })
            .collect();

        let mut config =
            "[[principals]]\nid='owner:brand'\nkind='person'\n[[sites]]\nid='site:brand'\npublisher='owner:brand'\ndeployable=false\n"
                .to_owned();
        for (id, subdirectory) in [
            ("manual", "inputs/manual"),
            ("sphinx", "inputs/sphinx"),
            ("rustdoc", "inputs/rustdoc"),
            ("typedoc", "inputs/typedoc"),
            ("gomarkdoc", "inputs/gomarkdoc"),
            ("swagger", "inputs/swagger"),
        ] {
            config.push_str(&format!(
                "[[sources]]\nid='source:{id}'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='{subdirectory}'}}\n",
                source.path().to_str().expect("UTF-8 source")
            ));
        }
        for project in ["manual", "python", "rust", "typescript", "go", "http"] {
            config.push_str(&format!(
                "[[projects]]\nid='project:{project}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"
            ));
        }
        config.push_str(&format!(
            "[[brands]]\nid='brand:one'\nname='One Brand Reference'\n[[brands.publications]]\nsite='site:brand'\n[brands.publications.documentation_repository]\nname='one-brand-domain-doc'\npath={:?}\n[brands.publications.documentation_repository.build]\ntarget={{kind='mintlify',theme='mint',primary_color='#123abc'}}\n",
            destination.to_str().expect("UTF-8 destination")
        ));
        config.push_str(
            "[[brands.publications.documentation_repository.build.modules]]\n\
             project='project:manual'\nslug='manual'\ntitle='Gazette manual'\n\
             manual={source='source:manual'}\n\
             [[brands.publications.documentation_repository.build.modules.navigation]]\n\
             kind='manual'\ntitle='Get started'\n\
             pages=[{title='Start',route='manual/start',input={kind='file',path='docs/start.md'}},{title='Install',route='manual/install',input={kind='file',path='docs/install.md'}}]\n",
        );
        for ((project, title, slug, source_id), provider) in [
            ("python", "Python / Sphinx", "python", "sphinx"),
            ("rust", "Rust / Rustdoc", "rust", "rustdoc"),
            (
                "typescript",
                "TypeScript / TypeDoc",
                "typescript",
                "typedoc",
            ),
            ("go", "Go / gomarkdoc", "go", "gomarkdoc"),
            ("http", "OpenAPI / Swagger", "http", "swagger"),
        ]
        .into_iter()
        .zip(&providers)
        {
            config.push_str(&format!(
                "[[brands.publications.documentation_repository.build.modules]]\nproject='project:{project}'\nslug='{slug}'\ntitle='{title}'\n[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='library'\ntitle='Reference'\ninput={{kind='generated',source='source:{source_id}',recipe={}}}\n",
                provider.recipe()
            ));
        }
        let source_before = source::inspect(source.path(), None, Default::default())
            .expect("source before generation");
        let destination_before = source::inspect(&destination, None, Default::default())
            .expect("destination before assembly");
        Self {
            source,
            jobs,
            outputs,
            _cache: cache,
            _destination_owner: destination_owner,
            destination,
            source_before,
            destination_before,
            expected,
            config,
        }
    }

    fn run(&self) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args([
                "assemble-docs",
                "--local",
                "--site",
                "site:brand",
                "--allow-execute",
                "--generator-parent",
            ])
            .arg(self.jobs.path())
            .arg("--output-parent")
            .arg(self.outputs.path())
            .arg("--max-generator-jobs")
            .arg("5")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real Gazette CLI");
        child
            .stdin
            .take()
            .expect("workspace stdin")
            .write_all(self.config.as_bytes())
            .expect("workspace configuration");
        child.wait_with_output().expect("Gazette completion")
    }
}

fn report(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("CLI receipt")
}

fn retained(report: &Value) -> &Path {
    Path::new(
        report["directory"]
            .as_str()
            .expect("retained Mintlify tree"),
    )
}

fn text(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).expect("staged UTF-8 output")
}

fn has_name(value: &Value, name: &str) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| has_name(value, name)),
        Value::Object(values) => {
            values.get("name").is_some_and(|value| value == name)
                || values.values().any(|value| has_name(value, name))
        }
        _ => false,
    }
}

#[test]
#[ignore = "requires explicit installed Sphinx, Rustdoc, Node+TypeDoc, gomarkdoc, Go and go-swagger provider paths"]
fn one_brand_owned_domain_doc_composes_installed_native_references_deterministically() {
    let fixture = Fixture::new();
    let first = report(fixture.run());
    let second = report(fixture.run());
    for report in [&first, &second] {
        assert_eq!(report["status"], "documentation-assembled-local");
        assert_eq!(report["repository"]["name"], "one-brand-domain-doc");
        assert_eq!(
            report["repository"]["path"],
            fixture.destination.to_str().unwrap()
        );
        let jobs = report["jobs"].as_array().expect("five native jobs");
        assert_eq!(jobs.len(), 5);
        let manual = report["manual"].as_array().expect("manual evidence");
        assert_eq!(manual.len(), 2);
        assert!(manual.iter().all(|item| item["kind"] == "page"));
        assert!(manual.iter().all(|item| item["source"] == "source:manual"));
        assert!(manual.iter().all(|item| item.get("generation").is_none()));
        assert_eq!(
            report["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|artifact| artifact["format"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "sphinx-json",
                "rustdoc-html",
                "typedoc-json",
                "markdown",
                "swagger-json"
            ]
        );
        for (job, expected) in jobs.iter().zip(&fixture.expected) {
            assert_eq!(job["evidence"]["version_text"], expected.version);
            assert_eq!(
                job["evidence"]["expected_version_sha256"],
                expected.digest.to_string()
            );
            assert_eq!(
                job["evidence"]["version"]["stdout_sha256"],
                expected.digest.to_string()
            );
            assert_eq!(job["evidence"]["generation"]["exit"]["code"], 0);
            if expected.validates {
                assert_eq!(
                    job["evidence"]["validation"]["invocation"]["exit"]["code"],
                    0
                );
                assert_eq!(
                    job["evidence"]["validation"]["tree"],
                    job["output_identity"]
                );
            } else {
                assert!(job["evidence"]["validation"].is_null());
            }
        }
    }

    let first_tree = source::inspect(retained(&first), None, Default::default()).unwrap();
    let second_tree = source::inspect(retained(&second), None, Default::default()).unwrap();
    assert_eq!(first_tree, second_tree);
    for entry in first_tree.entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(retained(&first).join(entry.path.as_str())).unwrap(),
                fs::read(retained(&second).join(entry.path.as_str())).unwrap()
            );
        }
    }

    let docs: Value =
        serde_json::from_slice(&fs::read(retained(&first).join("docs.json")).unwrap())
            .expect("Mintlify navigation");
    let groups = docs["navigation"]["groups"].as_array().unwrap();
    assert_eq!(
        groups
            .iter()
            .map(|group| group["group"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "Gazette manual",
            "Python / Sphinx",
            "Rust / Rustdoc",
            "TypeScript / TypeDoc",
            "Go / gomarkdoc",
            "OpenAPI / Swagger"
        ]
    );
    assert_eq!(
        groups[0]["pages"][0],
        json!({"group":"Get started","pages":["manual/start","manual/install"]})
    );
    assert_eq!(
        groups[1]["pages"][0]["sdk"],
        json!({"format":"sphinx","source":"artifacts/python/library","directory":"python/library"})
    );
    assert!(
        groups[2]["pages"][0]["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|page| page == "rust/library/fn.describe")
    );
    assert_eq!(
        groups[3]["pages"][0]["sdk"],
        json!({"format":"typedoc","source":"artifacts/typescript/library.json","directory":"typescript/library"})
    );
    assert_eq!(groups[4]["pages"][0]["pages"], json!(["go/library"]));
    assert_eq!(
        groups[5]["pages"][0]["openapi"],
        json!({"source":"artifacts/http/library.json","directory":"http/library"})
    );

    let manual_start = text(retained(&first), "manual/start.md");
    assert!(manual_start.contains("[Install](/manual/install#install)"));
    for link in [
        "[Python reference](/python/library)",
        "[Rust reference](/rust/library/fn.describe)",
        "[TypeScript reference](/typescript/library)",
        "[Go reference](/go/library)",
        "[HTTP reference](/http/library)",
    ] {
        assert!(manual_start.contains(link), "missing connected link {link}");
    }
    let manual_install = text(retained(&first), "manual/install.md");
    assert!(manual_install.contains("[Start](/manual/start#gazette-manual)"));

    let sphinx: Value = serde_json::from_slice(
        &fs::read(retained(&first).join("artifacts/python/library/model.fjson")).unwrap(),
    )
    .unwrap();
    let body = sphinx["body"].as_str().unwrap();
    for token in ["model.Record", "model.Record.scaled", "One retained value."] {
        assert!(body.contains(token), "Sphinx lost {token}");
    }
    let typedoc: Value = serde_json::from_slice(
        &fs::read(retained(&first).join("artifacts/typescript/library.json")).unwrap(),
    )
    .unwrap();
    for name in ["Envelope", "Choice", "wrap", "describe"] {
        assert!(has_name(&typedoc, name), "TypeDoc lost {name}");
    }
    let rust = text(retained(&first), "rust/library/model/struct.Record.md");
    assert!(rust.contains("Original payload, without conversion."));
    let go = text(retained(&first), "go/library.md");
    for token in [
        "type Record struct",
        "func New(value string) Record",
        "reference.New(reference.DefaultLabel)",
    ] {
        assert!(go.contains(token), "gomarkdoc lost {token}");
    }
    let openapi: Value = serde_json::from_slice(
        &fs::read(retained(&first).join("artifacts/http/library.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(openapi["openapi"], "3.0.3");
    assert_eq!(
        openapi["paths"]["/widgets/{id}"]["get"]["operationId"],
        "getWidget"
    );

    assert_eq!(
        source::inspect(fixture.source.path(), None, Default::default()).unwrap(),
        fixture.source_before
    );
    assert!(
        Repository::open(fixture.source.path())
            .unwrap()
            .statuses(None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        source::inspect(&fixture.destination, None, Default::default()).unwrap(),
        fixture.destination_before
    );
    assert_eq!(
        fs::read(fixture.destination.join("sentinel")).unwrap(),
        b"owner-controlled\n"
    );
}
