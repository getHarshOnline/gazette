//! Native dependency and test-placement checks for the composed repository.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use rtepl_repository::dependencies::{DependencyPolicy, check_dependencies};
use rtepl_repository::rust_source::{RustSourcePolicy, check_rust_source};
use rtepl_repository::source_entries;
use syn::visit::Visit;

#[test]
fn production_dependencies_follow_the_inward_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--offline",
        ])
        .current_dir(&root)
        .output()
        .expect("native Cargo metadata");
    assert!(output.status.success(), "{output:?}");
    let expected: BTreeMap<&str, BTreeSet<&str>> = [
        ("gazette-core", BTreeSet::new()),
        ("gazette-ports", BTreeSet::from(["gazette-core"])),
        (
            "gazette-integration-github",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-compiler",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-markdown",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-mintlify",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-documentation",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-fonts",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-tera",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-javascript",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-styles",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-raster",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-vector",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-application",
            BTreeSet::from(["gazette-core", "gazette-ports", "gazette-compiler"]),
        ),
        (
            "gazette-adapter-fs",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-http",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-process",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-authority",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-git",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-adapter-sqlite",
            BTreeSet::from(["gazette-core", "gazette-ports"]),
        ),
        (
            "gazette-sdk",
            BTreeSet::from([
                "gazette-core",
                "gazette-ports",
                "gazette-application",
                "gazette-compiler",
                "gazette-adapter-fs",
                "gazette-adapter-http",
                "gazette-adapter-process",
                "gazette-adapter-authority",
                "gazette-adapter-git",
                "gazette-adapter-sqlite",
                "gazette-adapter-tera",
                "gazette-adapter-javascript",
                "gazette-adapter-styles",
                "gazette-adapter-raster",
                "gazette-adapter-vector",
                "gazette-adapter-markdown",
                "gazette-adapter-mintlify",
                "gazette-adapter-documentation",
                "gazette-adapter-fonts",
                "gazette-integration-github",
            ]),
        ),
        ("gazette-cli", BTreeSet::from(["gazette-sdk"])),
        ("gazette-mcp", BTreeSet::from(["gazette-cli"])),
        ("gazette-lsp", BTreeSet::from(["gazette-sdk"])),
        ("gazette-node", BTreeSet::from(["gazette-sdk"])),
        (
            "gazette-launcher",
            BTreeSet::from(["gazette-cli", "gazette-mcp", "gazette-lsp"]),
        ),
    ]
    .into_iter()
    .collect();
    let policy = DependencyPolicy {
        forbidden: expected
            .keys()
            .filter(|name| name.starts_with("gazette-adapter-"))
            .map(|name| ((*name).to_owned(), "gazette-compiler".to_owned()))
            .collect(),
        production: expected
            .into_iter()
            .map(|(name, edges)| {
                (
                    name.to_owned(),
                    edges.into_iter().map(str::to_owned).collect(),
                )
            })
            .collect(),
        test_support: BTreeMap::from([("gazette-test-support".to_owned(), "tests/support".into())]),
    };
    let failures =
        check_dependencies(&root, &output.stdout, &policy).expect("Cargo workspace paths");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn source_modules_contain_no_inline_test_implementations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let failures: Vec<_> = ["crates", "adapters", "sdk", "interfaces", "integrations"]
        .into_iter()
        .flat_map(|directory| inspect_sources(&root.join(directory)))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn editor_interface_links_only_its_selected_authoring_and_configuration_adapters() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "gazette-lsp",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--locked",
            "--offline",
        ])
        .current_dir(&root)
        .output()
        .expect("native Cargo dependency tree");
    assert!(output.status.success(), "{output:?}");
    let tree = String::from_utf8(output.stdout).expect("UTF-8 Cargo dependency tree");
    assert!(
        tree.lines()
            .any(|line| line.starts_with("gazette-adapter-markdown ")),
        "the authoring interface must retain its selected Markdown adapter: {tree}"
    );
    assert!(
        tree.lines()
            .any(|line| line.starts_with("gazette-adapter-fs ")),
        "the editor interface must retain its selected confined configuration adapter: {tree}"
    );
    assert!(
        tree.lines()
            .any(|line| line.starts_with("gazette-adapter-git ")),
        "working-repository discovery needs the selected Git source adapter: {tree}"
    );
    for adapter in [
        "gazette-adapter-authority",
        "gazette-adapter-documentation",
        "gazette-adapter-fonts",
        "gazette-adapter-http",
        "gazette-adapter-javascript",
        "gazette-adapter-mintlify",
        "gazette-adapter-process",
        "gazette-adapter-raster",
        "gazette-adapter-vector",
        "gazette-adapter-sqlite",
        "gazette-adapter-styles",
        "gazette-adapter-tera",
        "gazette-integration-github",
    ] {
        assert!(
            !tree
                .lines()
                .any(|line| line.starts_with(&format!("{adapter} "))),
            "the authoring interface links unrelated native adapter {adapter}: {tree}"
        );
    }
}

#[test]
fn public_sdk_uses_are_curated_and_hide_native_adapters() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    inspect_public_api(&root);
}

#[test]
fn public_sdk_guard_rejects_adapter_types_in_fields_and_signatures() {
    for source in [
        "pub struct Leak { pub value: gazette_adapter_fixture::Observation }",
        "use gazette_adapter_fixture::Observation as Native; pub fn leak(_: Native) {}",
        "pub enum Leak { Variant(gazette_adapter_fixture::Observation) }",
    ] {
        let file = syn::parse_file(source).expect("synthetic public API");
        let aliases = adapter_aliases(&file);
        let rejected = std::panic::catch_unwind(|| {
            PublicApiBoundary {
                path: Path::new("synthetic-sdk-boundary.rs"),
                aliases: &aliases,
            }
            .visit_file(&file);
        });
        assert!(
            rejected.is_err(),
            "adapter type escaped the SDK guard: {source}"
        );
    }
}

#[test]
fn production_sources_do_not_embed_developer_or_package_manager_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    inspect_host_paths(&root);
}

fn inspect_host_paths(path: &Path) {
    for entry in source_entries(
        path,
        &[
            "target",
            "node_modules",
            ".git",
            ".codegraph",
            "docs",
            "tests",
            "vendor",
        ],
    )
    .expect("production source tree")
    {
        let path = entry.path;
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some(
                "rs" | "ts" | "js" | "mjs" | "py" | "sh" | "yaml" | "yml" | "toml" | "json" | "sql"
            )
        ) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("UTF-8 production source");
        for forbidden in [
            "/Users/",
            "/opt/homebrew/",
            "/private/tmp/",
            "/Applications/",
        ] {
            assert!(
                !text.contains(forbidden),
                "production source {} embeds host-specific path prefix {forbidden}",
                path.display()
            );
        }
    }
}

fn inspect_public_api(path: &Path) {
    for entry in source_entries(path, &[]).expect("SDK source tree") {
        let path = entry.path;
        if path.extension().is_some_and(|extension| extension == "rs") {
            let text = fs::read_to_string(&path).expect("Rust source");
            let file = syn::parse_file(&text).expect("valid Rust source");
            let aliases = adapter_aliases(&file);
            PublicApiBoundary {
                path: &path,
                aliases: &aliases,
            }
            .visit_file(&file);
        }
    }
}

struct PublicApiBoundary<'a> {
    path: &'a Path,
    aliases: &'a BTreeSet<String>,
}

impl PublicApiBoundary<'_> {
    fn inspect_type(&self, node: &syn::Type) {
        NativeAdapterTypeBoundary {
            path: self.path,
            aliases: self.aliases,
        }
        .visit_type(node);
    }

    fn inspect_signature(&self, signature: &syn::Signature) {
        NativeAdapterTypeBoundary {
            path: self.path,
            aliases: self.aliases,
        }
        .visit_signature(signature);
    }
}

impl Visit<'_> for PublicApiBoundary<'_> {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            let (root, glob) = use_root_and_glob(&item.tree);
            assert!(
                !root.starts_with("gazette_adapter_"),
                "public SDK use exposes native adapter {root} in {}",
                self.path.display()
            );
            assert!(
                !(root == "gazette_compiler" && glob),
                "public SDK use exposes an uncurated compiler glob in {}",
                self.path.display()
            );
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_fn(&mut self, item: &syn::ItemFn) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.inspect_signature(&item.sig);
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &syn::ImplItemFn) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.inspect_signature(&item.sig);
        }
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            for field in &item.fields {
                if matches!(field.vis, syn::Visibility::Public(_)) {
                    self.inspect_type(&field.ty);
                }
            }
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            for variant in &item.variants {
                for field in &variant.fields {
                    self.inspect_type(&field.ty);
                }
            }
        }
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_type(&mut self, item: &syn::ItemType) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.inspect_type(&item.ty);
        }
        syn::visit::visit_item_type(self, item);
    }

    fn visit_item_const(&mut self, item: &syn::ItemConst) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.inspect_type(&item.ty);
        }
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &syn::ItemStatic) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            self.inspect_type(&item.ty);
        }
        syn::visit::visit_item_static(self, item);
    }

    fn visit_item_trait(&mut self, item: &syn::ItemTrait) {
        if matches!(item.vis, syn::Visibility::Public(_)) {
            for member in &item.items {
                match member {
                    syn::TraitItem::Fn(function) => self.inspect_signature(&function.sig),
                    syn::TraitItem::Const(value) => self.inspect_type(&value.ty),
                    syn::TraitItem::Type(value) => {
                        let mut boundary = NativeAdapterTypeBoundary {
                            path: self.path,
                            aliases: self.aliases,
                        };
                        for bound in &value.bounds {
                            boundary.visit_type_param_bound(bound);
                        }
                        if let Some((_, default)) = &value.default {
                            boundary.visit_type(default);
                        }
                    }
                    _ => {}
                }
            }
        }
        syn::visit::visit_item_trait(self, item);
    }
}

struct NativeAdapterTypeBoundary<'a> {
    path: &'a Path,
    aliases: &'a BTreeSet<String>,
}

impl Visit<'_> for NativeAdapterTypeBoundary<'_> {
    fn visit_type_path(&mut self, item: &syn::TypePath) {
        if let Some(root) = item
            .path
            .segments
            .first()
            .map(|segment| segment.ident.to_string())
        {
            assert!(
                !root.starts_with("gazette_adapter_") && !self.aliases.contains(&root),
                "public SDK declaration exposes native adapter type {root} in {}",
                self.path.display()
            );
        }
        syn::visit::visit_type_path(self, item);
    }
}

fn adapter_aliases(file: &syn::File) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    for item in &file.items {
        let syn::Item::Use(item) = item else { continue };
        collect_adapter_aliases(&item.tree, false, &mut aliases);
    }
    aliases
}

fn collect_adapter_aliases(
    tree: &syn::UseTree,
    inside_adapter: bool,
    aliases: &mut BTreeSet<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let inside = inside_adapter || path.ident.to_string().starts_with("gazette_adapter_");
            collect_adapter_aliases(&path.tree, inside, aliases);
        }
        syn::UseTree::Name(name) if inside_adapter => {
            aliases.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename) if inside_adapter => {
            aliases.insert(rename.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_adapter_aliases(item, inside_adapter, aliases);
            }
        }
        syn::UseTree::Glob(_) if inside_adapter => {
            panic!("SDK source must not glob-import native adapter types")
        }
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
}

fn use_root_and_glob(tree: &syn::UseTree) -> (String, bool) {
    match tree {
        syn::UseTree::Path(path) => {
            let (_, glob) = use_root_and_glob(&path.tree);
            (path.ident.to_string(), glob)
        }
        syn::UseTree::Name(name) => (name.ident.to_string(), false),
        syn::UseTree::Rename(rename) => (rename.ident.to_string(), false),
        syn::UseTree::Glob(_) => (String::new(), true),
        syn::UseTree::Group(group) => group.items.iter().map(use_root_and_glob).fold(
            (String::new(), false),
            |(root, glob), (next, next_glob)| {
                (if root.is_empty() { next } else { root }, glob || next_glob)
            },
        ),
    }
}

fn inspect_sources(path: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    for entry in source_entries(
        path,
        &[
            "target",
            "node_modules",
            ".git",
            ".codegraph",
            "tests",
            "vendor",
        ],
    )
    .expect("source tree")
    {
        let relative = entry
            .path
            .strip_prefix(path)
            .expect("entry beneath source root");
        let in_source = relative
            .parent()
            .is_some_and(|parent| parent.components().any(|part| part.as_os_str() == "src"));
        let path = entry.path;
        if in_source && path.extension().is_some_and(|ext| ext == "rs") {
            let text = fs::read_to_string(&path).expect("Rust source");
            let source_failures = check_rust_source(
                &text,
                &RustSourcePolicy {
                    test_functions: true,
                    inline_test_modules: true,
                    forbidden_methods: &["unwrap", "unwrap_err"],
                },
            )
            .expect("valid Rust source");
            failures.extend(
                source_failures
                    .into_iter()
                    .map(|failure| format!("{}: {failure}", path.display())),
            );
        }
    }
    failures
}
