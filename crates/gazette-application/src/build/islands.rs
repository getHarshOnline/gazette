//! Explicit page-local enhancement bindings over the existing retained module compiler.

use std::{collections::BTreeMap, sync::Arc};

use gazette_compiler::authored::PageProjection;
use gazette_core::{
    publication::{Key, routing::PublicBaseUrl},
    resource,
    source::{Content, EntryPath, InventoryBuilder, Sha256Digest, TreeIdentity, TreeInput},
};
use gazette_ports::resource::ResourceCompiler;
use serde::Serialize;

use super::{PublicationRenderError, ResourceProjection, programs::compile_tree};

/// Generated bootstrap evidence, distinct from captured authored program provenance.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct IslandEvidence {
    /// Exact generated program tree derived from admitted component bindings.
    pub input: TreeIdentity,
    /// Exact generated source bytes, without private source coordinates.
    pub source_sha256: Sha256Digest,
    /// Selected native compiler profile; not browser execution proof.
    pub tool: String,
    /// Compiled page-local resource and its exact selected module references.
    pub resource: ResourceProjection,
}

#[derive(Default)]
pub(super) struct CompiledIslands {
    pub model: BTreeMap<Key, ResourceProjection>,
    pub outputs: BTreeMap<EntryPath, Arc<[u8]>>,
    pub evidence: BTreeMap<Key, IslandEvidence>,
}

pub(super) struct PreparedPages<'a> {
    pub pages: BTreeMap<Key, PageProjection<'a>>,
    pub islands: CompiledIslands,
}

// Per-element state is shared across duplicate bootstrap inclusions. No globals,
// framework detection, export fallback, retries or deletion of static meaning.
const RUNTIME: &str = r#"
async function mount(root) {
  if (root.dataset.gazetteState !== 'pending') return;
  const selected = definitions.get(root.dataset.gazetteComponent);
  if (!definitions.has(root.dataset.gazetteComponent)) return;
  root.dataset.gazetteState = 'mounting';
  const slot = root.querySelector(':scope > [data-gazette-mount]');
  let reason = 'props';
  try {
    const props = JSON.parse(root.dataset.gazetteProps);
    if (!props || typeof props !== 'object' || Array.isArray(props)) throw new Error();
    reason = 'target';
    if (!slot) throw new Error();
    reason = 'export';
    if (typeof selected !== 'function') throw new Error();
    reason = 'mount';
    await selected({element: slot, props});
    root.dataset.gazetteState = 'mounted';
  } catch {
    if (slot) slot.replaceChildren();
    root.dataset.gazetteState = 'failed';
    root.dataset.gazetteFailure = reason;
    root.dispatchEvent(new CustomEvent('gazette:island-failure', {bubbles: true, detail: {component: root.dataset.gazetteComponent, reason}}));
  }
}
function start() {
  for (const root of document.querySelectorAll('[data-gazette-component]')) mount(root);
}
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, {once: true});
else start();
"#;

pub(super) fn compile(
    pages: &BTreeMap<Key, PageProjection<'_>>,
    compiler: &impl ResourceCompiler,
    modules: &resource::ModuleLinks,
    base: &PublicBaseUrl,
    limits: resource::Limits,
    maximum: u64,
) -> Result<CompiledIslands, PublicationRenderError> {
    let mut result = CompiledIslands::default();
    let mut remaining = maximum;
    for (page, projection) in pages {
        let definitions: BTreeMap<_, _> = projection
            .component_uses()
            .filter_map(|component| {
                component
                    .interaction
                    .as_ref()
                    .map(|interaction| (&component.name, interaction))
            })
            .collect();
        if definitions.is_empty() {
            continue;
        }
        let mut source = gazette_core::render::LimitedWriter::new(
            Vec::new(),
            limits.inputs.total_bytes.try_into().unwrap_or(usize::MAX),
        );
        let mut imports = BTreeMap::new();
        let mut selections = Vec::new();
        use std::io::Write;
        for (index, (name, interaction)) in definitions.iter().enumerate() {
            let alias = format!("gazette-island-{index}");
            imports.insert(alias.clone(), interaction.module.clone());
            writeln!(source, "import * as m{index} from {alias:?};").map_err(|_| {
                PublicationRenderError::Resource(resource::Error::Limit("island source bytes"))
            })?;
            selections.push(format!(
                "[{},m{index}[{}]]",
                serde_json::to_string(name).map_err(encoding)?,
                serde_json::to_string(&interaction.export).map_err(encoding)?
            ));
        }
        write!(
            source,
            "const definitions = new Map([{}]);\n{RUNTIME}",
            selections.join(",")
        )
        .map_err(|_| {
            PublicationRenderError::Resource(resource::Error::Limit("island source bytes"))
        })?;
        let source = source.into_inner();
        let digest = Sha256Digest::of(&source);
        let entry: EntryPath = "island.js"
            .parse()
            .map_err(resource::Error::Source)
            .map_err(PublicationRenderError::Resource)?;
        let mut inventory = InventoryBuilder::new(limits.inputs);
        inventory
            .insert(
                entry.clone(),
                Content::File {
                    bytes: source.len() as u64,
                    sha256: digest,
                },
            )
            .map_err(resource::Error::Source)
            .map_err(PublicationRenderError::Resource)?;
        let inventory = inventory.finish();
        let tree = TreeInput::new(
            &inventory,
            BTreeMap::from([(entry.clone(), source.as_slice())]),
        )
        .map_err(resource::Error::Source)
        .map_err(PublicationRenderError::Resource)?;
        let program = compile_tree(
            compiler,
            &tree,
            &resource::Recipe::BrowserModule { entry, imports },
            modules,
            limits,
        )
        .map_err(PublicationRenderError::Resource)?;
        // Page identity chooses a deterministic reserved path; the normal inventory
        // rejects collisions with authored assets and every other generated output.
        let name = Sha256Digest::of(page.as_str().as_bytes());
        let route = format!("/gazette/islands/{name}.js")
            .parse()
            .map_err(|_| PublicationRenderError::Binding(page.clone()))?;
        let resource = program
            .project(base, &route, resource::Kind::BrowserModule)
            .map_err(PublicationRenderError::Resource)?;
        if resource.bytes > remaining {
            return Err(PublicationRenderError::Resource(resource::Error::Limit(
                "island output bytes",
            )));
        }
        remaining -= resource.bytes;
        result.evidence.insert(
            page.clone(),
            IslandEvidence {
                input: program.input,
                source_sha256: digest,
                tool: program.tool.into(),
                resource: resource.clone(),
            },
        );
        if result
            .outputs
            .insert(resource.output.clone(), program.bytes)
            .is_some()
        {
            return Err(PublicationRenderError::Binding(page.clone()));
        }
        result.model.insert(page.clone(), resource);
    }
    Ok(result)
}

fn encoding(error: serde_json::Error) -> PublicationRenderError {
    PublicationRenderError::Resource(resource::Error::Compile(error.to_string()))
}
