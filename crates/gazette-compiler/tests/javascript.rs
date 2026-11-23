//! Real native JavaScript syntax decoding, explicit goals and failure boundaries.

use std::{convert::Infallible, error::Error as _};

use gazette_compiler::javascript::{self, Error, Goal, Limits, TextKind};

fn text(
    source: &[u8],
    goal: Goal,
    limits: Limits,
) -> Result<Vec<(TextKind, String)>, Error<Infallible>> {
    let mut values = Vec::new();
    javascript::inspect(source, goal, limits, |kind, value| {
        values.push((kind, value.to_owned()));
        Ok(())
    })?;
    Ok(values)
}

#[test]
fn native_identifier_families_and_nested_template_chunks_are_decoded() {
    let source = br#"import {source as imported} from "mod\u0075le";
class Cl\u0061ss { #pr\u0069vate = 1; m\u0065thod(arg) { return `${arg.member}:${`inner${this.#private}`}`; } }
lab\u0065l: for (;;) { break label; }
const {prop: binding} = imported;
"#;
    let found = text(source, Goal::Module, Limits::default()).expect("valid nested module");
    for (kind, value) in [
        (TextKind::String, "module"),
        (TextKind::Identifier, "source"),
        (TextKind::Identifier, "imported"),
        (TextKind::Identifier, "Class"),
        (TextKind::Identifier, "private"),
        (TextKind::Identifier, "method"),
        (TextKind::Identifier, "arg"),
        (TextKind::Identifier, "member"),
        (TextKind::Identifier, "label"),
        (TextKind::Identifier, "prop"),
        (TextKind::Identifier, "binding"),
        (TextKind::Template, ""),
        (TextKind::Template, ":"),
        (TextKind::Template, "inner"),
    ] {
        assert!(
            found.contains(&(kind, value.into())),
            "missing {kind:?}: {value}"
        );
    }
}

#[test]
fn strings_preserve_unicode_escapes_replacement_character_and_line_continuations() {
    let source = "['é😀', '\\u00e9\\uD83D\\uDE00', '\\u{1f600}', '\\x73ecret', 'ab\\\r\ncd', '\\uFFFD']; `one\\\ntwo\\nthree`;";
    let found =
        text(source.as_bytes(), Goal::Script, Limits::default()).expect("native cooked Unicode");
    assert_eq!(
        found,
        vec![
            (TextKind::String, "é😀".into()),
            (TextKind::String, "é😀".into()),
            (TextKind::String, "😀".into()),
            (TextKind::String, "secret".into()),
            (TextKind::String, "abcd".into()),
            (TextKind::String, "�".into()),
            (TextKind::Template, "onetwo\nthree".into()),
        ]
    );
    let identifiers = text(
        "let café=1; caf\\u00e9;".as_bytes(),
        Goal::Script,
        Limits::default(),
    )
    .expect("raw and escaped Unicode identifiers");
    assert_eq!(identifiers, vec![(TextKind::Identifier, "café".into()); 2]);
}

#[test]
fn inspection_does_not_evaluate_join_regex_or_transport_encodings() {
    let found = text(
        br#"/* hidden */ /secret/u; 'sec' + 'ret'; 'a%2fb'; 'c2VjcmV0'; 'use strict';"#,
        Goal::Script,
        Limits::default(),
    )
    .expect("syntax without evaluation");
    assert_eq!(
        found,
        vec![
            (TextKind::String, "sec".into()),
            (TextKind::String, "ret".into()),
            (TextKind::String, "a%2fb".into()),
            (TextKind::String, "c2VjcmV0".into()),
            (TextKind::String, "use strict".into()),
        ]
    );
    assert_eq!(
        text(b"'use strict';", Goal::Script, Limits::default()).expect("directive literal"),
        vec![(TextKind::String, "use strict".into())]
    );
}

#[test]
fn explicit_goals_apply_native_module_and_early_error_rules() {
    let module = b"export const value = 'ok';";
    assert!(text(module, Goal::Module, Limits::default()).is_ok());
    assert!(text(module, Goal::Unambiguous, Limits::default()).is_ok());
    assert_eq!(
        text(module, Goal::Script, Limits::default()),
        Err(Error::Malformed)
    );
    let sloppy = b"with (object) {}";
    assert!(text(sloppy, Goal::Script, Limits::default()).is_ok());
    assert!(text(sloppy, Goal::Unambiguous, Limits::default()).is_ok());
    assert_eq!(
        text(sloppy, Goal::Module, Limits::default()),
        Err(Error::Malformed)
    );
    assert_eq!(
        serde_json::to_string(&Goal::Unambiguous).expect("goal serializes"),
        "\"unambiguous\""
    );
}

#[test]
fn whole_input_syntax_and_early_errors_precede_callbacks() {
    for source in [
        "'visible'; const broken = ;",
        "let repeated; let repeated;",
        "'use strict'; with (object) {}",
        "export { missing };",
        "const value: string = 'typescript';",
        "const value = <jsx />;",
        "'unterminated",
        "`unterminated",
        "return 1;",
    ] {
        let result = javascript::inspect(
            source.as_bytes(),
            Goal::Module,
            Limits::default(),
            |_, _| -> Result<(), Infallible> {
                panic!("invalid input must not emit text: {source}")
            },
        );
        assert_eq!(result, Err(Error::Malformed), "{source}");
    }
}

#[test]
fn unsupported_lone_surrogates_and_invalid_tagged_cooked_values_never_succeed() {
    for source in [
        r#"'\uD800'"#,
        r#"'\uDC00'"#,
        r#"`\uD800`"#,
        r#"tag`\unicode`"#,
        r#"tag`\8`"#,
    ] {
        assert_eq!(
            text(source.as_bytes(), Goal::Script, Limits::default()),
            Err(Error::Unsupported),
            "{source}"
        );
    }
    assert_eq!(
        text(br#"`\unicode`"#, Goal::Script, Limits::default()),
        Err(Error::Malformed)
    );
}

#[test]
fn input_nodes_and_depth_are_independent_exact_bounds() {
    assert_eq!(
        text(
            &[0xff],
            Goal::Script,
            Limits {
                input_bytes: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("input bytes"))
    );
    assert_eq!(
        text(&[0xff], Goal::Script, Limits::default()),
        Err(Error::Utf8)
    );
    assert!(
        text(
            b"",
            Goal::Script,
            Limits {
                input_bytes: 0,
                nodes: 1,
                depth: 1,
                decoded_bytes: 0
            }
        )
        .expect("one empty Program root")
        .is_empty()
    );
    assert_eq!(
        text(
            b"",
            Goal::Script,
            Limits {
                nodes: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nodes"))
    );
    assert_eq!(
        text(
            b"",
            Goal::Script,
            Limits {
                depth: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nesting depth"))
    );
    // Program -> expression statement -> identifier reference: three native nodes.
    let exact = Limits {
        input_bytes: 2,
        nodes: 3,
        depth: 3,
        decoded_bytes: 1,
    };
    assert_eq!(
        text(b"x;", Goal::Script, exact).expect("exact three-node tree"),
        vec![(TextKind::Identifier, "x".into())]
    );
    for (limits, bound) in [
        (
            Limits {
                input_bytes: 1,
                ..exact
            },
            "input bytes",
        ),
        (Limits { nodes: 2, ..exact }, "nodes"),
        (Limits { depth: 2, ..exact }, "nesting depth"),
        (
            Limits {
                decoded_bytes: 0,
                ..exact
            },
            "decoded bytes",
        ),
    ] {
        assert_eq!(text(b"x;", Goal::Script, limits), Err(Error::Limit(bound)));
    }
    assert_eq!(
        text(
            b"{{{x;}}}",
            Goal::Script,
            Limits {
                depth: 3,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nesting depth"))
    );
}

#[test]
fn decoded_utf8_budget_counts_each_occurrence_before_delivery() {
    let source = "['é','é'];";
    assert_eq!(
        text(
            source.as_bytes(),
            Goal::Script,
            Limits {
                decoded_bytes: 4,
                ..Limits::default()
            }
        )
        .expect("four decoded UTF-8 bytes")
        .len(),
        2
    );
    let mut delivered = Vec::new();
    let result = javascript::inspect(
        source.as_bytes(),
        Goal::Script,
        Limits {
            decoded_bytes: 3,
            ..Limits::default()
        },
        |_, value| {
            delivered.push(value.to_owned());
            Ok::<_, Infallible>(())
        },
    );
    assert_eq!(result, Err(Error::Limit("decoded bytes")));
    assert_eq!(delivered, vec!["é"]);
}

#[test]
fn exact_consumer_error_is_preserved_but_not_disclosed() {
    let mut native = Some(std::io::Error::other("private consumer details"));
    let result = javascript::inspect(
        b"'first'; 'second';",
        Goal::Script,
        Limits::default(),
        |_, _| {
            Err(native
                .take()
                .expect("consumer invoked only once after its refusal"))
        },
    );
    let error = result.expect_err("consumer refusal stops inspection");
    assert_eq!(error.to_string(), "JavaScript text consumer rejected input");
    assert_eq!(
        error.source().expect("native consumer source").to_string(),
        "private consumer details"
    );
    let Error::Consumer(cause) = error else {
        panic!("exact consumer variant required")
    };
    assert_eq!(cause.kind(), std::io::ErrorKind::Other);
}
