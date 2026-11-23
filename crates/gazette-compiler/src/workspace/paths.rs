//! Lexical native-location rules; existence, aliases and write confinement are later checks.

pub(super) fn absolute_directory(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
}
