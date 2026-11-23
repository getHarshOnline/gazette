//! Complete namespace and reference checks; errors never expose a partial topology.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use super::{Declaration, Diagnostic, Error, Key, Workspace};

pub(super) fn workspace(
    mut declaration: Declaration,
    limits: crate::toml::Limits,
) -> Result<Workspace, Error> {
    debug_assert!(declaration.includes.is_empty());
    for site in &mut declaration.sites {
        if let Some(brand) = site.brand.take() {
            declaration.brands.push(brand);
        }
    }
    for publication in declaration
        .brands
        .iter_mut()
        .flat_map(|brand| &mut brand.publications)
    {
        if publication.destination.is_none() {
            publication.destination = publication.repository.as_ref().map(|remote| {
                gazette_core::release::PublicationDestination::Git {
                    remote: remote.clone(),
                    branch: "main".into(),
                }
            });
        }
    }
    let mut errors = Vec::new();
    let extensions =
        super::extensions::Extensions::compile(declaration.extensions).map_err(Error::Invalid)?;
    if let Some(renderer) = &declaration.dot_renderer
        && renderer.executable.validate().is_err()
    {
        report(&mut errors, "workspace_dot_command", "dot_renderer", None);
    }
    if let Some(renderer) = &declaration.social_card_renderer {
        for (field, executable) in [
            ("social_card_renderer.node", &renderer.node),
            ("social_card_renderer.browser", &renderer.browser),
        ] {
            if executable.validate().is_err() {
                report(&mut errors, "workspace_social_card_command", field, None);
            }
        }
    }
    if let Some(typesetter) = &declaration.math_typesetter
        && (typesetter.command.validate().is_err() || typesetter.command.executable.is_none())
    {
        report(
            &mut errors,
            "workspace_math_command",
            "math_typesetter",
            None,
        );
    }
    if declaration.policy_output_transport.is_some() && declaration.policy_check.is_none() {
        report(
            &mut errors,
            "workspace_policy_transport",
            "policy_output_transport",
            None,
        );
    }
    if let Some(command) = &declaration.policy_check
        && command.validate().is_err()
    {
        report(
            &mut errors,
            "workspace_policy_command",
            "policy_check",
            None,
        );
    }
    for (id, name, code, field) in declaration
        .principals
        .iter()
        .map(|principal| {
            (
                &principal.id,
                &principal.name,
                "workspace_principal_name",
                "principals.name",
            )
        })
        .chain(
            declaration
                .projects
                .iter()
                .filter_map(super::ProjectDeclaration::inline)
                .map(|project| {
                    (
                        &project.id,
                        &project.name,
                        "workspace_project_name",
                        "projects.name",
                    )
                }),
        )
    {
        if name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().any(char::is_control))
        {
            report(&mut errors, code, field, Some(id));
        }
    }
    super::projects::validate_facts(
        declaration
            .projects
            .iter()
            .filter_map(super::ProjectDeclaration::inline),
        &mut errors,
    );
    let mut principals = index(
        declaration.principals,
        "principals",
        |value| &value.id,
        &mut errors,
    );
    let accounts = super::accounts::resolve(declaration.accounts, &principals, &mut errors);
    super::publication_defaults::validate_accounts(
        declaration.defaults.as_ref(),
        &declaration.publishers,
        &accounts,
        &mut errors,
    )?;
    for publisher in &declaration.publishers {
        if !principals.contains_key(&publisher.id) {
            report(
                &mut errors,
                "workspace_reference",
                "publishers.id",
                Some(&publisher.id),
            );
        }
    }
    super::facts::validate(&mut declaration.fact_sources, &principals, &mut errors);
    for site in &mut declaration.sites {
        site.roles.sort();
        for binding in &site.roles {
            if !principals.contains_key(&binding.principal) {
                report(
                    &mut errors,
                    "workspace_reference",
                    &format!("sites[{:?}].roles.principal", site.id.as_str()),
                    Some(&binding.principal),
                );
            }
        }
        if site.roles.windows(2).any(|pair| pair[0] == pair[1]) {
            report(
                &mut errors,
                "workspace_duplicate",
                "sites.roles",
                Some(&site.id),
            );
        }
        if !principals.contains_key(&site.publisher) {
            report(
                &mut errors,
                "workspace_reference",
                &format!("sites[{:?}].publisher", site.id.as_str()),
                Some(&site.publisher),
            );
        }
        for (field, selected) in [
            ("rights_holder", site.rights_holder.as_ref()),
            ("default_author", site.default_author.as_ref()),
        ] {
            if let Some(selected) = selected {
                match principals.get(selected) {
                    None => report(
                        &mut errors,
                        "workspace_reference",
                        &format!("sites[{:?}].{field}", site.id.as_str()),
                        Some(selected),
                    ),
                    Some(principal)
                        if field == "default_author"
                            && principal.kind
                                != gazette_core::publication::PrincipalKind::Person =>
                    {
                        report(
                            &mut errors,
                            "workspace_default_author_kind",
                            &format!("sites[{:?}].{field}", site.id.as_str()),
                            Some(selected),
                        );
                    }
                    Some(_) => (),
                }
            }
        }
    }
    let sites = index(
        declaration
            .sites
            .into_iter()
            .map(|declaration| declaration.site)
            .collect(),
        "sites",
        |value| &value.id,
        &mut errors,
    );
    if let Some(build) = &declaration.build {
        build.validate(&sites, &mut errors);
    }
    let mut declared_brands = declaration.brands;
    let mut publication_sites = BTreeSet::new();
    let mut documentation_paths = BTreeSet::new();
    let mut feed_identities = BTreeMap::new();
    for brand in &mut declared_brands {
        if brand.name.trim().is_empty() || brand.name.chars().any(char::is_control) {
            report(
                &mut errors,
                "workspace_brand_name",
                "brands.name",
                Some(&brand.id),
            );
        }
        brand
            .publications
            .sort_by(|left, right| left.site.cmp(&right.site));
        for publication in &brand.publications {
            let field = format!(
                "brands[{:?}].publications[{:?}]",
                brand.id.as_str(),
                publication.site.as_str()
            );
            let mut listing_owners = BTreeSet::new();
            for listing in &publication.listings {
                if !listing_owners.insert(&listing.content) {
                    report(
                        &mut errors,
                        "workspace_duplicate",
                        &format!("{field}.listings.content"),
                        Some(&listing.content),
                    );
                }
                if listing.validate_alternate().is_err() {
                    report(
                        &mut errors,
                        "workspace_listing_order",
                        &format!("{field}.listings.alternate_order"),
                        Some(&listing.content),
                    );
                }
                if listing.pagination.continuation(2).is_err() {
                    report(
                        &mut errors,
                        "workspace_listing_pagination",
                        &format!("{field}.listings.pagination"),
                        Some(&listing.content),
                    );
                }
                if let super::ListingOrder::Priority { first, .. } = &listing.order {
                    let mut priorities = BTreeSet::new();
                    for id in first {
                        if !priorities.insert(id) {
                            report(
                                &mut errors,
                                "workspace_duplicate",
                                &format!("{field}.listings.order.first"),
                                Some(id),
                            );
                        }
                    }
                }
            }
            if publication
                .machine
                .sitemap_urls_per_file
                .is_some_and(|count| !publication.machine.sitemap || !(1..=50_000).contains(&count))
            {
                report(
                    &mut errors,
                    "workspace_sitemap_partition",
                    &format!("{field}.machine.sitemap_urls_per_file"),
                    Some(&publication.site),
                );
            }
            if !sites.contains_key(&publication.site) {
                report(
                    &mut errors,
                    "workspace_reference",
                    &format!("{field}.site"),
                    Some(&publication.site),
                );
            }
            if !publication_sites.insert(publication.site.clone()) {
                // The diagnostic is site-owned so declaration order cannot choose a winner.
                report(
                    &mut errors,
                    "workspace_duplicate",
                    "brands.publications.site",
                    Some(&publication.site),
                );
            }
            if publication
                .destination
                .as_ref()
                .is_some_and(|destination| destination.validate().is_err())
            {
                report(
                    &mut errors,
                    "workspace_destination",
                    &format!("{field}.destination"),
                    Some(&publication.site),
                );
            }
            if publication
                .repository
                .as_ref()
                .is_some_and(|value| !super::repositories::valid(value))
            {
                report(
                    &mut errors,
                    "workspace_publication_repository",
                    &format!("{field}.repository"),
                    Some(&publication.site),
                );
            }
            if let Some(repository) = &publication.documentation_repository {
                if repository.name.is_empty()
                    || matches!(repository.name.as_str(), "." | "..")
                    || repository
                        .name
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'))
                {
                    report(
                        &mut errors,
                        "workspace_repository_name",
                        &format!("{field}.documentation_repository.name"),
                        None,
                    );
                }
                if repository.path.parse::<super::SourceRoot>().is_err() {
                    report(
                        &mut errors,
                        "workspace_repository_path",
                        &format!("{field}.documentation_repository.path"),
                        None,
                    );
                }
                if repository
                    .destination
                    .as_ref()
                    .is_some_and(|destination| destination.validate().is_err())
                {
                    report(
                        &mut errors,
                        "workspace_destination",
                        &format!("{field}.documentation_repository.destination"),
                        Some(&publication.site),
                    );
                }
                if !documentation_paths.insert(repository.path.clone()) {
                    report(
                        &mut errors,
                        "workspace_duplicate",
                        "brands.publications.documentation_repository.path",
                        None,
                    );
                }
            }
        }
        for publication in &mut brand.publications {
            super::feeds::validate(publication, &mut errors, &mut feed_identities);
            super::machine::validate(publication, &mut errors);
        }
    }
    let mut brands = index(declared_brands, "brands", |value| &value.id, &mut errors);
    super::accounts::select(&accounts, &mut brands, &mut errors);
    super::brands::validate(&brands, &mut errors);
    for principal in principals.values() {
        let website_valid = match &principal.website {
            None => true,
            Some(gazette_core::publication::PrincipalWebsite::External { url }) => {
                super::repositories::valid(url)
            }
            Some(gazette_core::publication::PrincipalWebsite::Site { site }) => {
                sites.contains_key(site)
                    && brands
                        .values()
                        .flat_map(|brand| &brand.publications)
                        .any(|publication| {
                            &publication.site == site && publication.public_base_url.is_some()
                        })
            }
        };
        if !website_valid {
            report(
                &mut errors,
                "workspace_principal_website",
                "principals.website",
                Some(&principal.id),
            );
        }
        let mut identities = BTreeSet::new();
        if principal
            .same_as
            .iter()
            .any(|url| !super::repositories::valid(url) || !identities.insert(url))
        {
            report(
                &mut errors,
                "workspace_principal_same_as",
                "principals.same_as",
                Some(&principal.id),
            );
        }
    }
    super::accounts::identities(&accounts, &mut principals);
    // The same native Git target cannot identify both an authored publication and a
    // documentation repository, including for one site. The boolean distinguishes
    // those two independently configured artifact classes without inventing a provider.
    let mut destinations: BTreeMap<(&str, &str), BTreeSet<(&Key, bool)>> = BTreeMap::new();
    for brand in brands.values() {
        for publication in &brand.publications {
            if let Some(gazette_core::release::PublicationDestination::Git { remote, branch }) =
                &publication.destination
            {
                destinations
                    .entry((remote, branch))
                    .or_default()
                    .insert((&publication.site, false));
            }
            if let Some(gazette_core::release::PublicationDestination::Git { remote, branch }) =
                publication
                    .documentation_repository
                    .as_ref()
                    .and_then(|repository| repository.destination.as_ref())
            {
                destinations
                    .entry((remote, branch))
                    .or_default()
                    .insert((&publication.site, true));
            }
        }
    }
    for uses in destinations.values().filter(|uses| uses.len() > 1) {
        for (site, documentation) in uses {
            report(
                &mut errors,
                "workspace_destination_duplicate",
                if *documentation {
                    "brands.publications.documentation_repository.destination"
                } else {
                    "brands.publications.destination"
                },
                Some(site),
            );
        }
    }
    let projects = index(
        declaration.projects,
        "projects",
        super::ProjectDeclaration::id,
        &mut errors,
    );
    super::projects::validate(&projects, &mut errors);
    let mut declared_taxonomies = declaration.taxonomies;
    super::taxonomies::normalize(&mut declared_taxonomies, &mut errors);
    let taxonomies = index(
        declared_taxonomies,
        "taxonomies",
        |value| &value.id,
        &mut errors,
    );
    super::taxonomies::publications(&brands, &taxonomies, &mut errors);
    if sites.is_empty() {
        report(&mut errors, "workspace_empty", "sites", None);
    }
    let sources = index(
        declaration.sources,
        "sources",
        |value| &value.id,
        &mut errors,
    );
    for (name, source) in &declaration.design_inputs {
        if !sources.contains_key(source) {
            report(
                &mut errors,
                "workspace_reference",
                &format!("design_inputs[{:?}]", name.as_str()),
                Some(source),
            );
        }
    }
    let (theme_packages, plugin_packages) = super::packages::index(
        declaration.theme_packages,
        declaration.plugin_packages,
        &sources,
        &declaration.design_inputs,
        &mut errors,
    );

    if let Some(typesetter) = &declaration.math_typesetter
        && !sources.contains_key(&typesetter.source)
    {
        report(
            &mut errors,
            "workspace_reference",
            "math_typesetter.source",
            Some(&typesetter.source),
        );
    }

    if let Some(policy) = &declaration.policy_source {
        if declaration.policy_check.is_none() {
            report(
                &mut errors,
                "workspace_policy_source",
                "policy_source",
                Some(&policy.source),
            );
        }
        if !sources.contains_key(&policy.source) {
            report(
                &mut errors,
                "workspace_reference",
                "policy_source.source",
                Some(&policy.source),
            );
        }
    }

    super::legal::validate(&brands, &sources, &mut errors);
    super::documentation::validate(&brands, &projects, &sources, &mut errors);
    let documentation_mounts = super::documentation_mounts::resolve(
        declaration.documentation,
        &sites,
        &sources,
        &mut errors,
    );
    let mut content_targets = BTreeSet::new();
    for brand in brands.values() {
        for binding in brand
            .publications
            .iter()
            .flat_map(|publication| &publication.authored)
        {
            if !sources.contains_key(&binding.source) {
                report(
                    &mut errors,
                    "workspace_reference",
                    "brands.publications.authored.source",
                    Some(&binding.source),
                );
            }
            if !content_targets.insert(binding.target.as_str()) {
                report(
                    &mut errors,
                    "workspace_content_overlap",
                    "content_bindings.target",
                    None,
                );
            }
        }
    }
    // One namespaced key cannot change identity form merely by changing its table.
    let mut owners = BTreeMap::new();
    for (kind, key) in principals
        .keys()
        .map(|key| ("principals", key))
        .chain(accounts.keys().map(|key| ("accounts", key)))
        .chain(sites.keys().map(|key| ("sites", key)))
        .chain(projects.keys().map(|key| ("projects", key)))
        .chain(brands.keys().map(|key| ("brands", key)))
        .chain(taxonomies.keys().map(|key| ("taxonomies", key)))
    {
        if owners.insert(key, kind).is_some() {
            report(&mut errors, "workspace_identity_conflict", kind, Some(key));
        }
    }

    let mut placements = declaration.placements;
    placements
        .sort_by(|left, right| (&left.site, &left.project).cmp(&(&right.site, &right.project)));
    let mut seen = BTreeSet::new();
    let mut placement_roles = BTreeMap::<&Key, usize>::new();
    for placement in &placements {
        let field = format!(
            "placements[{:?},{:?}]",
            placement.site.as_str(),
            placement.project.as_str()
        );
        if !seen.insert((&placement.site, &placement.project)) {
            report(&mut errors, "workspace_duplicate", &field, None);
        }
        if !sites.contains_key(&placement.site) {
            report(
                &mut errors,
                "workspace_reference",
                &format!("{field}.site"),
                Some(&placement.site),
            );
        }
        if !projects.contains_key(&placement.project) {
            report(
                &mut errors,
                "workspace_reference",
                &format!("{field}.project"),
                Some(&placement.project),
            );
        } else if placement.role.is_primary() {
            *placement_roles.entry(&placement.project).or_default() += 1;
        } else {
            placement_roles.entry(&placement.project).or_default();
        }
        match &placement.documents {
            Some(documents) if !sources.contains_key(&documents.source) => report(
                &mut errors,
                "workspace_reference",
                &format!("{field}.documents.source"),
                Some(&documents.source),
            ),
            None if placement.render => report(
                &mut errors,
                "workspace_source_required",
                &format!("{field}.documents"),
                None,
            ),
            _ => (),
        }
        if let Some(profile) = &placement.native_markdown {
            if placement.documents.is_none() {
                report(
                    &mut errors,
                    "workspace_source_required",
                    &format!("{field}.documents"),
                    None,
                );
            }
            if !profile.route_prefix.as_str().ends_with('/') {
                report(
                    &mut errors,
                    "workspace_native_route",
                    &format!("{field}.native_markdown.route_prefix"),
                    None,
                );
            }
        }
        if placement.render
            && let Some(binding) = &placement.documents
            && !content_targets.insert(binding.target.as_str())
        {
            report(
                &mut errors,
                "workspace_content_overlap",
                "content_bindings.target",
                None,
            );
        }
    }
    for (project, primary_count) in placement_roles {
        if primary_count != 1 {
            report(
                &mut errors,
                "workspace_placement_primary",
                "placements.role",
                Some(project),
            );
        }
    }
    let mut retention_by_source = BTreeMap::new();
    for binding in brands
        .values()
        .flat_map(|brand| &brand.publications)
        .flat_map(|publication| &publication.authored)
        .chain(
            placements
                .iter()
                .filter_map(|placement| placement.documents.as_ref()),
        )
        .chain(
            documentation_mounts
                .iter()
                .filter(|mount| mount.declaration.render)
                .map(|mount| &mount.binding),
        )
    {
        // The retained math runner receives the complete selected source tree,
        // not only its entrypoint. Retention-only bytes cannot be tool inputs.
        if !binding.retained_only.is_empty()
            && declaration
                .math_typesetter
                .as_ref()
                .is_some_and(|typesetter| typesetter.source == binding.source)
        {
            report(
                &mut errors,
                "workspace_retained_only_dependency",
                "math_typesetter.source",
                Some(&binding.source),
            );
        }
        if retention_by_source
            .insert(&binding.source, &binding.retained_only)
            .is_some_and(|previous| previous != &binding.retained_only)
        {
            report(
                &mut errors,
                "workspace_retained_only_conflict",
                "content_bindings.retained_only",
                Some(&binding.source),
            );
        }
        if binding.target.as_str() == "__legal" || binding.target.as_str().starts_with("__legal/") {
            report(
                &mut errors,
                "workspace_content_overlap",
                "content_bindings.target",
                Some(&binding.source),
            );
        }
        if binding
            .static_assets
            .as_ref()
            .is_some_and(|assets| !assets.route_prefix.as_str().ends_with('/'))
        {
            report(
                &mut errors,
                "workspace_asset_route",
                "content_bindings.static_assets.route_prefix",
                None,
            );
        }
    }
    for mount in documentation_mounts
        .iter()
        .filter(|mount| mount.declaration.render)
    {
        if !content_targets.insert(mount.binding.target.as_str()) {
            report(
                &mut errors,
                "workspace_content_overlap",
                "documentation.mounts",
                Some(&mount.declaration.source),
            );
        }
    }
    for target in &content_targets {
        for (offset, _) in target.match_indices('/') {
            if content_targets.contains(&target[..offset]) {
                report(
                    &mut errors,
                    "workspace_content_overlap",
                    "content_bindings.target",
                    None,
                );
            }
        }
    }
    super::composition::validate(
        &declaration.composition,
        super::SelectionScope::Workspace,
        &theme_packages,
        &plugin_packages,
        &mut errors,
    );
    // Duplicate sites have no unambiguous local body. Do not let first-wins
    // indexing choose dependent composition errors from one conflicting value.
    let ambiguous_sites: BTreeSet<_> = errors
        .iter()
        .filter(|error| error.code == "workspace_duplicate" && error.field == "sites")
        .filter_map(|error| error.key.clone())
        .collect();
    for site in sites
        .values()
        .filter(|site| !ambiguous_sites.contains(&site.id))
    {
        super::composition::validate(
            &site.composition,
            super::SelectionScope::Site(site.id.clone()),
            &theme_packages,
            &plugin_packages,
            &mut errors,
        );
    }
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(Error::Invalid(errors));
    }
    let site_compositions = super::composition::resolve(&declaration.composition, &sites, limits)?;
    for (site, composition) in &site_compositions {
        super::composition::validate_resolved(site, composition, &mut errors);
    }
    for brand in brands.values() {
        for publication in &brand.publications {
            let Some(composition) = site_compositions.get(&publication.site) else {
                continue;
            };
            for (role, resource) in [
                ("site_icon", publication.identity_assets.site_icon.as_ref()),
                (
                    "link_preview",
                    publication.identity_assets.link_preview.as_ref(),
                ),
            ] {
                let Some(resource) = resource else { continue };
                if !matches!(
                    composition
                        .resources()
                        .get(resource)
                        .map(|selected| selected.value()),
                    Some(super::ResourceSelection::Enabled {
                        use_: super::ResourceUse::Site,
                        ..
                    })
                ) {
                    report(
                        &mut errors,
                        "workspace_identity_asset_resource",
                        &format!(
                            "brands[{:?}].publications[{:?}].identity_assets.{role}",
                            brand.id.as_str(),
                            publication.site.as_str()
                        ),
                        None,
                    );
                }
            }
        }
    }
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(Error::Invalid(errors));
    }
    let workspace = Workspace {
        machine_binding: declaration.machine_binding,
        build: declaration.build,
        math_typesetter: declaration.math_typesetter,
        dot_renderer: declaration.dot_renderer,
        social_card_renderer: declaration.social_card_renderer,
        fact_sources: declaration.fact_sources,
        policy_check: declaration.policy_check,
        policy_output_transport: declaration.policy_output_transport,
        policy_source: declaration.policy_source,
        composition: declaration.composition,
        site_compositions,
        brands,
        principals,
        accounts,
        sites,
        projects,
        taxonomies,
        extensions,
        sources,
        design_inputs: declaration.design_inputs,
        theme_packages,
        plugin_packages,
        placements,
        documentation_mounts,
    };
    super::discussion::validate(&workspace, &mut errors);
    super::processing::validate(&workspace, &mut errors);
    if errors.is_empty() {
        Ok(workspace)
    } else {
        errors.sort();
        errors.dedup();
        Err(Error::Invalid(errors))
    }
}

fn index<T>(
    values: Vec<T>,
    field: &str,
    key: impl Fn(&T) -> &Key,
    errors: &mut Vec<Diagnostic>,
) -> BTreeMap<Key, T> {
    let mut result = BTreeMap::new();
    for value in values {
        match result.entry(key(&value).clone()) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(entry) => {
                report(errors, "workspace_duplicate", field, Some(entry.key()))
            }
        }
    }
    result
}

pub(super) fn report(
    errors: &mut Vec<Diagnostic>,
    code: &'static str,
    field: &str,
    key: Option<&Key>,
) {
    errors.push(Diagnostic {
        code,
        field: field.into(),
        key: key.cloned(),
    });
}
