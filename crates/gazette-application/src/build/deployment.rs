//! Bounded deployment outputs consumed by the ordinary complete-site assembly.

use super::{
    CompiledTheme, GraphRenderEvidence, SiteRenderError,
    presentation::{IdentityAssets, Presentation},
    serialization::{context, encode},
};
use gazette_compiler::{
    authored::PublicationMetadata,
    workspace::{RobotsConfiguration, WebManifestConfiguration},
};
use gazette_core::{
    publication::routing::PublicBaseUrl,
    render,
    source::{EntryPath, Sha256Digest},
};

pub(super) fn invalid(reason: &'static str) -> SiteRenderError {
    SiteRenderError::Render(render::Error::Context(Box::new(std::io::Error::other(
        reason,
    ))))
}

pub(super) fn robots(
    configuration: &RobotsConfiguration,
    sitemap: Option<&str>,
    maximum: usize,
) -> Result<Vec<u8>, SiteRenderError> {
    let mut output = render::LimitedWriter::new(Vec::new(), maximum);
    let result = gazette_core::publication::usage::write_robots(
        &mut output,
        configuration
            .rules
            .iter()
            .map(gazette_compiler::workspace::CrawlerRule::group),
        sitemap,
    );
    if output.exceeded() {
        return Err(SiteRenderError::Render(render::Error::Limit(
            "robots bytes",
        )));
    }
    result.map_err(|error| SiteRenderError::Render(render::Error::Context(Box::new(error))))?;
    Ok(output.into_inner())
}

/// The shared `TDMRep` reservation, refused when it exceeds the output file ceiling.
pub(super) fn tdm_reservation(policy: &str, maximum: usize) -> Result<Vec<u8>, SiteRenderError> {
    let bytes = gazette_core::publication::usage::tdm_reservation(policy);
    if bytes.len() > maximum {
        return Err(SiteRenderError::Render(render::Error::Limit(
            "TDM reservation bytes",
        )));
    }
    Ok(bytes)
}

pub(super) fn manifest(
    configuration: &WebManifestConfiguration,
    publication: &PublicationMetadata<'_>,
    base: &PublicBaseUrl,
    assets: Option<IdentityAssets<'_>>,
    maximum: usize,
) -> Result<Vec<u8>, SiteRenderError> {
    let mut value = serde_json::json!({
        "name": publication.brand.name,
        "start_url": publication.home.as_ref().map_or(base.as_str(), |home| home.url),
        "scope": base.as_str(),
        "display": "browser"
    });
    if let Some(name) = &configuration.short_name {
        value["short_name"] = name.clone().into();
    }
    if let Some(icon) = assets.and_then(|assets| assets.site_icon) {
        let sizes = icon.image.map_or_else(
            || "any".to_owned(),
            |image| format!("{}x{}", image.output.width, image.output.height),
        );
        value["icons"] = serde_json::json!([{ "src": icon.url, "sizes": sizes, "purpose": "any" }]);
    }
    encode(&value, maximum, "web manifest bytes").map_err(SiteRenderError::Render)
}

pub(super) fn not_found(
    theme: &CompiledTheme,
    template: &EntryPath,
    presentation: &Presentation<'_>,
    base: &PublicBaseUrl,
    search: Option<&str>,
    maximum: usize,
) -> Result<(Vec<u8>, GraphRenderEvidence), SiteRenderError> {
    let output: EntryPath = "404.html".parse().map_err(SiteRenderError::Output)?;
    let home = presentation
        .publication
        .as_ref()
        .and_then(|publication| publication.home.as_ref())
        .map_or(base.as_str(), |home| home.url);
    #[derive(serde::Serialize)]
    struct NotFound<'a> {
        url: String,
        home_url: &'a str,
        search_url: Option<&'a str>,
    }
    #[derive(serde::Serialize)]
    struct Model<'a, 'b> {
        site: &'a gazette_core::publication::Key,
        not_found: NotFound<'a>,
        presentation: &'a Presentation<'b>,
    }
    let maximum = maximum.min(theme.evidence().limits.context_bytes);
    let encoded = encode(
        &Model {
            site: &theme.evidence().site,
            not_found: NotFound {
                url: format!("{}404.html", base.as_str()),
                home_url: home,
                search_url: search,
            },
            presentation,
        },
        maximum,
        "not found model bytes",
    )
    .map_err(SiteRenderError::Render)?;
    let model = serde_json::from_slice(&encoded)
        .map_err(|error| SiteRenderError::Render(render::Error::Context(Box::new(error))))?;
    let (model, encoded) =
        context(model, maximum, "not found model bytes").map_err(SiteRenderError::Render)?;
    let bytes = theme
        .render(template, &model)
        .map_err(SiteRenderError::Render)?;
    std::str::from_utf8(&bytes).map_err(SiteRenderError::GraphUtf8)?;
    let evidence = GraphRenderEvidence {
        template: template.clone(),
        output,
        model_sha256: Sha256Digest::of(&encoded),
        output_sha256: Sha256Digest::of(&bytes),
    };
    Ok((bytes, evidence))
}

pub(super) struct Output<'a> {
    pub builder: &'a mut gazette_core::source::InventoryBuilder,
    pub directories: &'a mut std::collections::BTreeSet<EntryPath>,
    pub files: &'a mut std::collections::BTreeMap<EntryPath, Vec<u8>>,
    pub media_types: &'a mut std::collections::BTreeMap<EntryPath, render::MediaType>,
}

impl Output<'_> {
    fn insert(
        &mut self,
        path: EntryPath,
        bytes: Vec<u8>,
        media_type: render::MediaType,
    ) -> Result<(), SiteRenderError> {
        self.media_types.insert(path.clone(), media_type);
        super::site::insert(self.builder, self.directories, self.files, path, bytes)
    }
    fn maximum(&self) -> usize {
        usize::try_from(self.builder.file_ceiling()).unwrap_or(usize::MAX)
    }
}

pub(super) fn machine_outputs(
    publication: &gazette_compiler::workspace::BrandPublication,
    metadata: Option<&PublicationMetadata<'_>>,
    base: &PublicBaseUrl,
    assets: Option<IdentityAssets<'_>>,
    discovery: &mut Vec<super::presentation::OutputDiscovery>,
    output: &mut Output<'_>,
) -> Result<(), SiteRenderError> {
    use super::presentation::{DiscoveryKind, OutputDiscovery};
    if let Some(configuration) = publication
        .machine
        .robots
        .as_ref()
        .filter(|configuration| configuration.enabled)
    {
        let sitemap = discovery
            .iter()
            .find(|item| matches!(item.kind, DiscoveryKind::Sitemap))
            .map(|item| item.url.as_str());
        let bytes = robots(configuration, sitemap, output.maximum())?;
        output.insert(
            "robots.txt".parse().map_err(SiteRenderError::Output)?,
            bytes,
            render::MediaType::Text,
        )?;
    }
    if publication
        .machine
        .terms
        .as_ref()
        .is_some_and(|terms| terms.tdm_reservation)
    {
        let policy = publication
            .terms_url()
            .ok_or_else(|| invalid("TDM reservation requires the production public base URL"))?;
        let bytes = tdm_reservation(&policy, output.maximum())?;
        output.insert(
            ".well-known/tdmrep.json"
                .parse()
                .map_err(SiteRenderError::Output)?,
            bytes,
            render::MediaType::Json,
        )?;
        discovery.push(OutputDiscovery {
            kind: DiscoveryKind::TdmPolicy,
            url: policy,
            media_type: "text/html",
        });
    }
    if let Some(configuration) = publication
        .machine
        .web_manifest
        .as_ref()
        .filter(|configuration| configuration.enabled)
    {
        let bytes = manifest(
            configuration,
            metadata.ok_or_else(|| invalid("web manifest requires publication metadata"))?,
            base,
            assets,
            output.maximum(),
        )?;
        output.insert(
            "manifest.webmanifest"
                .parse()
                .map_err(SiteRenderError::Output)?,
            bytes,
            render::MediaType::WebManifest,
        )?;
        discovery.push(OutputDiscovery {
            kind: DiscoveryKind::WebManifest,
            url: format!("{}manifest.webmanifest", base.as_str()),
            media_type: "application/manifest+json",
        });
    }
    Ok(())
}
