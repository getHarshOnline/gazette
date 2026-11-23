//! Session-local reuse of exact resource requests, not publication or source custody.

use std::{cell::RefCell, collections::VecDeque};

use gazette_core::{
    render::LimitedWriter,
    resource::{Artifact, Error, Limits, ModuleLinks, Recipe},
    source::TreeInput,
};
use gazette_ports::resource::ResourceCompiler;

/// Derived retention ceilings, independent of compilation and publication limits.
#[derive(Clone, Copy, Debug)]
pub struct ResourceCacheLimits {
    /// Maximum retained successful requests. Zero disables retention.
    pub entries: usize,
    /// Maximum logical payload bytes across keys, output and evidence strings.
    /// Entry/container overhead and temporary return copies are not included;
    /// this is not an allocator or process-memory limit. Zero disables retention.
    pub bytes: usize,
}

impl Default for ResourceCacheLimits {
    fn default() -> Self {
        Self {
            entries: 128,
            bytes: 32 << 20,
        }
    }
}

/// One bounded cache bound to an owned, immutable resource compiler instance.
///
/// Keys cover the complete retained input tree, recipe, module links and effective
/// limits, including unused declared prerequisites. Only successful compiler
/// results are retained. Application evidence checks still run on every hit.
/// No native reads, persistent writes, clock or cross-compiler reuse occurs here.
pub struct ResourceCache<C> {
    compiler: C,
    limits: ResourceCacheLimits,
    entries: RefCell<VecDeque<Entry>>,
}

struct Entry {
    key: Vec<u8>,
    artifact: Artifact,
    bytes: usize,
}

impl<C> ResourceCache<C> {
    /// Start with empty derived state. Oversized requests bypass retention;
    /// eviction never turns a valid compilation into a failure.
    pub fn new(compiler: C, limits: ResourceCacheLimits) -> Self {
        Self {
            compiler,
            limits,
            entries: RefCell::new(VecDeque::new()),
        }
    }
}

impl<C: ResourceCompiler> ResourceCompiler for ResourceCache<C> {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        if self.limits.entries == 0 || self.limits.bytes == 0 {
            return self.compiler.compile(source, recipe, links, limits);
        }
        let mut key = LimitedWriter::new(Vec::new(), self.limits.bytes);
        if serde_json::to_writer(
            &mut key,
            &(source.inventory().identity(), recipe, links, limits),
        )
        .is_err()
            || key.exceeded()
        {
            return self.compiler.compile(source, recipe, links, limits);
        }
        let key = key.into_inner();
        if let Some(entry) = self.entries.borrow().iter().find(|entry| entry.key == key) {
            return Ok(entry.artifact.clone());
        }
        let artifact = self.compiler.compile(source, recipe, links, limits)?;
        let size = std::iter::once(key.len())
            .chain([artifact.bytes.len(), artifact.tool.len()])
            .chain(artifact.dependencies.iter().map(|path| path.as_str().len()))
            .chain(artifact.references.iter().map(String::len))
            .chain(artifact.deferred_references.iter().map(String::len))
            .try_fold(0usize, usize::checked_add);
        if let Some(bytes) = size.filter(|bytes| *bytes <= self.limits.bytes) {
            let mut entries = self.entries.borrow_mut();
            let mut retained: usize = entries.iter().map(|entry| entry.bytes).sum();
            while entries.len() >= self.limits.entries || retained > self.limits.bytes - bytes {
                if let Some(entry) = entries.pop_front() {
                    retained -= entry.bytes;
                } else {
                    break;
                }
            }
            entries.push_back(Entry {
                key,
                artifact: artifact.clone(),
                bytes,
            });
        }
        Ok(artifact)
    }
}
