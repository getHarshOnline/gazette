//! Code stays literal while known languages gain unstyled syntax spans.

use super::{Comrak, Error, Limits, Markdown, Profile, parse, plan};
use quick_xml::{Reader, events::Event};

#[test]
fn retained_syntax_notices_match_the_pinned_bundle() {
    assert_eq!(
        two_face::acknowledgement::listing().to_md(),
        include_str!("../../vendor/two-face-ACKNOWLEDGEMENTS.md")
    );
}

pub(super) fn code_text(html: &str) -> String {
    let mut reader = Reader::from_str(html);
    let mut inside = false;
    let mut encoded = String::new();
    loop {
        match reader.read_event().expect("balanced code HTML") {
            Event::Start(tag) if tag.name().as_ref() == b"code" => inside = true,
            Event::End(tag) if tag.name().as_ref() == b"code" => inside = false,
            Event::Text(text) if inside => encoded.push_str(&text.decode().unwrap()),
            Event::GeneralRef(reference) if inside => {
                encoded.push('&');
                encoded.push_str(&reference.decode().unwrap());
                encoded.push(';');
            }
            Event::Eof => break,
            _ => {}
        }
    }
    quick_xml::escape::unescape(&encoded).unwrap().into_owned()
}

#[test]
fn known_languages_keep_exact_code_and_reading_text_without_style() {
    for (language, code) in [
        (
            "typescript",
            "interface Greeting { text: string; }\nconst value: Greeting = { text: 'Café <tag> & value' };\n",
        ),
        (
            "toml",
            "# Configuration\n[publication]\ntitle = 'Café <tag> & value'\n",
        ),
        (
            "rust",
            "/* a comment\non two lines */\nfn main() { println!(\"Café 🧭 <tag> & value\"); }\n",
        ),
        ("python", "# Greeting\nprint(\"Café <tag> & value\")\n"),
        ("js", "const value = '<script>alert(1)</script> & café';\n"),
        (
            "json",
            "{\"meaning\": true, \"text\": \"<script> & café\"}\n",
        ),
        ("sh", "# Run locally\nprintf '%s\\n' '<tag> & café'\n"),
        ("markdown", "> [!IMPORTANT]\n> Fenced example.\n"),
    ] {
        let source = format!("```{language}\n{code}```\n");
        let document = parse(&source);
        let syntax = document.syntax().clone();
        let rendered = document.render(&plan(&syntax), None).unwrap();
        let html = &rendered.body.html;
        assert!(html.contains("<span class=\"syntax-"), "{language}: {html}");
        assert!(html.contains(&format!("class=\"language-{language}\"")));
        assert_eq!(code_text(html), code, "{language}");
        assert_eq!(rendered.body.text, format!("{code}\n"));
        assert!(!html.contains("style="));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("markdown-alert"));
        assert_eq!(document.syntax(), &syntax);
        let math = Comrak
            .parse(&source, Profile::PublicationMath, Limits::default())
            .unwrap();
        assert_eq!(math.render(&plan(math.syntax()), None).unwrap(), rendered);
    }
}

#[test]
fn unknown_and_unlabelled_code_does_not_guess_or_interpret_markup() {
    let code = "#!/usr/bin/python\nprint('<script>&amp; café</script>')\n";
    for language in [
        "",
        "not-a-language",
        "text",
        "plaintext",
        "x\"onclick=\"bad",
    ] {
        let source = format!("```{language}\n{code}```\n");
        let document = parse(&source);
        let html = document
            .render(&plan(document.syntax()), None)
            .unwrap()
            .body
            .html;
        assert_eq!(
            html,
            comrak::markdown_to_html(&source, &comrak::Options::default()),
            "{language}"
        );
        assert_eq!(code_text(&html), code);
    }
    let source = "    <script> & literal\n";
    let document = parse(source);
    assert_eq!(
        document
            .render(&plan(document.syntax()), None)
            .unwrap()
            .body
            .html,
        comrak::markdown_to_html(source, &comrak::Options::default())
    );
}

#[test]
fn highlighted_output_obeys_the_existing_byte_limit() {
    let source = "```rust\nlet value = 1;\n```\n";
    let document = parse(source);
    let complete = document.render(&plan(document.syntax()), None).unwrap();
    for limit in [80, complete.body.html.len() - 1] {
        let document = Comrak
            .parse(
                source,
                Profile::Publication,
                Limits {
                    output_bytes: limit,
                    ..Limits::default()
                },
            )
            .unwrap();
        assert!(matches!(
            document.render(&plan(document.syntax()), None),
            Err(Error::Limit("rendered output bytes"))
        ));
    }
}

#[test]
fn direct_code_uses_the_same_highlighter_with_exact_bytes_and_bounds() {
    let code = "print('<script> & café')\n\nvalue = 42";
    for language in ["python", "unknown-language"] {
        let html = Comrak
            .highlight_code(code, language, Limits::default())
            .unwrap()
            .unwrap();
        assert_eq!(code_text(&format!("<code>{html}</code>")), code);
        assert_eq!(html.contains("syntax-"), language == "python");
        assert!(!html.contains("<script>"));
        for limits in [
            Limits {
                output_bytes: html.len() - 1,
                ..Limits::default()
            },
            Limits {
                source_bytes: code.len() - 1,
                ..Limits::default()
            },
        ] {
            assert!(matches!(
                Comrak.highlight_code(code, language, limits),
                Err(Error::Limit(_))
            ));
        }
    }
}
