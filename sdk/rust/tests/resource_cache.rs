//! Cache reuse preserves actual JS/Sass output and the complete resource contract.

use std::{cell::Cell, collections::BTreeMap};

use gazette_application::build::{ResourceCache, ResourceCacheLimits};
use gazette_core::{
    resource::{Artifact, Error, Limits, ModuleLinks, Recipe},
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput},
};
use gazette_ports::resource::ResourceCompiler;

struct Counting<'a, C> {
    calls: &'a Cell<usize>,
    compiler: C,
}

impl<C: ResourceCompiler> ResourceCompiler for Counting<'_, C> {
    fn compile(
        &self,
        input: &TreeInput<'_>,
        recipe: &Recipe,
        links: &ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        self.calls.set(self.calls.get() + 1);
        self.compiler.compile(input, recipe, links, limits)
    }
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("flat retained fixture path")
}

fn capture(files: &[(&str, &[u8])]) -> CapturedSource {
    let files: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| (path(name), bytes.to_vec()))
        .collect();
    let mut inventory = InventoryBuilder::new(Default::default());
    for (path, bytes) in &files {
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("bounded native input");
    }
    CapturedSource::from_parts(inventory.finish(), files).expect("verified capture")
}

fn script(entry: &str) -> Recipe {
    Recipe::BrowserModule {
        entry: path(entry),
        imports: BTreeMap::new(),
    }
}

fn cache<C: ResourceCompiler>(compiler: C) -> ResourceCache<C> {
    ResourceCache::new(
        compiler,
        ResourceCacheLimits {
            entries: 32,
            bytes: 8 << 20,
        },
    )
}

fn compiled(
    compiler: &impl ResourceCompiler,
    source: &CapturedSource,
    recipe: &Recipe,
) -> Artifact {
    compiler
        .compile(
            &source.as_input(),
            recipe,
            &ModuleLinks::new(),
            Limits::default(),
        )
        .expect("native resource compilation")
}

fn same(actual: &Artifact, expected: &Artifact) {
    assert_eq!(actual.input, expected.input);
    assert_eq!(actual.bytes, expected.bytes);
    assert_eq!(actual.dependencies, expected.dependencies);
    assert_eq!(actual.references, expected.references);
    assert_eq!(actual.image, expected.image);
    assert_eq!(actual.tool, expected.tool);
}

#[test]
fn hits_preserve_native_bytes_and_evidence_while_changed_imports_and_unused_inputs_miss() {
    let calls = Cell::new(0);
    let cache = cache(Counting {
        calls: &calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    let fixture = |dependency: &'static [u8], unused: &'static [u8]| {
        capture(&[
            (
                "main.ts",
                b"import { value } from './dependency.ts'; export const answer: number = value;",
            ),
            ("dependency.ts", dependency),
            ("unused.ts", unused),
        ])
    };
    let first = fixture(b"export const value = 1;", b"export const unused = 0;");
    let recipe = script("main.ts");
    let native = compiled(&gazette_adapter_javascript::Compiler, &first, &recipe);
    let mut returned = compiled(&cache, &first, &recipe);
    same(&returned, &native);
    assert_eq!(calls.get(), 1);
    // Callers own returned artifacts; mutating one must not corrupt the retained entry.
    returned.bytes.clear();
    returned.dependencies.clear();
    let hit = compiled(&cache, &first, &recipe);
    same(&hit, &native);
    assert_eq!(calls.get(), 1);

    let imported = fixture(b"export const value = 2;", b"export const unused = 0;");
    let changed = compiled(&cache, &imported, &recipe);
    assert_eq!(calls.get(), 2);
    assert_ne!(changed.bytes, native.bytes);
    same(
        &changed,
        &compiled(&gazette_adapter_javascript::Compiler, &imported, &recipe),
    );

    let unused = fixture(b"export const value = 2;", b"export const unused = 99;");
    let complete = compiled(&cache, &unused, &recipe);
    assert_eq!(
        calls.get(),
        3,
        "complete declared tree, not only reachable imports"
    );
    assert_eq!(complete.bytes, changed.bytes);
    assert_ne!(complete.input, changed.input);
    same(
        &complete,
        &compiled(&gazette_adapter_javascript::Compiler, &unused, &recipe),
    );
    same(&compiled(&cache, &unused, &recipe), &complete);
    assert_eq!(calls.get(), 3);
}

#[test]
fn exact_recipe_and_module_links_are_independent_cache_inputs() {
    let calls = Cell::new(0);
    let cache = cache(Counting {
        calls: &calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    let source = capture(&[
        ("main.ts", b"export { value } from '@shared/value';"),
        (
            "other.ts",
            b"export { value } from '@shared/value'; export const another = 42;",
        ),
    ]);
    let recipe = |key: &str| Recipe::BrowserModule {
        entry: path("main.ts"),
        imports: BTreeMap::from([(
            "@shared/value".into(),
            key.parse().expect("module identity"),
        )]),
    };
    let mut links = ModuleLinks::from([(
        "module:a".parse().unwrap(),
        "https://example.test/a.js".into(),
    )]);
    let first = cache
        .compile(
            &source.as_input(),
            &recipe("module:a"),
            &links,
            Limits::default(),
        )
        .expect("first linked module");
    let hit = cache
        .compile(
            &source.as_input(),
            &recipe("module:a"),
            &links,
            Limits::default(),
        )
        .expect("identical links");
    same(&hit, &first);
    assert_eq!(calls.get(), 1);

    links = ModuleLinks::from([(
        "module:b".parse().unwrap(),
        "https://example.test/b.js".into(),
    )]);
    let changed_recipe = cache
        .compile(
            &source.as_input(),
            &recipe("module:b"),
            &links,
            Limits::default(),
        )
        .expect("new import binding");
    assert_eq!(calls.get(), 2);
    assert_ne!(first.bytes, changed_recipe.bytes);
    assert_ne!(first.references, changed_recipe.references);

    links.insert(
        "module:b".parse().unwrap(),
        "https://other.test/b.js".into(),
    );
    let changed_links = cache
        .compile(
            &source.as_input(),
            &recipe("module:b"),
            &links,
            Limits::default(),
        )
        .expect("new link address");
    assert_eq!(calls.get(), 3);
    assert_ne!(changed_recipe.bytes, changed_links.bytes);
    same(
        &changed_links,
        &gazette_adapter_javascript::Compiler
            .compile(
                &source.as_input(),
                &recipe("module:b"),
                &links,
                Limits::default(),
            )
            .expect("uncached equivalent"),
    );
    let other_recipe = Recipe::BrowserModule {
        entry: path("other.ts"),
        imports: BTreeMap::from([("@shared/value".into(), "module:b".parse().unwrap())]),
    };
    let other = cache
        .compile(&source.as_input(), &other_recipe, &links, Limits::default())
        .expect("different entry with identical import bindings and module links");
    assert_eq!(calls.get(), 4);
    assert_ne!(other.bytes, changed_links.bytes);
    assert_eq!(other.references, changed_links.references);

    // An extra link violates the native adapter's exact declared-key contract;
    // a prior cached success must not hide that failure.
    links.insert(
        "module:unused".parse().unwrap(),
        "https://example.test/unused.js".into(),
    );
    for expected in [5, 6] {
        let extra_link = cache
            .compile(
                &source.as_input(),
                &recipe("module:b"),
                &links,
                Limits::default(),
            )
            .expect_err("unused identity must not be accepted from cache");
        assert!(matches!(
            extra_link,
            Error::Input {
                reason: "module links must match declared identities exactly",
                ..
            }
        ));
        assert_eq!(calls.get(), expected, "invalid links are not retained");
    }
    links.remove(
        &"module:unused"
            .parse::<gazette_core::resource::ResourceId>()
            .unwrap(),
    );
    same(
        &cache
            .compile(
                &source.as_input(),
                &recipe("module:b"),
                &links,
                Limits::default(),
            )
            .expect("original valid link map still cached"),
        &changed_links,
    );
    assert_eq!(calls.get(), 6);
}

#[test]
fn every_limit_field_participates_even_when_this_native_compiler_does_not_use_it() {
    let calls = Cell::new(0);
    let cache = cache(Counting {
        calls: &calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    let source = capture(&[("main.js", b"export const answer = 42;")]);
    let recipe = script("main.js");
    let baseline = compiled(&cache, &source, &recipe);
    assert_eq!(calls.get(), 1);
    let default = Limits::default();
    let mut variants = [default; 9];
    variants[0].inputs.entries -= 1;
    variants[1].inputs.depth -= 1;
    variants[2].inputs.file_bytes -= 1;
    variants[3].inputs.total_bytes -= 1;
    variants[4].output_bytes -= 1;
    variants[5].raster.encoded_bytes -= 1;
    variants[6].raster.decoded_bytes -= 1;
    variants[7].raster.width -= 1;
    variants[8].raster.height -= 1;
    for (index, limits) in variants.into_iter().enumerate() {
        let result = cache
            .compile(&source.as_input(), &recipe, &ModuleLinks::new(), limits)
            .expect("still sufficient native limits");
        same(&result, &baseline);
        assert_eq!(calls.get(), index + 2, "changed limit {index}");
        same(
            &cache
                .compile(&source.as_input(), &recipe, &ModuleLinks::new(), limits)
                .expect("same limited invocation"),
            &result,
        );
        assert_eq!(calls.get(), index + 2, "exact limited invocation hits");
    }
}

#[test]
fn errors_are_not_retained_and_a_prior_success_cannot_bypass_tighter_limits() {
    let calls = Cell::new(0);
    let cache = cache(Counting {
        calls: &calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    let source = capture(&[("main.js", b"export const answer = 42;")]);
    let recipe = script("main.js");
    let successful = compiled(&cache, &source, &recipe);
    let limited = Limits {
        output_bytes: 1,
        ..Limits::default()
    };
    for expected in [2, 3] {
        let error = cache
            .compile(&source.as_input(), &recipe, &ModuleLinks::new(), limited)
            .expect_err("earlier success must not evade new limit");
        assert_eq!(error.code(), "resource_limit");
        assert_eq!(calls.get(), expected, "failed compilation is not memoized");
    }
    same(&compiled(&cache, &source, &recipe), &successful);
    assert_eq!(
        calls.get(),
        3,
        "failed attempts do not replace the successful key"
    );
    let malformed = capture(&[("main.js", b"export const = ;")]);
    for expected in [4, 5] {
        assert!(
            cache
                .compile(
                    &malformed.as_input(),
                    &recipe,
                    &ModuleLinks::new(),
                    Limits::default()
                )
                .is_err()
        );
        assert_eq!(
            calls.get(),
            expected,
            "invalid syntax always reaches the native compiler"
        );
    }
}

#[test]
fn bounded_fifo_evicts_oldest_insertion_and_hits_do_not_renew_its_position() {
    let calls = Cell::new(0);
    let cache = ResourceCache::new(
        Counting {
            calls: &calls,
            compiler: gazette_adapter_javascript::Compiler,
        },
        ResourceCacheLimits {
            entries: 2,
            bytes: 8 << 20,
        },
    );
    let sources = [
        capture(&[("main.js", b"export const value = 1;")]),
        capture(&[("main.js", b"export const value = 2;")]),
        capture(&[("main.js", b"export const value = 3;")]),
    ];
    let recipe = script("main.js");
    let first = compiled(&cache, &sources[0], &recipe);
    compiled(&cache, &sources[1], &recipe);
    same(&compiled(&cache, &sources[0], &recipe), &first);
    assert_eq!(calls.get(), 2);
    compiled(&cache, &sources[2], &recipe);
    assert_eq!(calls.get(), 3);
    compiled(&cache, &sources[1], &recipe);
    assert_eq!(calls.get(), 3, "second insertion remains retained");
    same(&compiled(&cache, &sources[0], &recipe), &first);
    assert_eq!(
        calls.get(),
        4,
        "first insertion was evicted despite its intervening hit"
    );
}

#[test]
fn byte_capacity_evicts_even_when_entry_capacity_has_room() {
    let calls = Cell::new(0);
    let cache = ResourceCache::new(
        Counting {
            calls: &calls,
            compiler: gazette_adapter_javascript::Compiler,
        },
        ResourceCacheLimits {
            entries: 8,
            bytes: 16 << 10,
        },
    );
    let first_text = format!("export const value = {:?};", "a".repeat(8192));
    let second_text = format!("export const value = {:?};", "b".repeat(8192));
    let first = capture(&[("main.js", first_text.as_bytes())]);
    let second = capture(&[("main.js", second_text.as_bytes())]);
    let recipe = script("main.js");
    let expected = compiled(&cache, &first, &recipe);
    assert!(expected.bytes.len() > 8192);
    same(&compiled(&cache, &first, &recipe), &expected);
    assert_eq!(calls.get(), 1, "one complete artifact fits the byte budget");
    compiled(&cache, &second, &recipe);
    assert_eq!(calls.get(), 2);
    compiled(&cache, &second, &recipe);
    assert_eq!(calls.get(), 2, "second complete artifact is retained");
    same(&compiled(&cache, &first, &recipe), &expected);
    assert_eq!(
        calls.get(),
        3,
        "aggregate byte budget evicted the first artifact"
    );
}

#[test]
fn zero_capacity_oversized_artifacts_and_fresh_compilers_do_not_share_retained_results() {
    let source = capture(&[("main.js", b"export const answer = 42;")]);
    let recipe = script("main.js");
    let native = compiled(&gazette_adapter_javascript::Compiler, &source, &recipe);
    for limits in [
        ResourceCacheLimits {
            entries: 0,
            bytes: 8 << 20,
        },
        ResourceCacheLimits {
            entries: 8,
            bytes: 0,
        },
        ResourceCacheLimits {
            entries: 8,
            bytes: 1,
        },
    ] {
        let calls = Cell::new(0);
        let cache = ResourceCache::new(
            Counting {
                calls: &calls,
                compiler: gazette_adapter_javascript::Compiler,
            },
            limits,
        );
        for expected in [1, 2] {
            same(&compiled(&cache, &source, &recipe), &native);
            assert_eq!(
                calls.get(),
                expected,
                "bypass still returns real complete output"
            );
        }
    }
    let large_text = format!("export const value = {:?};", "x".repeat(4096));
    let large = capture(&[("main.js", large_text.as_bytes())]);
    let large_native = compiled(&gazette_adapter_javascript::Compiler, &large, &recipe);
    assert!(large_native.bytes.len() > 2048);
    let large_calls = Cell::new(0);
    let too_large = ResourceCache::new(
        Counting {
            calls: &large_calls,
            compiler: gazette_adapter_javascript::Compiler,
        },
        ResourceCacheLimits {
            entries: 8,
            bytes: 2048,
        },
    );
    for expected in [1, 2] {
        same(&compiled(&too_large, &large, &recipe), &large_native);
        assert_eq!(
            large_calls.get(),
            expected,
            "oversized output bypasses retention"
        );
    }
    let first_calls = Cell::new(0);
    let second_calls = Cell::new(0);
    let first = cache(Counting {
        calls: &first_calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    let second = cache(Counting {
        calls: &second_calls,
        compiler: gazette_adapter_javascript::Compiler,
    });
    same(&compiled(&first, &source, &recipe), &native);
    same(&compiled(&second, &source, &recipe), &native);
    assert_eq!((first_calls.get(), second_calls.get()), (1, 1));
    same(&compiled(&second, &source, &recipe), &native);
    assert_eq!(second_calls.get(), 1);
}

#[test]
fn sass_compiler_uses_the_same_cache_contract_without_reconstructing_dependency_evidence() {
    let calls = Cell::new(0);
    let cache = cache(Counting {
        calls: &calls,
        compiler: gazette_adapter_styles::Compiler,
    });
    let source = capture(&[
        (
            "main.scss",
            b"@use 'tokens'; .card { color: tokens.$color; }",
        ),
        ("_tokens.scss", b"$color: #123456;"),
        ("unused.scss", b"invalid unselected Sass {"),
    ]);
    let recipe = Recipe::Stylesheet {
        entry: path("main.scss"),
    };
    let native = compiled(&gazette_adapter_styles::Compiler, &source, &recipe);
    assert_eq!(
        native.dependencies,
        [path("_tokens.scss"), path("main.scss")]
            .into_iter()
            .collect()
    );
    for _ in 0..2 {
        same(&compiled(&cache, &source, &recipe), &native);
    }
    assert_eq!(calls.get(), 1);
    assert!(
        std::str::from_utf8(&native.bytes)
            .expect("CSS")
            .contains("#123456")
    );
}
