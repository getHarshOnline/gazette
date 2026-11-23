use std::{error::Error as _, fmt};

/// Gazette-owned location wrapper around the renderer's complete native cause.
#[derive(Debug)]
pub(crate) struct TemplateDiagnostic {
    template: String,
    location: Option<(usize, usize)>,
    reason: String,
    source: Option<tera::Error>,
}

impl TemplateDiagnostic {
    pub(crate) fn compile(template: &str, source: &str, error: tera::Error) -> Self {
        let reason = deepest_reason(&error);
        let (line, column) =
            parse_position(&reason).unwrap_or_else(|| locate_reason(source, &reason));
        Self {
            template: template.to_owned(),
            location: Some((line, column)),
            reason,
            source: Some(error),
        }
    }

    pub(crate) fn render(
        requested_template: &str,
        sources: &std::collections::BTreeMap<String, String>,
        error: tera::Error,
    ) -> Self {
        let reason = deepest_reason(&error);
        let template = referenced_template(&reason)
            .filter(|name| sources.contains_key(*name))
            .unwrap_or(requested_template);
        let (line, column) = sources
            .get(template)
            .map(|source| locate_reason(source, &reason))
            .unwrap_or((1, 1));
        Self {
            template: template.to_owned(),
            location: Some((line, column)),
            reason,
            source: Some(error),
        }
    }

    // Tera's parsed AST has no source spans. A safety-admission cause names its
    // retained declaration, not a fabricated runtime call-site location.
    pub(crate) fn admission(template: &str, reason: &str) -> Self {
        Self {
            template: template.to_owned(),
            location: None,
            reason: reason.to_owned(),
            source: None,
        }
    }
}

impl fmt::Display for TemplateDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((line, column)) = self.location {
            write!(
                formatter,
                "template '{}' at line {line}, column {column}: {}",
                self.template, self.reason
            )
        } else {
            write!(
                formatter,
                "template '{}' admission: {}",
                self.template, self.reason
            )
        }
    }
}

impl std::error::Error for TemplateDiagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

fn deepest_reason(error: &tera::Error) -> String {
    let mut reason = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        reason = cause.to_string();
        source = cause.source();
    }
    reason
}

fn referenced_template(reason: &str) -> Option<&str> {
    let marker = "while rendering '";
    let start = reason.rfind(marker)? + marker.len();
    let tail = &reason[start..];
    Some(&tail[..tail.find('\'')?])
}

fn locate_reason(source: &str, reason: &str) -> (usize, usize) {
    variable_name(reason)
        .and_then(|name| locate(source, name))
        .or_else(|| parse_position(reason))
        .unwrap_or((1, 1))
}

fn variable_name(reason: &str) -> Option<&str> {
    let marker = "Variable `";
    let start = reason.find(marker)? + marker.len();
    let tail = &reason[start..];
    Some(&tail[..tail.find('`')?])
}

fn locate(source: &str, needle: &str) -> Option<(usize, usize)> {
    let offset = source.find(needle)?;
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = source[line_start..offset].chars().count() + 1;
    Some((line, column))
}

fn parse_position(message: &str) -> Option<(usize, usize)> {
    for token in message.split_whitespace() {
        let token =
            token.trim_matches(|character: char| !character.is_ascii_digit() && character != ':');
        if let Some((line, column)) = token.split_once(':')
            && let (Ok(line), Ok(column)) = (line.parse(), column.parse())
        {
            return Some((line, column));
        }
    }
    None
}
