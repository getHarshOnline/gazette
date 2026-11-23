//! Syntax classes for code, with escaped plain text for unrecognized languages.

use comrak::adapters::SyntaxHighlighterAdapter;
use std::{borrow::Cow, collections::HashMap, fmt, sync::LazyLock};
use syntect::{
    html::{ClassStyle, line_tokens_to_classed_spans},
    parsing::{ParseState, ScopeStack, SyntaxSet},
    util::LinesWithEndings,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

pub(super) struct Highlight;

impl SyntaxHighlighterAdapter for Highlight {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        language: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let Some(syntax) = language
            .filter(|value| !value.is_empty())
            .and_then(|value| SYNTAXES.find_syntax_by_token(value))
        else {
            return comrak::html::escape(output, code);
        };
        let mut parser = ParseState::new(syntax);
        let mut scopes = ScopeStack::new();
        let mut open_spans = 0isize;
        for line in LinesWithEndings::from(code) {
            let tokens = parser.parse_line(line, &SYNTAXES).map_err(|_| fmt::Error)?;
            let (html, delta) = line_tokens_to_classed_spans(
                line,
                &tokens,
                ClassStyle::SpacedPrefixed { prefix: "syntax-" },
                &mut scopes,
            )
            .map_err(|_| fmt::Error)?;
            open_spans = open_spans
                .checked_add(delta)
                .filter(|n| *n >= 0)
                .ok_or(fmt::Error)?;
            output.write_str(&html)?;
        }
        for _ in 0..open_spans {
            output.write_str("</span>")?;
        }
        Ok(())
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        comrak::html::write_opening_tag(output, "pre", attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        comrak::html::write_opening_tag(output, "code", attributes)
    }
}
