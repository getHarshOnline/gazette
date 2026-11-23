//! Captured-only rendering, adapter semantics and explicit resource-boundary proofs.

use std::collections::{BTreeMap, BTreeSet};

use gazette_adapter_tera::{Compiler, Error, Limits, TemplateSet};
use gazette_core::source::{
    CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput,
};
use gazette_ports::render::{CompiledTemplates, TemplateCompiler};
use serde_json::json;

#[path = "render/css_url.rs"]
mod css_url;
#[path = "render/editorial_date.rs"]
mod editorial_date;
#[path = "render/json_script.rs"]
mod json_script;
#[path = "render/markdown_text.rs"]
mod markdown_text;
#[path = "render/safety.rs"]
mod safety;

fn captured(files: &[(&str, &[u8])]) -> CapturedSource {
    let mut directories = BTreeSet::new();
    let mut payloads = BTreeMap::new();
    for (name, bytes) in files {
        let mut ancestor = *name;
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            directories.insert(parent);
            ancestor = parent;
        }
        payloads.insert(name.parse::<EntryPath>().expect("path"), bytes.to_vec());
    }
    let mut builder = InventoryBuilder::new(Default::default());
    for directory in directories {
        builder
            .insert(directory.parse().expect("directory"), Content::Directory)
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
            .expect("file");
    }
    CapturedSource::from_parts(builder.finish(), payloads).expect("capture")
}

fn output(set: &TemplateSet, name: &str, context: serde_json::Value) -> Result<String, Error> {
    let bytes = set.render(&name.parse().expect("entry"), &context)?;
    Ok(String::from_utf8(bytes).expect("Tera emits UTF-8"))
}

fn compile_port(
    source: &CapturedSource,
    limits: Limits,
) -> Result<Box<dyn CompiledTemplates>, Error> {
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
    let input = TreeInput::new(source.inventory(), files).expect("borrowed checked tree");
    let compiler: &dyn TemplateCompiler = &Compiler;
    compiler.compile(&input, limits)
}

fn compile_error(source: &CapturedSource) -> Error {
    match TemplateSet::compile(source, Limits::default()) {
        Err(error) => error,
        Ok(_) => panic!("invalid template compiled"),
    }
}

fn has_native_tera_cause(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(current) = cause {
        if current.downcast_ref::<tera::Error>().is_some() {
            return true;
        }
        cause = current.source();
    }
    false
}

#[test]
fn dynamic_port_preserves_identity_and_rendering_after_borrowed_input_is_dropped() {
    let source = captured(&[
        (
            "base.html",
            b"<main>{% block body %}{% endblock body %}</main>",
        ),
        (
            "page.html",
            b"{% extends 'base.html' %}{% block body %}{{ title }}{% endblock body %}",
        ),
    ]);
    let identity = source.inventory().identity();
    let direct = TemplateSet::compile(&source, Limits::default()).expect("direct compilation");
    let dynamic = compile_port(&source, Limits::default()).expect("dynamic compilation");
    drop(source);
    let entry = "page.html".parse().expect("entry");
    let context = json!({"title":"नमस्ते <world>"});
    assert_eq!(dynamic.identity(), identity);
    assert_eq!(dynamic.identity(), direct.identity());
    assert_eq!(
        dynamic.profile(),
        "tera@1.20.1/gazette-captured-templates-v8"
    );
    assert_eq!(dynamic.profile(), direct.profile());
    let bytes = dynamic
        .render(&entry, &context)
        .expect("retained templates");
    assert_eq!(bytes, "<main>नमस्ते &lt;world&gt;</main>".as_bytes());
    assert_eq!(
        bytes,
        direct
            .render(&entry, &context)
            .expect("same implementation")
    );
    // The port promises an owned Send + Sync result, not a borrow into the input.
    let other = std::thread::spawn(move || dynamic.render(&entry, &context));
    assert_eq!(other.join().expect("thread").expect("render"), bytes);
}

#[test]
fn dynamic_port_keeps_forbidden_functions_errors_and_limits() {
    let source = captured(&[
        ("environment.html", b"{{ get_env(name='HOME') }}"),
        ("clock.html", b"{{ now() }}"),
        ("random.html", b"{{ get_random(start=0,end=1) }}"),
        (
            "range.html",
            b"{% for n in range(end=1000000000) %}{{ n }}{% endfor %}",
        ),
        ("page.html", b"{{ value }}"),
    ]);
    let templates = compile_port(
        &source,
        Limits {
            output_bytes: 3,
            ..Limits::default()
        },
    )
    .expect("dynamic templates");
    for entry in [
        "environment.html",
        "clock.html",
        "random.html",
        "range.html",
        "missing.html",
    ] {
        let result = templates.render(&entry.parse().expect("entry"), &json!({}));
        let Err(Error::Template(error)) = result else {
            panic!("{entry}: expected native renderer failure");
        };
        assert!(
            has_native_tera_cause(error.as_ref()),
            "original renderer error retained"
        );
    }
    let page = "page.html".parse().expect("entry");
    assert!(matches!(
        templates.render(&page, &json!([])),
        Err(Error::ContextShape)
    ));
    assert!(matches!(
        templates.render(&page, &json!({"value":"four"})),
        Err(Error::Limit("output bytes"))
    ));
    assert_eq!(
        templates
            .render(&page, &json!({"value":"ok"}))
            .expect("reusable"),
        b"ok"
    );
    let templates = compile_port(
        &source,
        Limits {
            context_bytes: 1,
            ..Limits::default()
        },
    )
    .expect("context-bound templates");
    assert!(matches!(
        templates.render(&page, &json!({})),
        Err(Error::Limit("context bytes"))
    ));
}

#[test]
fn dynamic_port_rejects_complete_set_input_and_renderer_parse_failures() {
    let source = captured(&[("page.html", b"{% extends 'missing.html' %}")]);
    let Err(Error::Template(error)) = compile_port(&source, Limits::default()) else {
        panic!("invalid inheritance must reject dynamic compilation");
    };
    assert!(has_native_tera_cause(error.as_ref()));
    let source = captured(&[("bad.html", &[0xff])]);
    assert!(matches!(
        compile_port(&source, Limits::default()),
        Err(Error::Input { .. })
    ));
    let mut limits = Limits::default();
    limits.templates.entries = 0;
    assert!(matches!(
        compile_port(&source, limits),
        Err(Error::Limit("template entries"))
    ));
}

#[test]
fn diagnostics_name_template_line_column_and_deepest_reason() {
    let malformed = captured(&[("broken.html", b"first\n{{ value %}\nlast")]);
    let error = compile_error(&malformed);
    let message = error.to_string();
    assert!(message.contains("template 'broken.html'"), "{message}");
    assert!(message.contains("line 2, column"), "{message}");
    assert!(message.contains("expected"), "{message}");

    let source = captured(&[("page.html", b"heading\n{{ missing_value }}\nfooter")]);
    let templates = TemplateSet::compile(&source, Limits::default()).expect("templates");
    let error = templates
        .render(&"page.html".parse().expect("entry"), &json!({}))
        .expect_err("undefined variable");
    let message = error.to_string();
    assert!(message.contains("template 'page.html'"), "{message}");
    assert!(message.contains("line 2, column 4"), "{message}");
    assert!(
        message.contains("Variable `missing_value` not found"),
        "{message}"
    );
}

#[test]
fn recursive_macro_is_rejected_before_rendering() {
    let source = captured(&[(
        "page.html",
        b"{% macro r(n) %}{{ self::r(n=n+1) }}{% endmacro %}{{ self::r(n=0) }}",
    )]);
    let error = compile_error(&source);
    let message = error.to_string();
    assert!(
        message.contains("recursive template macro `r`"),
        "{message}"
    );
    assert!(
        message.contains("template 'page.html' admission:"),
        "{message}"
    );
    assert!(
        !has_native_tera_cause(&error),
        "admission is not a native runtime cause"
    );

    let healthy = captured(&[("page.html", b"{{ title }}")]);
    let templates = TemplateSet::compile(&healthy, Limits::default()).expect("fresh request");
    assert_eq!(
        output(&templates, "page.html", json!({"title":"server survived"}))
            .expect("subsequent render"),
        "server survived"
    );
}

#[test]
fn captured_inheritance_includes_macros_and_unicode_render_without_source_access() {
    let files: [(&str, &[u8]); 4] = [
        ("base.html", b"{% import 'macros.html' as m %}<main>{% block body %}base{% endblock body %}</main>"),
        ("page.html", b"{% extends 'base.html' %}{% block body %}{{ super() }}:{% include 'parts/title.html' %}:{{ m::label(value=label) }}{% endblock body %}"),
        ("parts/title.html", b"<h1>{{ title }}</h1>"),
        ("macros.html", b"{% macro label(value) %}[{{ value }}]{% endmacro label %}"),
    ];
    let source = captured(&files);
    let identity = source.inventory().identity();
    let set = TemplateSet::compile(&source, Limits::default()).expect("templates");
    assert_eq!(set.identity(), identity);
    drop(source);
    let context = json!({"title":"नमस्ते <tag>", "label":"é🌟"});
    let first = output(&set, "page.html", context.clone()).expect("render");
    assert_eq!(first, "<main>base:<h1>नमस्ते &lt;tag&gt;</h1>:[é🌟]</main>");
    assert_eq!(first, output(&set, "page.html", context).expect("repeat"));
    let mut reversed = files;
    reversed.reverse();
    let other = captured(&reversed);
    let other = TemplateSet::compile(&other, Limits::default()).expect("order independence");
    assert_eq!(other.identity(), identity);
    assert_eq!(
        first,
        output(
            &other,
            "page.html",
            json!({"label":"é🌟", "title":"नमस्ते <tag>"})
        )
        .expect("same bytes")
    );
}

#[test]
fn escaping_is_extension_based_and_safe_is_an_explicit_trusted_template_bypass() {
    let source = captured(&[
        ("page.html", b"{{ value }}|{{ value | safe }}"),
        ("page.txt", b"{{ value }}"),
        ("page.xml", b"{{ value }}"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("templates");
    let context = json!({"value":"<em>A&B</em>"});
    assert_eq!(
        output(&set, "page.html", context.clone()).expect("html"),
        "&lt;em&gt;A&amp;B&lt;&#x2F;em&gt;|<em>A&B</em>"
    );
    assert_eq!(
        output(&set, "page.txt", context.clone()).expect("text"),
        "<em>A&B</em>"
    );
    assert_eq!(
        output(&set, "page.xml", context).expect("xml"),
        "&lt;em&gt;A&amp;B&lt;&#x2F;em&gt;"
    );
}

#[test]
fn default_filter_treats_explicit_null_and_missing_values_the_same() {
    let source = captured(&[(
        "page.html",
        br#"{{ explicit_null | default(value="fallback") }}|{{ missing | default(value="fallback") }}"#,
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("templates");
    assert_eq!(
        output(&set, "page.html", json!({"explicit_null":null})).expect("render"),
        "fallback|fallback"
    );
}

#[test]
fn missing_includes_entries_data_and_unsupported_functions_are_errors() {
    let source = captured(&[
        ("include.html", b"before{% include '/etc/passwd' %}after"),
        ("value.html", b"{{ missing }}"),
        ("environment.html", b"{{ get_env(name='HOME') }}"),
        ("clock.html", b"{{ now() }}"),
        ("random.html", b"{{ get_random(start=0,end=100) }}"),
        (
            "range.html",
            b"{% for n in range(end=1000000000) %}{{ n }}{% endfor %}",
        ),
        ("filter.html", b"{{ value | unsupported }}"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("syntax is valid");
    for name in [
        "absent.html",
        "include.html",
        "value.html",
        "environment.html",
        "clock.html",
        "random.html",
        "range.html",
        "filter.html",
    ] {
        assert!(
            matches!(
                output(&set, name, json!({"value":"x"})),
                Err(Error::Template(_))
            ),
            "{name}"
        );
    }
    assert!(matches!(
        output(&set, "value.html", json!([])),
        Err(Error::ContextShape)
    ));
}

#[test]
fn native_inheritance_and_syntax_errors_prevent_a_partial_template_set() {
    for files in [
        vec![("page.html", b"{% extends 'absent.html' %}".as_slice())],
        vec![("page.html", b"{% if %}".as_slice())],
        vec![
            ("a.html", b"{% extends 'b.html' %}".as_slice()),
            ("b.html", b"{% extends 'a.html' %}".as_slice()),
        ],
    ] {
        assert!(matches!(
            TemplateSet::compile(&captured(&files), Limits::default()),
            Err(Error::Template(_))
        ));
    }
    assert!(matches!(
        TemplateSet::compile(&captured(&[("bad.html", &[0xff])]), Limits::default()),
        Err(Error::Input { .. })
    ));
}

#[test]
fn byte_ceilings_include_unicode_context_and_whole_template_input() {
    let source = captured(&[("page.html", "é🌟".as_bytes())]);
    let limits = Limits {
        output_bytes: 6,
        ..Default::default()
    };
    let set = TemplateSet::compile(&source, limits).expect("templates");
    assert_eq!(
        output(&set, "page.html", json!({})).expect("exact UTF-8 bound"),
        "é🌟"
    );
    let set = TemplateSet::compile(
        &source,
        Limits {
            output_bytes: 5,
            ..limits
        },
    )
    .expect("templates");
    assert!(matches!(
        output(&set, "page.html", json!({})),
        Err(Error::Limit("output bytes"))
    ));
    let set = TemplateSet::compile(
        &source,
        Limits {
            context_bytes: 1,
            ..limits
        },
    )
    .expect("templates");
    assert!(matches!(
        output(&set, "page.html", json!({})),
        Err(Error::Limit("context bytes"))
    ));
    let mut limited = limits;
    limited.templates.total_bytes = 5;
    assert!(matches!(
        TemplateSet::compile(&source, limited),
        Err(Error::Limit("template bytes"))
    ));
    limited = limits;
    limited.templates.entries = 0;
    assert!(matches!(
        TemplateSet::compile(&source, limited),
        Err(Error::Limit("template entries"))
    ));
}

#[test]
fn overflow_during_expansion_returns_no_partial_success_and_set_remains_reusable() {
    let source = captured(&[(
        "page.html",
        b"{% for value in values %}{{ value }}{% endfor %}",
    )]);
    let set = TemplateSet::compile(
        &source,
        Limits {
            output_bytes: 4,
            ..Default::default()
        },
    )
    .expect("templates");
    assert!(matches!(
        output(&set, "page.html", json!({"values":["abc","def"]})),
        Err(Error::Limit("output bytes"))
    ));
    assert_eq!(
        output(&set, "page.html", json!({"values":["ok"]})).expect("independent render"),
        "ok"
    );
}

#[test]
fn query_component_filter_is_registered_without_ambient_functions_or_unsafe_html() {
    let source = captured(&[(
        "page.html",
        br#"<a href="?q={{ query | url_query }}&amp;scope=all">Search</a>"#,
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).unwrap();
    assert_eq!(
        output(&set, "page.html", json!({"query":"x&scope=other 雪"})).unwrap(),
        "<a href=\"?q=x%26scope%3Dother%20%E9%9B%AA&amp;scope=all\">Search</a>"
    );
    assert!(output(&set, "page.html", json!({"query":["wrong"]})).is_err());
}
