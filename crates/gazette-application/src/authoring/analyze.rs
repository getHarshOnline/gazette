//! Analyze one current selection through the shared workspace and content compilers.

use super::*;

// Retain iterator-based input so each item is checked before requesting the next.
pub(super) struct AnalysisCorpus<D, M, P> {
    pub(super) documents: D,
    pub(super) manuals: M,
    pub(super) page_inputs: P,
    pub(super) asset_metadata: BTreeMap<EntryPath, u64>,
}

pub(super) fn analyze_selected<'s>(
    workspace_input: WorkspaceInput<'s>,
    corpus: AnalysisCorpus<
        impl IntoIterator<Item = Input<'s>>,
        impl IntoIterator<Item = ManualBaseline<'s>>,
        impl IntoIterator<Item = Input<'s>>,
    >,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
    markdown: &dyn Markdown,
) -> Result<Analysis, Error> {
    let AnalysisCorpus {
        documents,
        manuals,
        page_inputs,
        asset_metadata,
    } = corpus;
    let (workspace, workspace_sources, modular) = match workspace_input {
        WorkspaceInput::Buffer(input) => {
            let root = input.path.clone();
            (root, BTreeMap::from([(input.path, input.bytes)]), false)
        }
        WorkspaceInput::Documents { root, documents } => {
            let mut sources = BTreeMap::new();
            for input in documents {
                if sources.insert(input.path.clone(), input.bytes).is_some() {
                    return Err(Error::Selection(corpus::Error::Duplicate(input.path)));
                }
            }
            if !sources.contains_key(&root) {
                return Err(Error::InvalidSource);
            }
            (root, sources, true)
        }
    };
    let mut selected = BTreeMap::new();
    let mut manual_paths = BTreeSet::new();
    let mut retained_manuals = BTreeMap::new();
    let mut page_input_paths = BTreeSet::new();
    let mut package_inputs = BTreeMap::new();
    let mut selected_paths: BTreeSet<_> = workspace_sources.keys().cloned().collect();
    let mut bytes = 0usize;
    let mut paths = workspace_sources
        .keys()
        .map(|path| path.as_str().len())
        .sum();
    if paths > limits.corpus.path_bytes {
        return Err(Error::Limit("input path bytes"));
    }
    for (
        Input {
            path,
            bytes: source,
        },
        page_input,
    ) in documents
        .into_iter()
        .map(|input| (input, false))
        .chain(page_inputs.into_iter().map(|input| (input, true)))
    {
        if !selected_paths.insert(path.clone()) {
            return Err(Error::Selection(corpus::Error::Duplicate(path)));
        }
        if selected.len() >= limits.corpus.files {
            return Err(Error::Limit("document inputs"));
        }
        charge(
            &mut bytes,
            source.len(),
            limits.corpus.source_bytes,
            "document bytes",
        )?;
        charge(
            &mut paths,
            path.as_str().len(),
            limits.corpus.path_bytes,
            "input path bytes",
        )?;
        if page_input {
            page_input_paths.insert(path.clone());
        }
        selected.insert(path, source);
    }
    for manual in manuals {
        let binding = ManualBinding {
            source: manual.source,
            path: manual.path,
        };
        if retained_manuals.contains_key(&binding) {
            return Err(Error::ManualSelection(
                "duplicate configured source and capture-relative path",
            ));
        }
        charge(
            &mut bytes,
            manual.bytes.len(),
            limits.corpus.source_bytes,
            "document bytes",
        )?;
        charge(
            &mut paths,
            binding
                .source
                .as_str()
                .len()
                .checked_add(binding.path.as_str().len())
                .ok_or(Error::Limit("input identity bytes"))?,
            limits.corpus.path_bytes,
            "input identity bytes",
        )?;
        if selected.len() >= limits.corpus.files {
            return Err(Error::Limit("document inputs"));
        }
        let analysis_path = qualified_manual_path(&binding)?;
        if !selected_paths.insert(analysis_path.clone()) {
            return Err(Error::ManualSelection(
                "qualified manual analysis path collides with another selected input",
            ));
        }
        manual_paths.insert(analysis_path.clone());
        selected.insert(analysis_path.clone(), manual.bytes);
        retained_manuals.insert(
            binding,
            RetainedManual {
                analysis_path,
                source_sha256: Sha256Digest::of(manual.bytes),
                bytes: manual.bytes.to_vec(),
            },
        );
    }
    for input in package_baselines {
        if package_inputs.contains_key(&input.package) {
            return Err(Error::PackageSelection("duplicate package baseline"));
        }
        if selected.len() + package_inputs.len() >= limits.corpus.files {
            return Err(Error::Limit("selected inputs"));
        }
        charge(
            &mut bytes,
            input.bytes.len(),
            limits.corpus.source_bytes,
            "selected source bytes",
        )?;
        charge(
            &mut paths,
            input.package.as_str().len(),
            limits.corpus.path_bytes,
            "input identity bytes",
        )?;
        package_inputs.insert(input.package, input.bytes);
    }
    for path in asset_metadata.keys() {
        if !selected_paths.insert(path.clone()) {
            return Err(Error::Selection(corpus::Error::Duplicate(path.clone())));
        }
        if selected
            .len()
            .saturating_add(package_inputs.len())
            .saturating_add(asset_metadata.len())
            > limits.corpus.files
        {
            return Err(Error::Limit("selected inputs"));
        }
        charge(
            &mut paths,
            path.as_str().len(),
            limits.corpus.path_bytes,
            "input path bytes",
        )?;
    }
    let mut inputs: BTreeMap<_, _> = selected
        .iter()
        .map(|(path, bytes)| (path.clone(), Sha256Digest::of(bytes)))
        .collect();
    inputs.extend(
        workspace_sources
            .iter()
            .map(|(path, bytes)| (path.clone(), Sha256Digest::of(bytes))),
    );
    let workspace_authoring = workspace_sources
        .iter()
        .map(|(path, bytes)| {
            (
                path.clone(),
                std::str::from_utf8(bytes)
                    .map_err(|_| workspace::AuthoringError::Invalid)
                    .and_then(|source| {
                        workspace::WorkspaceAuthoring::parse(source, limits.workspace)
                    }),
            )
        })
        .collect();
    let mut analysis = Analysis {
        workspace,
        workspace_documents: workspace_sources.keys().cloned().collect(),
        manuals: retained_manuals,
        manual_paths,
        page_inputs: page_input_paths,
        asset_metadata: asset_metadata.clone(),
        inputs,
        page_packages: BTreeMap::new(),
        page_resources: BTreeMap::new(),
        package_baselines: package_inputs
            .iter()
            .map(|(package, bytes)| (package.clone(), Sha256Digest::of(bytes)))
            .collect(),
        failures: Vec::new(),
        headings: BTreeMap::new(),
        footnotes: BTreeMap::new(),
        heading_links: BTreeMap::new(),
        heading_destinations: BTreeMap::new(),
        symbolic_destinations: BTreeMap::new(),
        symbolic_targets: None,
        workspace_reference_targets: None,
        related_reference_targets: None,
        document_links: Ok(Vec::new()),
        editorial_links: Vec::new(),
        editorial_authoring: None,
        workspace_navigation: None,
        workspace_authoring,
        authored_authoring: BTreeMap::new(),
        components: BTreeMap::new(),
        landing_components: BTreeMap::new(),
        workspace_compilation: None,
    };
    let mut navigation_count = 0;
    let mut navigation_bytes = 0;
    analysis.workspace_compilation = if modular {
        match workspace::inspect_documents(
            &analysis.workspace,
            workspace_sources.iter().map(|(path, bytes)| (path, *bytes)),
            limits.workspace_documents,
        ) {
            Ok((compilation, navigation)) => {
                let compilation = match (compilation, navigation) {
                    (compilation, Ok(navigation)) => {
                        charge(
                            &mut navigation_count,
                            navigation.occurrence_count(),
                            limits.navigation_occurrences,
                            "navigation occurrences",
                        )?;
                        charge(
                            &mut navigation_bytes,
                            navigation.key_bytes(),
                            limits.navigation_bytes,
                            "navigation bytes",
                        )?;
                        analysis.workspace_navigation =
                            Some(WorkspaceNavigation::Documents(navigation));
                        compilation
                    }
                    (Err(error), Err(_)) | (Ok(_), Err(error)) => Err(error),
                };
                match compilation {
                    Ok(compilation) => Some(compilation),
                    Err(error) => {
                        analysis.failures.push(Failure::Workspace(error));
                        None
                    }
                }
            }
            Err(error) => {
                analysis.failures.push(Failure::Workspace(error));
                None
            }
        }
    } else {
        let bytes = workspace_sources
            .get(&analysis.workspace)
            .ok_or(Error::InvalidSource)?;
        if bytes.len() > limits.workspace.source_bytes {
            return Err(Error::Limit("workspace bytes"));
        }
        match std::str::from_utf8(bytes) {
            Ok(source) => match workspace::WorkspaceInspection::parse(source, limits.workspace) {
                Ok(parsed) => {
                    let (compilation, navigation) = parsed.into_parts();
                    let compilation = match (compilation, navigation) {
                        (compilation, Ok(navigation)) => {
                            charge(
                                &mut navigation_count,
                                navigation.occurrence_count(),
                                limits.navigation_occurrences,
                                "navigation occurrences",
                            )?;
                            charge(
                                &mut navigation_bytes,
                                navigation.key_bytes(),
                                limits.navigation_bytes,
                                "navigation bytes",
                            )?;
                            analysis.workspace_navigation =
                                Some(WorkspaceNavigation::Buffer(navigation));
                            compilation
                        }
                        (Err(error), Err(_)) | (Ok(_), Err(error)) => Err(error),
                    };
                    match compilation {
                        Ok(compilation) => Some(compilation),
                        Err(error) => {
                            analysis.failures.push(Failure::Workspace(error));
                            None
                        }
                    }
                }
                Err(error) => {
                    analysis.failures.push(Failure::Workspace(error));
                    None
                }
            },
            Err(error) => {
                analysis.failures.push(Failure::WorkspaceEncoding(error));
                None
            }
        }
    };
    analysis.authored_authoring = selected
        .iter()
        .filter(|(path, _)| !analysis.manual_paths.contains(*path))
        .filter(|(path, _)| !analysis.page_inputs.contains(*path))
        .filter_map(|(path, bytes)| {
            std::str::from_utf8(bytes).ok().map(|source| {
                let legal = analysis
                    .workspace_compilation
                    .as_ref()
                    .is_some_and(|compilation| {
                        compilation.workspace.legal_bindings(path).next().is_some()
                    });
                let profile = if legal {
                    authored::AuthoredSchema::LegalSource
                } else {
                    authored::AuthoredSchema::Page
                };
                (
                    path.clone(),
                    authored::AuthoredAuthoring::parse_with_schema(
                        source,
                        limits.corpus.document,
                        profile,
                    ),
                )
            })
        })
        .collect();
    analysis.workspace_reference_targets = authored_completion::retain_workspace_targets(
        &analysis,
        &workspace_sources,
        limits,
        &mut navigation_count,
        &mut navigation_bytes,
    )?;
    let mut theme_manifests = BTreeMap::new();
    if let Some(compilation) = analysis.workspace_compilation.as_ref() {
        for (package, bytes) in &package_inputs {
            let binding = match package {
                PackageBaselineKey::Theme(package) => compilation
                    .workspace
                    .theme_package(package)
                    .map(|binding| (&binding.source, &binding.manifest)),
                PackageBaselineKey::Plugin(package) => compilation
                    .workspace
                    .plugin_package(package)
                    .map(|binding| (&binding.source, &binding.manifest)),
            };
            let Some((source, manifest)) = binding else {
                analysis.failures.push(Failure::Package {
                    package: package.clone(),
                    source: None,
                    manifest: None,
                    error: PackageFailure::Binding(
                        "package baseline is not declared by the workspace",
                    ),
                });
                continue;
            };
            let source_text = match std::str::from_utf8(bytes) {
                Ok(source) => source,
                Err(_) => {
                    analysis.failures.push(Failure::Package {
                        package: package.clone(),
                        source: Some(source.clone()),
                        manifest: Some(manifest.clone()),
                        error: PackageFailure::Encoding,
                    });
                    continue;
                }
            };
            match package {
                PackageBaselineKey::Theme(package) => {
                    match theme_package::ParsedManifest::parse(source_text, limits.workspace) {
                        Ok(parsed) if &parsed.manifest().package == package => {
                            theme_manifests.insert(package.clone(), parsed);
                        }
                        Ok(_) => analysis.failures.push(Failure::Package {
                            package: PackageBaselineKey::Theme(package.clone()),
                            source: Some(source.clone()),
                            manifest: Some(manifest.clone()),
                            error: PackageFailure::Binding(
                                "Theme manifest identity does not match its workspace binding",
                            ),
                        }),
                        Err(error) => analysis.failures.push(Failure::Package {
                            package: PackageBaselineKey::Theme(package.clone()),
                            source: Some(source.clone()),
                            manifest: Some(manifest.clone()),
                            error: PackageFailure::ThemeManifest(error),
                        }),
                    }
                }
                PackageBaselineKey::Plugin(package) => {
                    match plugin::ParsedManifest::parse(source_text, limits.workspace) {
                        Ok(parsed) if &parsed.manifest().package == package => {}
                        Ok(_) => analysis.failures.push(Failure::Package {
                            package: PackageBaselineKey::Plugin(package.clone()),
                            source: Some(source.clone()),
                            manifest: Some(manifest.clone()),
                            error: PackageFailure::Binding(
                                "plugin manifest identity does not match its workspace binding",
                            ),
                        }),
                        Err(error) => analysis.failures.push(Failure::Package {
                            package: PackageBaselineKey::Plugin(package.clone()),
                            source: Some(source.clone()),
                            manifest: Some(manifest.clone()),
                            error: PackageFailure::PluginManifest(error),
                        }),
                    }
                }
            }
        }
    }
    let profiles = selected
        .keys()
        .map(|path| {
            let profile = if analysis.manual_paths.contains(path) {
                MANUAL_PROFILE
            } else {
                analysis
                    .workspace_compilation
                    .as_ref()
                    .map(|compilation| compilation.workspace.markdown_profile(path))
                    .unwrap_or_default()
            };
            (path.clone(), profile)
        })
        .collect();
    let corpus = match corpus::ParsedCorpus::parse_with_profiles(
        selected.iter().map(|(path, bytes)| Input {
            path: path.clone(),
            bytes,
        }),
        &profiles,
        limits.corpus,
        markdown,
    ) {
        Ok(corpus) => corpus,
        Err(error) => {
            analysis.failures.push(Failure::Corpus(error));
            return Ok(analysis);
        }
    };
    let component_roots: BTreeSet<_> = analysis
        .workspace_compilation
        .as_ref()
        .into_iter()
        .flat_map(|compilation| {
            corpus.files().iter().filter_map(|(path, file)| {
                if analysis.manual_paths.contains(path) {
                    return None;
                }
                let body = file.body()?;
                if body.directives().is_empty() {
                    return None;
                }
                let content = compilation.workspace.content_source(path)?;
                compilation
                    .workspace
                    .composition_for(content.site)?
                    .theme()
                    .map(|selected| selected.value().package.clone())
            })
        })
        .collect();
    let mut effective_components = BTreeMap::new();
    let mut graph_failures = Vec::new();
    if let Some(compilation) = analysis.workspace_compilation.as_ref() {
        for root in component_roots {
            match theme_graph::Graph::resolve(
                &root,
                theme_manifests.values().map(|parsed| parsed.manifest()),
            )
            .and_then(|graph| theme_graph::EffectiveTheme::resolve(&graph))
            {
                Ok(theme) => {
                    effective_components.insert(root, Arc::clone(theme.components()));
                }
                Err(error) => {
                    let binding = compilation.workspace.theme_package(&root);
                    graph_failures.push(Failure::Package {
                        package: PackageBaselineKey::Theme(root),
                        source: binding.map(|binding| binding.source.clone()),
                        manifest: binding.map(|binding| binding.manifest.clone()),
                        error: PackageFailure::ThemeGraph(error),
                    });
                }
            }
        }
    }
    analysis.failures.extend(graph_failures);
    let mut heading_count = 0;
    let mut heading_bytes = 0;
    for (path, file) in corpus.files() {
        let Some(body) = file.body() else { continue };
        let source = std::str::from_utf8(selected[path]).map_err(|_| Error::InvalidSource)?;
        let definitions = analysis
            .workspace_compilation
            .as_ref()
            .and_then(|compilation| {
                let content = compilation.workspace.content_source(path)?;
                let selected = compilation
                    .workspace
                    .composition_for(content.site)?
                    .theme()?
                    .value();
                effective_components.get(&selected.package).cloned()
            });
        // Only original YAML section locations can offer exact edits. The
        // normalized Markdown body has a different coordinate space.
        if let corpus::File::Landing(landing) = file {
            if let Some(definitions) = definitions {
                let slots = landing.component_slots();
                if !slots.is_empty() {
                    analysis.landing_components.insert(
                        path.clone(),
                        LandingComponentContext {
                            slots,
                            definitions: Arc::clone(&definitions),
                        },
                    );
                }
                if let Err(error) = definitions.bind(body.directives(), limits.workspace) {
                    let original = error
                        .location()
                        .and_then(|location| landing.original_location(location));
                    analysis.failures.push(Failure::Components {
                        path: path.clone(),
                        coordinates: ComponentCoordinates::Landing(original),
                        error,
                    });
                }
            }
            continue;
        }
        let start = file.body_offset().ok_or(Error::InvalidSource)?;
        let body_span = Span::checked(source, start..source.len()).ok_or(Error::InvalidSource)?;
        let directives = body.directives().to_vec();
        // Component binding depends on the current resolved workspace. Its root
        // failure is already retained; do not blame every otherwise valid page
        // for an effective Theme which cannot yet be selected.
        if analysis.workspace_compilation.is_some()
            && !analysis.manual_paths.contains(path)
            && !directives.is_empty()
        {
            let result = match definitions.as_ref() {
                Some(definitions) => definitions.bind(&directives, limits.workspace),
                None => Err(markdown::Error::Adapter(
                    gazette_core::markdown::Error::Unsupported {
                        location: directives[0].location,
                        reason: "components require complete package baselines for the effective selected Theme graph",
                    },
                )),
            };
            if let Err(error) = result {
                analysis.failures.push(Failure::Components {
                    path: path.clone(),
                    coordinates: ComponentCoordinates::Markdown(body_span),
                    error,
                });
            }
            analysis.components.insert(
                path.clone(),
                ComponentContext {
                    body_span,
                    directives,
                    definitions,
                    authoring_limits: limits.workspace,
                },
            );
        }
        analysis.footnotes.insert(
            path.clone(),
            navigation::retain(
                body,
                source,
                body_span,
                limits,
                &mut navigation_count,
                &mut navigation_bytes,
            )?,
        );
        let anchor_policy = if analysis.manual_paths.contains(path) {
            markdown::AnchorPolicy::Manual
        } else {
            markdown::AnchorPolicy::Publication
        };
        let renderability = body.check_source_rendering_with_policy(anchor_policy).err();
        let headings = match body.headings_with_policy(anchor_policy) {
            Ok(headings) => headings,
            Err(error) => {
                let same_failure = renderability.as_ref() == Some(&error);
                analysis.failures.push(Failure::Headings {
                    path: path.clone(),
                    body_span,
                    error,
                });
                if let Some(error) = renderability
                    && !same_failure
                {
                    analysis.failures.push(Failure::Renderability {
                        path: path.clone(),
                        body_span,
                        error,
                    });
                }
                continue;
            }
        };
        charge(
            &mut heading_count,
            headings.len(),
            limits.headings,
            "headings",
        )?;
        for heading in headings {
            for value in [&heading.text, &heading.id, &heading.fragment] {
                charge(
                    &mut heading_bytes,
                    value.len(),
                    limits.heading_bytes,
                    "heading bytes",
                )?;
            }
            heading
                .span
                .in_parent(source, body_span)
                .ok_or(Error::InvalidSource)?;
        }
        analysis.headings.insert(
            path.clone(),
            Headings {
                body_span,
                values: headings.to_vec(),
            },
        );
        if let Some(error) = renderability {
            analysis.failures.push(Failure::Renderability {
                path: path.clone(),
                body_span,
                error,
            });
        }
        let destinations = body
            .heading_completion_slots()
            .map_err(|error| match error {
                markdown::Error::Limit(resource) => Error::Limit(resource),
                _ => Error::InvalidSource,
            })?;
        charge(
            &mut navigation_count,
            destinations.len(),
            limits.navigation_occurrences,
            "navigation occurrences",
        )?;
        let destinations = destinations
            .into_iter()
            .map(|span| {
                span.in_parent(source, body_span)
                    .ok_or(Error::InvalidSource)
            })
            .collect::<Result<Vec<_>, _>>()?;
        analysis
            .heading_destinations
            .insert(path.clone(), destinations);
        let destinations = body
            .symbolic_completion_slots()
            .map_err(|error| match error {
                markdown::Error::Limit(resource) => Error::Limit(resource),
                _ => Error::InvalidSource,
            })?;
        charge(
            &mut navigation_count,
            destinations.len(),
            limits.navigation_occurrences,
            "navigation occurrences",
        )?;
        let destinations = destinations
            .into_iter()
            .map(|span| {
                span.in_parent(source, body_span)
                    .ok_or(Error::InvalidSource)
            })
            .collect::<Result<Vec<_>, _>>()?;
        analysis
            .symbolic_destinations
            .insert(path.clone(), destinations);
        let references = body.heading_references_with_policy(anchor_policy);
        if let Err(error) = &references {
            analysis.failures.push(Failure::Renderability {
                path: path.clone(),
                body_span,
                error: error.clone(),
            });
        }
        if let Ok(references) = &references {
            charge(
                &mut navigation_count,
                references.len(),
                limits.navigation_occurrences,
                "navigation occurrences",
            )?;
            for reference in references {
                reference
                    .location
                    .span
                    .in_parent(source, body_span)
                    .ok_or(Error::InvalidSource)?;
                if reference.heading >= headings.len() {
                    return Err(Error::InvalidSource);
                }
            }
        }
        analysis.heading_links.insert(path.clone(), references);
    }
    analysis.document_links = match documents::retain(
        &corpus,
        &selected,
        &analysis.manual_paths,
        limits,
        &mut navigation_count,
        &mut navigation_bytes,
    )? {
        Ok(links) => Ok(links),
        Err(error) => {
            analysis.failures.push(Failure::DocumentReferences(error));
            Err(())
        }
    };
    if let Some(workspace) = &analysis.workspace_compilation {
        let authored_profiles = profiles
            .iter()
            .filter(|(path, _)| !analysis.manual_paths.contains(*path))
            .map(|(path, profile)| (path.clone(), *profile))
            .collect();
        let authored_corpus = match corpus::ParsedCorpus::parse_with_profiles(
            selected
                .iter()
                .filter(|(path, _)| !analysis.manual_paths.contains(*path))
                .map(|(path, bytes)| Input {
                    path: path.clone(),
                    bytes,
                }),
            &authored_profiles,
            limits.corpus,
            markdown,
        ) {
            Ok(corpus) => corpus,
            Err(error) => {
                analysis.failures.push(Failure::Corpus(error));
                return Ok(analysis);
            }
        };
        let editorial = authored::editorial_authoring(
            workspace,
            &authored_corpus,
            limits
                .navigation_occurrences
                .saturating_sub(navigation_count),
            limits.navigation_bytes.saturating_sub(navigation_bytes),
        );
        match &editorial {
            Ok(facts) => {
                charge(
                    &mut navigation_count,
                    facts.occurrence_count(),
                    limits.navigation_occurrences,
                    "navigation occurrences",
                )?;
                charge(
                    &mut navigation_bytes,
                    facts.fact_bytes(),
                    limits.navigation_bytes,
                    "navigation bytes",
                )?;
            }
            Err(authored::ProjectionError::Limit(resource)) => return Err(Error::Limit(resource)),
            Err(_) => {}
        }
        analysis.editorial_authoring = Some(editorial);
        match authored::resolve_with_asset_metadata(
            workspace,
            &authored_corpus,
            asset_metadata,
            limits.content,
        ) {
            Ok(resolved) => {
                let targets = resolved
                    .authored_reference_targets(
                        limits
                            .navigation_occurrences
                            .saturating_sub(navigation_count),
                        limits.navigation_bytes.saturating_sub(navigation_bytes),
                    )
                    .map_err(|error| match error {
                        authored::ProjectionError::Limit(resource) => Error::Limit(resource),
                        _ => Error::InvalidSource,
                    })?;
                charge(
                    &mut navigation_count,
                    targets.len(),
                    limits.navigation_occurrences,
                    "navigation occurrences",
                )?;
                for target in &targets {
                    charge(
                        &mut navigation_bytes,
                        target.fact_bytes(),
                        limits.navigation_bytes,
                        "navigation bytes",
                    )?;
                }
                analysis.related_reference_targets = Some(targets);
                match resolved.symbolic_completion_targets(
                    limits
                        .navigation_occurrences
                        .saturating_sub(navigation_count),
                    limits.navigation_bytes.saturating_sub(navigation_bytes),
                ) {
                    Ok(targets) => {
                        charge(
                            &mut navigation_count,
                            targets.len(),
                            limits.navigation_occurrences,
                            "navigation occurrences",
                        )?;
                        for target in &targets {
                            charge(
                                &mut navigation_bytes,
                                target.fact_bytes(),
                                limits.navigation_bytes,
                                "navigation bytes",
                            )?;
                        }
                        analysis.symbolic_targets = Some(targets);
                    }
                    Err(authored::ProjectionError::Limit(resource)) => {
                        return Err(Error::Limit(resource));
                    }
                    Err(_) => {}
                }
                let references = resolved
                    .editorial_references(
                        limits
                            .navigation_occurrences
                            .saturating_sub(navigation_count),
                        limits.navigation_bytes.saturating_sub(navigation_bytes),
                    )
                    .map_err(|error| match error {
                        authored::ProjectionError::Limit(resource) => Error::Limit(resource),
                        _ => Error::InvalidSource,
                    })?;
                documents::append_compiler_references(
                    &mut analysis.editorial_links,
                    references.into_iter().map(|reference| {
                        (
                            reference.path,
                            reference.location,
                            reference.target,
                            reference.definition,
                        )
                    }),
                    &selected,
                    limits,
                    &mut navigation_count,
                    &mut navigation_bytes,
                )?;
                analysis.page_resources = resolved
                    .page_packages()
                    .keys()
                    .map(|content| (content.clone(), resolved.resources(content).to_vec()))
                    .collect();
                analysis.page_packages = resolved.page_packages().clone();
                match resolved.publication_references(
                    limits
                        .navigation_occurrences
                        .saturating_sub(navigation_count),
                    limits.navigation_bytes.saturating_sub(navigation_bytes),
                ) {
                    Ok(references) => {
                        if let Ok(links) = &mut analysis.document_links {
                            documents::append_compiler_references(
                                links,
                                references.into_iter().map(|reference| {
                                    (
                                        reference.path,
                                        reference.location,
                                        reference.target,
                                        reference.definition,
                                    )
                                }),
                                &selected,
                                limits,
                                &mut navigation_count,
                                &mut navigation_bytes,
                            )?;
                        }
                    }
                    Err(authored::ProjectionError::Limit(resource)) => {
                        return Err(Error::Limit(resource));
                    }
                    Err(authored::ProjectionError::Landing {
                        origin,
                        code,
                        message,
                    }) => {
                        let source = selected.get(&origin.path).ok_or(Error::InvalidSource)?;
                        if origin.source_sha256 != Sha256Digest::of(source) {
                            return Err(Error::InvalidSource);
                        }
                        let failure = || corpus::Error::File {
                            path: origin.path.clone(),
                            error: corpus::FileError::Landing(
                                gazette_compiler::landing::Error::Yaml(
                                    gazette_compiler::yaml::Error {
                                        code,
                                        message: message.clone(),
                                        span: origin.span,
                                    },
                                ),
                            ),
                        };
                        analysis.failures.push(Failure::Corpus(failure()));
                        analysis.document_links = Err(());
                    }
                    Err(authored::ProjectionError::Document { origin, error }) => {
                        let document::Error::Markdown { error, offset } = *error else {
                            return Err(Error::InvalidSource);
                        };
                        let source = selected.get(&origin.path).ok_or(Error::InvalidSource)?;
                        if origin.source_sha256 != Sha256Digest::of(source) {
                            return Err(Error::InvalidSource);
                        }
                        let source =
                            std::str::from_utf8(source).map_err(|_| Error::InvalidSource)?;
                        let body_span = Span::checked(source, offset..source.len())
                            .ok_or(Error::InvalidSource)?;
                        analysis.failures.push(Failure::Renderability {
                            path: origin.path.clone(),
                            body_span,
                            error: error.clone(),
                        });
                        // A failed complete reference pass cannot leave a successful
                        // cross-document navigation subset. Retain the native failure.
                        analysis.document_links = Err(());
                    }
                    Err(_) => return Err(Error::InvalidSource),
                }
            }
            Err(error) => analysis.failures.push(Failure::Authored(error)),
        }
    }
    Ok(analysis)
}
