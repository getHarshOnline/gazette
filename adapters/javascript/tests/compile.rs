//! Real adapter behavior through the resource port, with no filesystem fixtures.

use std::collections::{BTreeMap, BTreeSet};

use gazette_adapter_javascript::Compiler;
use gazette_core::resource::{Artifact, Error, Limits, Recipe, ResourceId};
use gazette_core::source::{
    CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput,
};
use gazette_ports::resource::ResourceCompiler;

fn path(value: &str) -> EntryPath {
    value.parse().expect("logical fixture path")
}

fn capture(files: &[(&str, &[u8])]) -> CapturedSource {
    let mut directories = BTreeSet::new();
    let mut payloads = BTreeMap::new();
    for (name, bytes) in files {
        let mut ancestor = *name;
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            directories.insert(parent);
            ancestor = parent;
        }
        payloads.insert(path(name), bytes.to_vec());
    }
    let mut builder = InventoryBuilder::new(Default::default());
    for directory in directories {
        builder
            .insert(path(directory), Content::Directory)
            .expect("directory");
    }
    for (path, bytes) in &payloads {
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("captured file");
    }
    CapturedSource::from_parts(builder.finish(), payloads).expect("verified bytes")
}

fn compile(source: &CapturedSource, entry: &str, limits: Limits) -> Result<Artifact, Error> {
    compile_linked(source, entry, BTreeMap::new(), &BTreeMap::new(), limits)
}

fn compile_linked(
    source: &CapturedSource,
    entry: &str,
    imports: BTreeMap<String, ResourceId>,
    links: &gazette_core::resource::ModuleLinks,
    limits: Limits,
) -> Result<Artifact, Error> {
    let files = source
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            source
                .file(&entry.path)
                .map(|bytes| (entry.path.clone(), bytes))
        })
        .collect();
    let input = TreeInput::new(source.inventory(), files).expect("borrowed tree");
    let compiler: &dyn ResourceCompiler = &Compiler;
    compiler.compile(
        &input,
        &Recipe::BrowserModule {
            entry: path(entry),
            imports,
        },
        links,
        limits,
    )
}

fn text(artifact: &Artifact) -> &str {
    std::str::from_utf8(&artifact.bytes).expect("JavaScript is UTF-8")
}

#[test]
fn compact_emission_preserves_runtime_tokens_strings_and_automatic_semicolons() {
    let code = r#"
        export function inspect(value) {
            const message = `first line
second line: ${value}`;
            const expression = /a\/b/.test('a/b');
            const object = () => ({ value });
            function absent() {
                return
                { value };
            }
            return [value + +value, value - -value, object(), absent(), message, expression];
        }
        console.log(JSON.stringify(inspect(2)));
    "#;
    let artifact = compile(
        &capture(&[("main.js", code.as_bytes())]),
        "main.js",
        Limits::default(),
    )
    .expect("compact valid module");
    assert!(artifact.bytes.len() < code.len());
    assert_eq!(
        artifact.tool,
        "swc_core@80.0.0+oxc@0.95.0/browser-esm-v3-deferred"
    );
    let node = std::env::var_os("GAZETTE_TEST_NODE")
        .expect("set GAZETTE_TEST_NODE to the Node executable, as described in docs/install.md");
    let run = |source: &str| {
        let output = std::process::Command::new(&node)
            .args(["--input-type=module", "--eval", source])
            .output()
            .expect("execute the module in Node");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    assert_eq!(run(text(&artifact)), run(code));
}

#[test]
fn marked_notices_survive_reachable_bundling_in_stable_order_and_output_bounds() {
    let source = capture(&[
        ("main.ts", b"/*! Entry notice */\nimport { value } from './dependency.ts'; export const answer: number = value; // @preserve trailing entry\n// ordinary comment is omitted\n"),
        ("dependency.ts", b"/** @license Dependency permission and warranty */\nexport const value = 42; export const unused = 99;"),
        ("unselected.ts", b"/*! Unselected notice */ export const absent = 1;"),
    ]);
    let artifact =
        compile(&source, "main.ts", Limits::default()).expect("native notice preservation");
    let output = text(&artifact);
    for notice in [
        "Entry notice",
        "@preserve trailing entry",
        "Dependency permission and warranty",
    ] {
        assert_eq!(
            output.matches(notice).count(),
            1,
            "complete single notice: {notice}"
        );
    }
    assert!(!output.contains("Unselected notice"));
    assert!(!output.contains("ordinary comment"));
    assert!(output.find("Dependency permission").unwrap() < output.find("Entry notice").unwrap());
    assert_eq!(
        artifact.bytes,
        compile(&source, "main.ts", Limits::default())
            .unwrap()
            .bytes
    );
    let limits = Limits {
        output_bytes: artifact.bytes.len() - 1,
        ..Limits::default()
    };
    assert!(matches!(
        compile(&source, "main.ts", limits),
        Err(Error::Limit("resource output bytes"))
    ));
}

#[test]
fn notice_markers_inside_literals_are_not_extracted_as_comments() {
    let source = capture(&[("main.js", br#"export const literal = "/*! string-only notice */"; export const pattern = /@license/; // unrelated\n"#)]);
    let artifact = compile(&source, "main.js", Limits::default()).expect("native literal parsing");
    assert_eq!(text(&artifact).matches("string-only notice").count(), 1);
    assert_eq!(text(&artifact).matches("@license").count(), 1);
    assert!(!text(&artifact).contains("unrelated"));
}

#[test]
fn declared_modules_remain_linked_while_relative_imports_bundle_from_retained_files() {
    let source = capture(&[
        ("main.ts", b"import { value } from './nested/value.ts'; import '@shared/state'; export { increment as advance } from '@shared/state'; export * from '@shared/values'; export const answer: number = value; export const literal = '@shared/state';"),
        ("nested/value.ts", b"import { count } from '@shared/state'; export const value: number = count + 1;"),
        ("unselected.ts", b"invalid unselected source"),
    ]);
    let state = "module:state".parse().expect("state key");
    let values = "module:values".parse().expect("values key");
    let imports = BTreeMap::from([
        ("@shared/state".to_owned(), state),
        ("@shared/values".to_owned(), values),
    ]);
    let links = BTreeMap::from([
        (
            "module:state".parse().expect("state"),
            "https://example.test/prefix/assets/state.js".to_owned(),
        ),
        (
            "module:values".parse().expect("values"),
            "https://example.test/prefix/assets/values.js".to_owned(),
        ),
    ]);
    let artifact = compile_linked(
        &source,
        "main.ts",
        imports.clone(),
        &links,
        Limits::default(),
    )
    .expect("explicit external links");
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([path("main.ts"), path("nested/value.ts")])
    );
    assert_eq!(artifact.references, links.values().cloned().collect());
    assert_eq!(artifact.input, source.inventory().identity());
    assert!(
        text(&artifact).contains("@shared/state"),
        "ordinary string literal is not rewritten"
    );
    assert!(!text(&artifact).contains("./nested/value.ts"));
    assert!(!text(&artifact).contains(": number"));
    assert!(text(&artifact).contains("advance"));
    let arena = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&arena, text(&artifact), oxc_span::SourceType::mjs()).parse();
    assert!(parsed.errors.is_empty(), "actual emitted ESM parses");
    let again = compile_linked(&source, "main.ts", imports, &links, Limits::default())
        .expect("retained repeat");
    assert_eq!(artifact.bytes, again.bytes);
    assert_eq!(artifact.references, again.references);
}

#[test]
fn missing_extra_or_unsafe_module_bindings_refuse_without_source_or_url_fallback() {
    let source = capture(&[
        (
            "main.js",
            b"import { value } from '@shared/value'; export { value };",
        ),
        ("@shared/value.js", b"export const value = 99;"),
    ]);
    let key = "module:value".parse().expect("module key");
    let imports = BTreeMap::from([("@shared/value".to_owned(), key)]);
    assert!(
        compile_linked(
            &source,
            "main.js",
            imports.clone(),
            &BTreeMap::new(),
            Limits::default()
        )
        .is_err()
    );
    for url in [
        "",
        "file:///tmp/value.js",
        "https://user@example.test/value.js",
        "https://example.test/value.js?raw",
        "https://example.test/value.js#part",
        "https://example.test/a/../value.js",
        "https://example.test/value.js\n",
        "https://example.test/directory/",
    ] {
        let links = BTreeMap::from([("module:value".parse().expect("module"), url.to_owned())]);
        assert!(
            compile_linked(
                &source,
                "main.js",
                imports.clone(),
                &links,
                Limits::default()
            )
            .is_err(),
            "{url:?}"
        );
    }
    let mut links = BTreeMap::from([(
        "module:value".parse().expect("module"),
        "https://example.test/value.js".to_owned(),
    )]);
    assert!(
        compile_linked(
            &source,
            "main.js",
            BTreeMap::new(),
            &links,
            Limits::default()
        )
        .is_err()
    );
    links.insert(
        "module:extra".parse().expect("extra"),
        "https://example.test/extra.js".to_owned(),
    );
    assert!(compile_linked(&source, "main.js", imports, &links, Limits::default()).is_err());
    let direct = capture(&[("main.js", b"import 'https://example.test/value.js';")]);
    assert!(
        compile(&direct, "main.js", Limits::default()).is_err(),
        "a literal URL is not a declared alias"
    );
}

#[test]
fn erased_type_only_and_unused_declarations_do_not_create_runtime_module_references() {
    let source = capture(&[(
        "main.ts",
        b"import type { Value } from '@shared/types'; export const value: Value = 42;",
    )]);
    let imports = BTreeMap::from([(
        "@shared/types".to_owned(),
        "module:types".parse().expect("module"),
    )]);
    let links = BTreeMap::from([(
        "module:types".parse().expect("module"),
        "https://example.test/types.js".to_owned(),
    )]);
    let artifact = compile_linked(&source, "main.ts", imports, &links, Limits::default())
        .expect("type erasure");
    assert!(artifact.references.is_empty());
    assert_eq!(artifact.dependencies, BTreeSet::from([path("main.ts")]));
    assert!(!text(&artifact).contains("types.js"));
}

#[test]
fn stylesheet_recipe_cannot_be_reinterpreted_as_a_browser_module() {
    let source = capture(&[("main.js", b"export const answer = 42;")]);
    let entry = path("main.js");
    let input = TreeInput::new(
        source.inventory(),
        BTreeMap::from([(entry.clone(), source.file(&entry).expect("entry"))]),
    )
    .expect("retained tree");
    let error = Compiler
        .compile(
            &input,
            &Recipe::Stylesheet { entry },
            &BTreeMap::new(),
            Limits::default(),
        )
        .expect_err("a different recipe cannot select the JavaScript compiler");
    assert_eq!(error.code(), "resource_input");
}

#[test]
fn actual_multifile_bundle_retains_exports_side_effects_and_exact_consumed_dependencies() {
    let source = capture(&[
        ("ui/main.ts", b"import './setup.js'; import { message } from '../shared/message.ts'; export { double } from '../shared/math.js'; export const title: string = message;"),
        ("ui/setup.js", b"console.log('setup-marker');"),
        ("shared/message.ts", b"interface NotRuntime { body: string } export const message: string = 'retained-message';"),
        ("shared/math.js", b"export function double(value) { return value * 2; }"),
        ("unused.ts", b"this is deliberately invalid unused source"),
    ]);
    let artifact = compile(&source, "ui/main.ts", Limits::default()).expect("real bundle");
    assert_eq!(artifact.input, source.inventory().identity());
    assert_eq!(
        artifact.dependencies,
        BTreeSet::from([
            path("ui/main.ts"),
            path("ui/setup.js"),
            path("shared/message.ts"),
            path("shared/math.js"),
        ])
    );
    assert!(text(&artifact).contains("setup-marker"));
    assert!(text(&artifact).contains("retained-message"));
    assert!(text(&artifact).contains("export"));
    assert!(text(&artifact).contains("double"));
    assert!(text(&artifact).contains("title"));
    assert!(!text(&artifact).contains(": string"));
    assert!(!text(&artifact).contains("interface NotRuntime"));
    assert!(!text(&artifact).contains("../shared"));
    assert!(!text(&artifact).contains("/Users/"));
    assert!(!text(&artifact).contains("sourceMappingURL"));
    // Actual emitted JavaScript must parse and bundle again through the same port.
    let output = capture(&[("bundle.js", &artifact.bytes)]);
    compile(&output, "bundle.js", Limits::default()).expect("valid emitted ESM");
    drop(source);
    assert!(text(&artifact).contains("retained-message"));
}

#[test]
fn input_order_and_repeat_invocations_do_not_change_emitted_bytes() {
    let files: [(&str, &[u8]); 3] = [
        (
            "main.ts",
            b"import { value } from './value.ts'; export { value };",
        ),
        ("value.ts", b"export const value: number = 42;"),
        ("unused.js", b"export const ignored = false;"),
    ];
    let first = compile(&capture(&files), "main.ts", Limits::default()).expect("first");
    let mut reversed = files;
    reversed.reverse();
    for _ in 0..5 {
        let next = compile(&capture(&reversed), "main.ts", Limits::default()).expect("repeat");
        assert_eq!(first.bytes, next.bytes);
        assert_eq!(first.input, next.input);
        assert_eq!(first.dependencies, next.dependencies);
    }
    let changed = capture(&[
        ("main.ts", files[0].1),
        ("value.ts", b"export const value: number = 99;"),
        ("unused.js", files[2].1),
    ]);
    let next = compile(&changed, "main.ts", Limits::default()).expect("changed dependency");
    assert_ne!(first.bytes, next.bytes);
    assert_ne!(first.input, next.input);
}

#[test]
fn source_is_immutable_and_type_erasure_is_not_type_checking() {
    let mut original = b"export const value: number = 'intentional-type-error';".to_vec();
    let source = capture(&[("main.ts", &original)]);
    original.fill(b'x');
    let artifact = compile(&source, "main.ts", Limits::default()).expect("syntax erasure");
    assert!(text(&artifact).contains("intentional-type-error"));
    assert!(!text(&artifact).contains(": number"));
}

#[test]
fn missing_implicit_external_and_asset_dependencies_fail_without_fallback() {
    for specifier in [
        "./missing.ts",
        "./present",
        "../outside.ts",
        "package",
        "@theme/hook",
        "/tmp/present.ts",
        "https://example.test/app.js",
        "./data.json",
        "./style.css",
        "./present.ts?raw",
        "./present%2Ets",
        "./nested//present.ts",
    ] {
        let main = format!("import '{specifier}'; export const entry = true;");
        let source = capture(&[
            ("main.ts", main.as_bytes()),
            ("present.ts", b"export const present = true;"),
            ("data.json", b"{}"),
            ("style.css", b"body {}"),
        ]);
        let error = compile(&source, "main.ts", Limits::default()).expect_err(specifier);
        assert_eq!(error.code(), "resource_input", "{specifier}: {error}");
    }
    let source = capture(&[("main.js", b"export default 1;")]);
    assert!(matches!(
        compile(&source, "missing.js", Limits::default()),
        Err(Error::Input { .. })
    ));
}

#[test]
fn unsupported_runtime_seams_and_invalid_syntax_are_rejected() {
    for (entry, code) in [
        ("main.js", "export const broken = ;"),
        ("main.js", "export const typed: number = 1;"),
        ("main.ts", "const x = 1; const x = 2; export { x };"),
        (
            "main.js",
            "export const loader = () => import('./next.js');",
        ),
        (
            "main.js",
            "const value = require('./next.js'); export { value };",
        ),
        ("main.js", "module.exports = 42;"),
        ("main.js", "exports.value = 42;"),
        ("main.js", "export const url = import.meta.url;"),
        (
            "main.ts",
            "import value = require('./next.js'); export { value };",
        ),
        ("main.ts", "export = 42;"),
        ("main.tsx", "export const value = <div/>;"),
        (
            "main.js",
            "import data from './next.js' with { type: 'json' }; export { data };",
        ),
    ] {
        let source = capture(&[(entry, code.as_bytes()), ("next.js", b"export default 1;")]);
        assert!(
            compile(&source, entry, Limits::default()).is_err(),
            "{entry}: {code}"
        );
    }
    let invalid_utf8 = capture(&[("main.js", &[0xff])]);
    assert!(matches!(
        compile(&invalid_utf8, "main.js", Limits::default()),
        Err(Error::Input { .. })
    ));
}

#[test]
fn ordinary_locally_scoped_names_are_not_mistaken_for_commonjs_globals() {
    let source = capture(&[(
        "main.js",
        b"export function echo(module, exports, require) { return module + exports + require; }",
    )]);
    compile(&source, "main.js", Limits::default()).expect("ordinary local bindings");
}

#[test]
fn lexical_early_errors_are_distinct_from_types_and_valid_nested_shadowing() {
    for source_text in [
        "const value = 1; const value = 2; export { value };",
        "export const value = 1; export { value };",
        "export { missing };",
        "export function f(value, value) { return value; }",
        "export function f() { let value; let value; return value; }",
    ] {
        let source = capture(&[("main.js", source_text.as_bytes())]);
        let error = compile(&source, "main.js", Limits::default()).expect_err(source_text);
        assert_eq!(error.code(), "resource_compile", "{source_text}: {error}");
    }
    let source = capture(&[("main.ts", b"const value: number = 1; export function nested() { const value: string = 'inner'; return value; } export { value };")]);
    compile(&source, "main.ts", Limits::default()).expect("different lexical scopes");
}

#[test]
fn input_and_output_budgets_fail_closed_at_the_native_port() {
    let source = capture(&[(
        "nested/main.ts",
        b"export const message: string = 'bounded';",
    )]);
    for input_limits in [
        gazette_core::source::Limits {
            entries: 0,
            ..Default::default()
        },
        gazette_core::source::Limits {
            depth: 1,
            ..Default::default()
        },
        gazette_core::source::Limits {
            file_bytes: 1,
            ..Default::default()
        },
        gazette_core::source::Limits {
            total_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            compile(
                &source,
                "nested/main.ts",
                Limits {
                    inputs: input_limits,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit(_))
        ));
    }
    let output = compile(&source, "nested/main.ts", Limits::default()).expect("baseline");
    for bound in [0, output.bytes.len() - 1] {
        assert!(matches!(
            compile(
                &source,
                "nested/main.ts",
                Limits {
                    output_bytes: bound,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("resource output bytes"))
        ));
    }
    let exact = compile(
        &source,
        "nested/main.ts",
        Limits {
            output_bytes: output.bytes.len(),
            ..Limits::default()
        },
    )
    .expect("exact byte ceiling");
    assert_eq!(exact.bytes, output.bytes);
}

#[test]
fn declared_dynamic_imports_are_linked_but_not_eager() {
    let source = capture(&[("main.js", b"export const start = () => import('@shared/later'); export const label = '@shared/later';")]);
    let id: ResourceId = "module:later".parse().unwrap();
    let url = "https://example.test/assets/later.js".to_owned();
    let imports = BTreeMap::from([("@shared/later".to_owned(), id.clone())]);
    let links = BTreeMap::from([(id, url.clone())]);
    let result = compile_linked(
        &source,
        "main.js",
        imports.clone(),
        &links,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(result.references, BTreeSet::from([url.clone()]));
    assert_eq!(result.deferred_references, result.references);
    assert!(text(&result).contains("import("));
    assert!(text(&result).contains("@shared/later"));
    let node = std::env::var_os("GAZETTE_TEST_NODE")
        .expect("set GAZETTE_TEST_NODE as described in docs/install.md");
    let script = format!(
        r#"
        import {{ registerHooks }} from 'node:module';
        import assert from 'node:assert/strict';
        registerHooks({{ resolve(specifier, context, next) {{
            if (specifier === 'https://example.test/assets/later.js') return {{
                shortCircuit: true, url: 'data:text/javascript,' + encodeURIComponent('globalThis.loaded = (globalThis.loaded || 0) + 1; export const answer = 42;')
            }};
            return next(specifier, context);
        }} }});
        {module}
        assert.equal(globalThis.loaded, undefined);
        assert.equal((await start()).answer, 42);
        assert.equal((await start()).answer, 42);
        assert.equal(globalThis.loaded, 1);
    "#,
        module = text(&result)
    );
    let output = std::process::Command::new(node)
        .args(["--input-type=module", "--eval", &script])
        .output()
        .expect("execute deferred module");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        result.bytes,
        compile_linked(
            &source,
            "main.js",
            imports.clone(),
            &links,
            Limits::default()
        )
        .unwrap()
        .bytes
    );
    let eager = capture(&[(
        "main.js",
        b"import '@shared/later'; export const start = () => import('@shared/later');",
    )]);
    let result = compile_linked(
        &eager,
        "main.js",
        imports.clone(),
        &links,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(result.references, BTreeSet::from([url]));
    assert!(result.deferred_references.is_empty());
    for invalid in [
        "export const f = name => import(name);",
        "export const f = () => import('./later.js');",
        "export const f = () => import('https://example.test/assets/later.js');",
        "export const f = () => import('@shared/later', {with: {type: 'json'}});",
    ] {
        let source = capture(&[("main.js", invalid.as_bytes())]);
        assert!(
            compile_linked(
                &source,
                "main.js",
                imports.clone(),
                &links,
                Limits::default()
            )
            .is_err(),
            "{invalid}"
        );
    }
}
