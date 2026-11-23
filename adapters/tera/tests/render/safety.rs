//! Pinned native execution semantics, not a second template interpreter.

use super::{Limits, TemplateSet, captured, compile_error, output};
use serde_json::json;

fn refuses_recursive_execution(files: &[(&str, &[u8])]) {
    let error = compile_error(&captured(files));
    assert_eq!(error.code(), "render_template", "{error}");
    assert!(error.to_string().contains("recursive"), "{error}");
    let healthy = captured(&[("healthy.html", b"{{ title }}")]);
    let set = TemplateSet::compile(&healthy, Limits::default()).expect("subsequent request");
    assert_eq!(
        output(&set, "healthy.html", json!({"title":"still healthy"}))
            .expect("subsequent native rendering"),
        "still healthy"
    );
}

#[test]
fn inherited_block_namespace_cycle_refuses_before_native_execution() {
    refuses_recursive_execution(&[
        (
            "base.html",
            b"{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}OK{% endmacro b %}{% block body %}base{% endblock body %}",
        ),
        (
            "child.html",
            b"{% extends 'base.html' %}{% import 'base.html' as base %}{% macro b() %}{{ base::a() }}{% endmacro b %}{% block body %}{{ base::a() }}{% endblock body %}",
        ),
    ]);
}

#[test]
fn included_block_super_uses_original_parent_and_cycle_refuses_before_execution() {
    refuses_recursive_execution(&[
        (
            "base.html",
            b"{% block body %}{% include 'part.html' %}{% endblock body %}",
        ),
        (
            "child.html",
            b"{% extends 'base.html' %}{% block body %}{{ super() }}{% endblock body %}",
        ),
        (
            "part.html",
            b"{% block body %}{{ super() }}{% endblock body %}",
        ),
    ]);
}

#[test]
fn macro_loader_publishes_before_parents_but_not_before_imports() {
    let safe = captured(&[
        (
            "base.html",
            b"{% import 'child.html' as child %}{% block body %}base{% endblock body %}",
        ),
        (
            "child.html",
            b"{% extends 'base.html' %}{% macro label() %}child{% endmacro label %}{% block body %}{{ self::label() }}{% endblock body %}",
        ),
    ]);
    let set = TemplateSet::compile(&safe, Limits::default()).expect("safe mixed load graph");
    assert_eq!(output(&set, "child.html", json!({})).unwrap(), "child");
    assert_eq!(output(&set, "base.html", json!({})).unwrap(), "base");

    refuses_recursive_execution(&[
        ("a.html", b"{% import 'b.html' as b %}a"),
        ("b.html", b"{% import 'a.html' as a %}b"),
    ]);
}

#[test]
fn include_fallback_uses_first_existing_literal_not_every_candidate() {
    let source = captured(&[
        (
            "page.html",
            b"{% include ['missing.html', 'terminal.html', 'page.html'] %}",
        ),
        ("terminal.html", b"terminal"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("shadowed cycle is inert");
    assert_eq!(output(&set, "page.html", json!({})).unwrap(), "terminal");
}

#[test]
fn extending_include_refuses_without_trying_later_existing_candidate() {
    let source = captured(&[
        (
            "page.html",
            b"{% include ['extends.html', 'terminal.html'] %}",
        ),
        (
            "extends.html",
            b"{% extends 'base.html' %}{% block body %}unreachable{% endblock body %}",
        ),
        ("base.html", b"base"),
        ("terminal.html", b"wrong fallback"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("native refusal is finite");
    let error = output(&set, "page.html", json!({})).expect_err("no successful fallback");
    assert_eq!(error.code(), "render_template");
    assert!(
        error
            .to_string()
            .contains("Inheritance in included templates is currently not supported"),
        "{error}"
    );
}

#[test]
fn macro_definition_is_not_template_body_execution() {
    let source = captured(&[(
        "page.html",
        b"{% macro unused() %}{% include 'page.html' %}{% endmacro unused %}OK",
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("inert definition body");
    assert_eq!(output(&set, "page.html", json!({})).unwrap(), "OK");
}

#[test]
fn native_transitive_and_parent_namespaces_keep_declared_precedence() {
    let source = captured(&[
        ("leaf.html", b"{% macro label() %}leaf{% endmacro label %}"),
        (
            "bridge.html",
            b"{% import 'leaf.html' as leaf %}{% macro label() %}bridge{% endmacro label %}",
        ),
        (
            "direct.html",
            b"{% macro label() %}direct{% endmacro label %}",
        ),
        (
            "grandparent.html",
            b"{% import 'leaf.html' as bridge %}{% block body %}grandparent{% endblock body %}",
        ),
        (
            "base.html",
            b"{% extends 'grandparent.html' %}{% import 'bridge.html' as bridge %}{% block body %}base{% endblock body %}",
        ),
        (
            "child.html",
            b"{% extends 'base.html' %}{% import 'direct.html' as leaf %}{% block body %}{{ bridge::label() }}|{{ leaf::label() }}{% endblock body %}",
        ),
        (
            "transitive.html",
            b"{% import 'bridge.html' as bridge %}{{ leaf::label() }}",
        ),
        (
            "ordered.html",
            b"{% import 'bridge.html' as bridge %}{% import 'direct.html' as leaf %}{{ leaf::label() }}",
        ),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("resolved native namespaces");
    assert_eq!(
        output(&set, "child.html", json!({})).unwrap(),
        "bridge|direct"
    );
    assert_eq!(output(&set, "transitive.html", json!({})).unwrap(), "leaf");
    assert_eq!(output(&set, "ordered.html", json!({})).unwrap(), "direct");
}

fn include_chain_files(prefix: &str, count: usize) -> Vec<(String, Vec<u8>)> {
    (0..count)
        .map(|index| {
            let name = format!("{prefix}{index:03}.html");
            let body = if index + 1 == count {
                b"done".to_vec()
            } else {
                format!("{{% include '{prefix}{:03}.html' %}}", index + 1).into_bytes()
            };
            (name, body)
        })
        .collect()
}

fn owned_capture(files: &[(String, Vec<u8>)]) -> super::CapturedSource {
    let borrowed: Vec<_> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    captured(&borrowed)
}

fn refuses_execution_depth(source: &super::CapturedSource, execution_depth: usize) {
    let error = match TemplateSet::compile(
        source,
        Limits {
            execution_depth,
            ..Limits::default()
        },
    ) {
        Ok(_) => panic!("complete execution exceeds the declared depth"),
        Err(error) => error,
    };
    assert!(matches!(error, super::Error::Limit(_)), "{error}");
    assert_eq!(error.code(), "render_limit");
    assert!(error.to_string().contains("depth"), "{error}");
}

#[test]
fn finite_include_depth_accepts_exact_sixty_four_and_refuses_one_less_or_one_more() {
    let files = include_chain_files("part-", 64);
    let source = owned_capture(&files);
    let set = TemplateSet::compile(&source, Limits::default()).expect("exact native frame bound");
    assert_eq!(output(&set, "part-000.html", json!({})).unwrap(), "done");
    refuses_execution_depth(&source, 63);
    let longer = owned_capture(&include_chain_files("part-", 65));
    refuses_execution_depth(&longer, 64);
}

#[test]
fn complete_longest_depth_counts_shared_tail_after_a_shorter_path() {
    let mut files = include_chain_files("tail-", 60);
    files.extend([
        (
            "00-root.html".to_owned(),
            b"{% include '01-short.html' %}{% include '02-long.html' %}".to_vec(),
        ),
        (
            "01-short.html".to_owned(),
            b"{% include 'tail-000.html' %}".to_vec(),
        ),
        (
            "02-long.html".to_owned(),
            b"{% include '03-middle.html' %}".to_vec(),
        ),
        (
            "03-middle.html".to_owned(),
            b"{% include '04-middle.html' %}".to_vec(),
        ),
        (
            "04-middle.html".to_owned(),
            b"{% include 'tail-000.html' %}".to_vec(),
        ),
    ]);
    let source = owned_capture(&files);
    let set =
        TemplateSet::compile(&source, Limits::default()).expect("complete longest path is 64");
    assert_eq!(output(&set, "00-root.html", json!({})).unwrap(), "donedone");
    refuses_execution_depth(&source, 63);
}

#[test]
fn macro_arguments_change_execution_scope_before_the_chosen_body() {
    let main = b"{% import 'macros.html' as m %}{{ m::run(x=m::seed()) }}".as_slice();
    let macros = b"{% macro seed() %}{% include 'part.html' %}{% endmacro seed %}{% macro run(x) %}{{ self::again() }}{% endmacro run %}".as_slice();
    refuses_recursive_execution(&[
        ("main.html", main),
        ("macros.html", macros),
        (
            "part.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let safe = captured(&[
        ("main.html", main),
        ("macros.html", macros),
        (
            "part.html",
            b"{% macro again() %}OK{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set = TemplateSet::compile(&safe, Limits::default()).expect("finite argument state");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OK");
}

#[test]
fn later_loop_iterations_keep_native_block_and_namespace_state() {
    let main = b"{% macro again() %}OK{% endmacro again %}{% for k in [0,1] %}{{ self::again() }}{% include 'part.html' %}{% endfor %}".as_slice();
    refuses_recursive_execution(&[
        ("main.html", main),
        (
            "part.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let safe = captured(&[
        ("main.html", main),
        (
            "part.html",
            b"{% macro again() %}next{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set = TemplateSet::compile(&safe, Limits::default()).expect("finite later iteration");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OKSnextS");
}

#[test]
fn condition_execution_state_reaches_its_selected_branch() {
    let main = b"{% import 'macros.html' as m %}{% if m::seed() %}{{ self::again() }}{% endif %}"
        .as_slice();
    let macros = b"{% macro seed() %}{% include 'part.html' %}{% endmacro seed %}".as_slice();
    refuses_recursive_execution(&[
        ("main.html", main),
        ("macros.html", macros),
        (
            "part.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let safe = captured(&[
        ("main.html", main),
        ("macros.html", macros),
        (
            "part.html",
            b"{% macro again() %}OK{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set = TemplateSet::compile(&safe, Limits::default()).expect("finite condition state");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OK");
}

#[test]
fn nonterminal_super_keeps_the_callers_block_for_a_later_super() {
    let source = captured(&[
        (
            "grandparent.html",
            b"{% block body %}G{% endblock body %}",
        ),
        (
            "base.html",
            b"{% extends 'grandparent.html' %}{% block body %}B{% endblock body %}",
        ),
        (
            "child.html",
            b"{% extends 'base.html' %}{% block body %}{{ super() }}{{ super() }}{% endblock body %}",
        ),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("finite native super pops");
    assert_eq!(output(&set, "child.html", json!({})).unwrap(), "BB");
}

#[test]
fn filter_section_body_changes_scope_before_its_arguments() {
    let main = b"{% macro again() %}OK{% endmacro again %}{% filter replace(from=self::again(), to='X') %}{% include 'part.html' %}{% endfilter %}".as_slice();
    refuses_recursive_execution(&[
        ("main.html", main),
        (
            "part.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let safe = captured(&[
        ("main.html", main),
        (
            "part.html",
            b"{% macro again() %}S{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set = TemplateSet::compile(&safe, Limits::default()).expect("finite filter argument state");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "X");
}

#[test]
fn single_supplied_argument_is_not_repeated_after_its_effects() {
    let source = captured(&[
        (
            "main.html",
            b"{% macro seed() %}{% include 'part.html' %}{% endmacro seed %}{% macro run(x) %}OK{% endmacro run %}{{ self::run(x=self::seed()) }}",
        ),
        (
            "part.html",
            b"{% macro seed() %}{{ self::seed() }}{% endmacro seed %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("one finite argument");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OK");
}

#[test]
fn execution_depth_default_and_explicit_profile_ceiling_are_native() {
    let defaults: Limits = serde_json::from_value(json!({})).expect("native default limits");
    assert_eq!(defaults.execution_depth, 64);
    assert_eq!(Limits::default().execution_depth, 64);
    let source = captured(&[("main.html", b"OK")]);
    refuses_execution_depth(&source, 0);
    refuses_execution_depth(&source, 65);
    let set = TemplateSet::compile(
        &source,
        Limits {
            execution_depth: 1,
            ..defaults
        },
    )
    .expect("minimum admitted body frame");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OK");
}

#[test]
fn nested_container_and_include_calls_share_one_execution_depth() {
    let mut files = include_chain_files("part-", 62);
    files.push((
        "main.html".to_owned(),
        b"{% if true %}{% include 'part-000.html' %}{% endif %}".to_vec(),
    ));
    let source = owned_capture(&files);
    let set = TemplateSet::compile(&source, Limits::default()).expect("one combined depth of 64");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "done");
    refuses_execution_depth(&source, 63);
}

#[test]
fn guaranteed_block_effects_do_not_reuse_the_pre_block_namespace() {
    let source = captured(&[
        (
            "main.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block body %}{% include 'part.html' %}{% endblock body %}{{ self::again() }}",
        ),
        (
            "part.html",
            b"{% macro again() %}OK{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
    ]);
    let set =
        TemplateSet::compile(&source, Limits::default()).expect("post-block native namespace");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "SOK");
}

#[test]
fn native_literal_loop_cardinality_does_not_invent_another_iteration() {
    let source = captured(&[
        (
            "main.html",
            b"{% macro again() %}OK{% endmacro again %}{% for k in [0] %}{{ self::again() }}{% include 'part.html' %}{% endfor %}",
        ),
        (
            "part.html",
            b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}",
        ),
        (
            "empty.html",
            b"{% for k in [] %}{% include 'empty.html' %}{% else %}empty{% endfor %}",
        ),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("exact literal cardinality");
    assert_eq!(output(&set, "main.html", json!({})).unwrap(), "OKS");
    assert_eq!(output(&set, "empty.html", json!({})).unwrap(), "empty");
}

#[test]
fn state_changing_unordered_arguments_refuse_across_independent_compilations() {
    let source = captured(&[
        (
            "main.html",
            b"{% import 'macros.html' as m %}{{ m::run(x=m::left(), y=m::right()) }}",
        ),
        (
            "macros.html",
            b"{% macro left() %}{% include 'left.html' %}{% endmacro left %}{% macro right() %}{% include 'right.html' %}{% endmacro right %}{% macro run(x,y) %}{{ self::again() }}{% endmacro run %}",
        ),
        (
            "left.html",
            b"{% import 'macros.html' as m %}{% macro again() %}LEFT{% endmacro again %}{% block stamp %}L{% endblock stamp %}",
        ),
        (
            "right.html",
            b"{% import 'macros.html' as m %}{% macro again() %}RIGHT{% endmacro again %}{% block stamp %}R{% endblock stamp %}",
        ),
    ]);
    let identity = source.inventory().identity();
    for _ in 0..16 {
        let error = compile_error(&source);
        assert_eq!(error.code(), "render_template", "{error}");
        assert!(error.to_string().contains("order-sensitive"), "{error}");
    }
    assert_eq!(source.inventory().identity(), identity);
}

#[test]
fn pure_multiple_macro_arguments_keep_exact_bytes_across_independent_compilations() {
    let source = captured(&[
        (
            "main.html",
            b"{% import 'macros.html' as m %}{{ m::run(x=m::left(), y=m::right()) }}",
        ),
        (
            "macros.html",
            b"{% macro left() %}L{% endmacro left %}{% macro right() %}R{% endmacro right %}{% macro run(x,y) %}{{ x }}{{ y }}{% endmacro run %}",
        ),
    ]);
    let identity = source.inventory().identity();
    for _ in 0..16 {
        let set = TemplateSet::compile(&source, Limits::default()).expect("pure native arguments");
        assert_eq!(set.identity(), identity);
        assert_eq!(output(&set, "main.html", json!({})).unwrap(), "LR");
    }
}

#[test]
fn repeated_shared_partial_paths_admit_without_rescanning_settled_bodies() {
    let mut files = include_chain_files("settled-", 30);
    for (index, (_, body)) in files.iter_mut().enumerate() {
        let mut repeated = if index + 1 < 30 {
            format!("{{% import 'settled-{:03}.html' as child %}}", index + 1).into_bytes()
        } else {
            Vec::new()
        };
        repeated.extend_from_slice(b"{% if available %}");
        for _ in 0..600 {
            repeated.extend_from_slice(body);
        }
        repeated.extend_from_slice(b"{% endif %}");
        *body = repeated;
    }
    for index in 0..100 {
        files.push((
            format!("surface-{index}.html"),
            b"{% extends 'settled-000.html' %}".to_vec(),
        ));
    }
    // Shared includes are called repeatedly but have the same finite admission
    // state. Actual render uses a false branch to keep output deliberately small.
    let source = owned_capture(&files);
    let mut limits = Limits::default();
    limits.templates.total_bytes = files.iter().map(|(_, body)| body.len() as u64).sum();
    let set = TemplateSet::compile(&source, limits)
        .expect("finite shared paths within the fixed safety profile");
    assert_eq!(
        output(&set, "settled-000.html", json!({"available":false})).unwrap(),
        ""
    );
}

#[test]
fn sequential_entries_reclaim_analysis_storage_but_one_oversized_entry_refuses() {
    let body = "{{ value }}".repeat(64);
    let files = (0..12)
        .map(|index| (format!("entry-{index:02}.html"), body.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    let mut limits = Limits::default();
    limits.templates.total_bytes = files.iter().map(|(_, body)| body.len() as u64).sum();
    let source = owned_capture(&files);
    let set = TemplateSet::compile(&source, limits)
        .expect("sequential finite entry analysis releases its owned metadata");
    assert_eq!(
        output(&set, "entry-11.html", json!({"value":"x"})).unwrap(),
        "x".repeat(64)
    );
    let one = owned_capture(&[("one.html".into(), body.repeat(12).into_bytes())]);
    let error = match TemplateSet::compile(&one, limits) {
        Ok(_) => panic!("one retained execution exceeds the unchanged storage ceiling"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error,
            super::Error::Limit("template safety analysis storage")
        ),
        "{error}"
    );
}

#[test]
fn effect_free_shared_macro_reuses_depth_summary_across_loader_cache_contexts() {
    let mut main = String::from("{% import 'attributes.html' as link %}");
    let mut files = Vec::new();
    for index in 0..8 {
        main.push_str(&format!(
            "{{% if flags %}}{{% include 'optional-{index}.html' %}}{{% endif %}}"
        ));
        files.push((format!("optional-{index}.html"), Vec::new()));
    }
    main.push_str("{{ link::attributes(kind=kind) }}");
    let attributes = format!(
        "{{% macro attributes(kind) %}}{}{{% endmacro attributes %}}",
        "{% if kind %} data-kind=\"{{ kind }}\"{% endif %}".repeat(40)
    );
    files.push(("main.html".into(), main.into_bytes()));
    files.push(("attributes.html".into(), attributes.into_bytes()));
    let source = owned_capture(&files);
    let mut limits = Limits::default();
    limits.templates.total_bytes = 256 << 10;
    let set = TemplateSet::compile(&source, limits)
        .expect("pure shared macro has one exact depth graph without duplicating it for every loaded-template subset");
    assert_eq!(
        output(
            &set,
            "main.html",
            json!({"flags":false,"kind":"cross-site"})
        )
        .unwrap(),
        " data-kind=\"cross-site\"".repeat(40)
    );
}

#[test]
fn shared_effect_free_macro_keeps_its_complete_expression_and_body_depth() {
    let source = captured(&[
        ("main.html", b"{% import 'macro.html' as m %}{{ m::run() }}"),
        ("macro.html", b"{% macro run() %}{% if a %}{% if b %}{% if c %}{{ value }}{% endif %}{% endif %}{% endif %}{% endmacro run %}"),
    ]);
    let too_shallow = Limits {
        execution_depth: 6,
        ..Default::default()
    };
    assert!(matches!(
        TemplateSet::compile(&source, too_shallow),
        Err(super::Error::Limit("template execution depth"))
    ));
    let exact = Limits {
        execution_depth: 7,
        ..Default::default()
    };
    let set = TemplateSet::compile(&source, exact).expect("exact unchanged graph depth");
    assert_eq!(
        output(
            &set,
            "main.html",
            json!({"a":true,"b":true,"c":true,"value":"kept"})
        )
        .unwrap(),
        "kept"
    );
}

#[test]
fn concatenated_function_arguments_cannot_hide_recursive_macros() {
    refuses_recursive_execution(&[("main.html", b"{% macro again() %}{{ throw(message=self::again()) ~ '' }}{% endmacro again %}{{ self::again() }}")]);
    refuses_recursive_execution(&[("main.html", b"{% macro again() %}{{ self::again() }}{% endmacro again %}{{ 'prefix' ~ throw(message=self::again()) }}")]);
}

#[test]
fn concatenated_function_arguments_keep_complete_execution_depth() {
    let source = captured(&[("main.html", b"{% macro value() %}{{ title }}{% endmacro value %}{{ get_env(name=self::value(), default='fallback') ~ '' }}")]);
    // Compile only: the global function is deliberately never executed.
    refuses_execution_depth(&source, 4);
    TemplateSet::compile(
        &source,
        Limits {
            execution_depth: 5,
            ..Limits::default()
        },
    )
    .expect("exact depth accepts");
}

#[test]
fn concatenated_function_calls_preserve_left_to_right_loader_scope() {
    refuses_recursive_execution(&[
        ("main.html", b"{% macro again() %}OK{% endmacro again %}{% macro seed() %}{% include 'part.html' %}{% endmacro seed %}{{ throw(message=self::seed()) ~ throw(message=self::again()) }}"),
        ("part.html", b"{% macro again() %}{{ self::again() }}{% endmacro again %}{% block stamp %}S{% endblock stamp %}"),
    ]);
}

#[test]
fn transitive_pure_helpers_share_analysis_across_unrelated_optional_includes() {
    let mut main = "{% import 'helpers.html' as ui %}".to_owned();
    let body = (0..24)
        .map(|_| "{% if value %}{{ value }}{% endif %}")
        .collect::<String>();
    let helpers = format!(
        "{{% macro inner(value) %}}{body}{{% endmacro inner %}}{{% macro outer(value) %}}{{{{ self::inner(value=value) }}}}{{% endmacro outer %}}"
    );
    let mut owned = vec![("helpers.html".to_owned(), helpers)];
    for index in 0..5 {
        main.push_str(&format!("{{% if flag{index} %}}{{% include 'leaf{index}.html' %}}{{% endif %}}{{{{ ui::outer(value=value) }}}}"));
        owned.push((format!("leaf{index}.html"), String::new()));
    }
    owned.push(("main.html".to_owned(), main));
    let files = owned
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_bytes()))
        .collect::<Vec<_>>();
    let mut limits = Limits::default();
    limits.templates.total_bytes = 16 << 10;
    let set = TemplateSet::compile(&captured(&files), limits)
        .expect("one transitive pure helper summary, no per-cache body copies");
    for value in [false, true] {
        let expected = if value {
            "x".repeat(5 * 24)
        } else {
            String::new()
        };
        assert_eq!(output(&set,"main.html",json!({"value":if value {"x"} else {""},"flag0":value,"flag1":value,"flag2":value,"flag3":value,"flag4":value})).unwrap(), expected);
    }
}

#[test]
fn many_optional_loads_keep_exact_macro_namespaces_and_native_output() {
    let mut main = String::new();
    let mut files = Vec::new();
    for index in 0..16 {
        main.push_str(&format!("{{% import 'helper-{index}.html' as h{index} %}}"));
        files.push((
            format!("helper-{index}.html"),
            format!("{{% macro value() %}}{index}{{% endmacro value %}}"),
        ));
    }
    for index in 0..10 {
        main.push_str(&format!("{{% if enabled %}}{{% include 'optional-{index}.html' %}}{{% endif %}}{{{{ h15::value() }}}}"));
        files.push((format!("optional-{index}.html"), String::new()));
    }
    files.push(("main.html".into(), main));
    let borrowed = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_bytes()))
        .collect::<Vec<_>>();
    let mut limits = Limits::default();
    limits.templates.total_bytes = 2 << 20;
    let set = TemplateSet::compile(&captured(&borrowed), limits)
        .expect("optional loader subsets share only exact nonempty namespaces");
    for enabled in [false, true] {
        assert_eq!(
            output(&set, "main.html", json!({"enabled":enabled})).unwrap(),
            "15".repeat(10)
        );
    }
}
