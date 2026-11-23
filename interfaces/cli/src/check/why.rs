//! Read publication values and their winning declarations without building a site.

use super::*;

pub(super) fn run(args: &Args) -> ExitCode {
    let field = args.why.as_deref().expect("why operation");
    if field.is_empty() || field.split('.').any(str::is_empty) {
        return failure(
            "workspace_origin_query",
            &"use a field path such as contacts.rights",
        );
    }
    let query = workspace::PublicationOriginQuery {
        site: args.site.clone().expect("site required by clap"),
        path: field
            .split('.')
            .map(|part| toml::PathSegment::Key(part.into()))
            .collect(),
    };
    let limits = toml::Limits {
        source_bytes: args.max_source_bytes,
        nodes: args.max_nodes,
        depth: args.max_depth,
    };
    let mut supplied = Vec::new();
    if args.stdin
        && let Err(error) = io::stdin()
            .lock()
            .take((limits.source_bytes as u64).saturating_add(1))
            .read_to_end(&mut supplied)
    {
        return failure("workspace_input", &error);
    }
    if supplied.len() > limits.source_bytes {
        return failure("toml_limit", &"TOML source bytes ceiling exceeded");
    }
    if args.stdin {
        if args.workspace_root.is_some() {
            return failure(
                "workspace_input",
                &"select a local workspace or supply TOML, not both",
            );
        }
        let source = match std::str::from_utf8(&supplied) {
            Ok(source) => source,
            Err(error) => return failure("workspace_utf8", &error),
        };
        let parsed = match workspace::ParsedWorkspace::parse(source, limits) {
            Ok(parsed) => parsed,
            Err(error) => return failure(error.code(), &error),
        };
        return report(parsed.publication_origin(&query));
    }
    let location = match locate_workspace(args) {
        Ok(location) => location,
        Err(error) => return failure("workspace_location", &error),
    };
    let load_limits = workspace_limits(args);
    let loaded =
        match configuration::load_workspace(&location.root, &location.workspace, load_limits) {
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
        load_limits.syntax,
    ) {
        Ok(parsed) => parsed,
        Err(error) => return failure(error.code(), &error),
    };
    match parsed.publication_origin(&query) {
        Ok(origin) => write_located(&location, &origin),
        Err(error) => failure("workspace_origin", &error),
    }
}

fn report(result: Result<workspace::PublicationFieldOrigin, workspace::OriginError>) -> ExitCode {
    match result {
        Ok(origin) => crate::write_json(&origin),
        Err(error) => failure("workspace_origin", &error),
    }
}
