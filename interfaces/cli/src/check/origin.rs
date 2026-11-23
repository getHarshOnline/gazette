//! Locate a configured value in the selected workspace and its included documents.

use super::*;

pub(super) fn run(args: &Args) -> ExitCode {
    let location = match locate_workspace(args) {
        Ok(location) => location,
        Err(error) => return failure("workspace_location", &error),
    };
    let limits = workspace_limits(args);
    let loaded = match configuration::load_workspace(&location.root, &location.workspace, limits) {
        Ok(loaded) => loaded,
        Err(error) => return failure(error.code(), &error),
    };
    let sources = loaded.documents().documents.iter().map(|document| {
        (
            &document.path,
            loaded
                .source(&document.path)
                .expect("loaded document bytes"),
        )
    });
    let parsed = match workspace::ParsedWorkspaceDocuments::parse(
        &location.workspace,
        sources,
        limits.syntax,
    ) {
        Ok(parsed) => parsed,
        Err(error) => return failure(error.code(), &error),
    };
    let result = if let Some(query) = &args.origin {
        let query = match serde_json::from_str(query) {
            Ok(query) => query,
            Err(error) => return failure("workspace_origin_query", &error),
        };
        parsed.origin(&query).map(serde_json::to_value)
    } else {
        let query =
            match serde_json::from_str(args.composition_origin.as_deref().expect("origin mode")) {
                Ok(query) => query,
                Err(error) => return failure("workspace_origin_query", &error),
            };
        parsed.composition_origin(&query).map(serde_json::to_value)
    };
    match result {
        Ok(Ok(value)) => write_located(&location, &value),
        Ok(Err(error)) => failure("workspace_report", &error),
        Err(error) => failure("workspace_origin", &error),
    }
}
