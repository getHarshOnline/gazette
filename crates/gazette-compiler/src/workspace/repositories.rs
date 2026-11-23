//! Explicit public addresses; private Git delivery coordinates never enter this path.

pub(super) fn valid(value: &str) -> bool {
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default());
    !value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '\\')
        && authority.is_some_and(|value| !value.is_empty() && !value.contains('@'))
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "https" | "http")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}
