//! Native loader states and exact namespace equivalence for shared depth analysis.

use super::{Budget, Error, Execution, Return, depth, recursive};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
};

type Namespaces<'a> = BTreeMap<&'a str, &'a str>;
pub(super) type Loaded<'a> = BTreeMap<&'a str, Namespaces<'a>>;

#[derive(Clone, Copy)]
enum Projection {
    Complete,
    Nonempty,
}

impl Projection {
    fn keeps(self, aliases: &Namespaces<'_>) -> bool {
        matches!(self, Self::Complete) || !aliases.is_empty()
    }

    fn fingerprint(self, loaded: &Loaded<'_>, budget: &mut Budget) -> Result<u64, Error> {
        let mut hash = DefaultHasher::new();
        for (name, aliases) in loaded {
            budget.step()?;
            if !self.keeps(aliases) {
                continue;
            }
            name.hash(&mut hash);
            aliases.len().hash(&mut hash);
            for entry in aliases {
                budget.step()?;
                entry.hash(&mut hash);
            }
        }
        Ok(hash.finish())
    }

    fn equal(self, left: &Loaded<'_>, right: &Loaded<'_>) -> bool {
        left.iter()
            .filter(|(_, aliases)| self.keeps(aliases))
            .eq(right.iter().filter(|(_, aliases)| self.keeps(aliases)))
    }
}

/// Fingerprints narrow the search; only complete equality admits a shared state.
/// IDs enter each bucket in creation order, retaining the first equivalent cache.
#[derive(Default)]
pub(super) struct CacheIndex(BTreeMap<u64, Vec<usize>>);

impl CacheIndex {
    fn find(
        &self,
        fingerprint: u64,
        caches: &[Loaded<'_>],
        candidate: &Loaded<'_>,
        projection: Projection,
        budget: &mut Budget,
    ) -> Result<Option<usize>, Error> {
        if let Some(ids) = self.0.get(&fingerprint) {
            for &id in ids {
                budget.step()?;
                if projection.equal(&caches[id], candidate) {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }

    fn insert(&mut self, fingerprint: u64, id: usize, budget: &mut Budget) -> Result<(), Error> {
        // Charge the map entry and vector capacity even when the bucket exists.
        budget.charge(128)?;
        self.0.entry(fingerprint).or_default().push(id);
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Phase<'a> {
    Import(usize),
    ImportReturn(usize, &'a str),
    Parent(usize),
    ParentReturn(usize, &'a str),
}
struct LoadFrame<'a> {
    name: &'a str,
    generation: usize,
    phase: Phase<'a>,
    imports: Namespaces<'a>,
}

fn push_load<'a>(
    tera: &'a tera::Tera,
    loaded: &Loaded<'a>,
    frames: &mut Vec<LoadFrame<'a>>,
    name: &'a str,
    limit: usize,
    budget: &mut Budget,
) -> Result<(), Error> {
    if loaded.contains_key(name) {
        return Ok(());
    }
    // A repeated name is unsafe only without intervening cache publication.
    // Base→import child→publish child→parent base can terminate natively.
    if frames
        .iter()
        .any(|frame| frame.name == name && frame.generation == loaded.len())
    {
        return Err(recursive(
            name,
            "recursive template macro import loading is not allowed",
        ));
    }
    if frames.len() >= limit {
        return Err(depth());
    }
    budget.charge(128)?;
    let mut imports = BTreeMap::new();
    if !tera.templates[name].macros.is_empty() {
        budget.charge(64)?;
        imports.insert("self", name);
    }
    frames.push(LoadFrame {
        name,
        generation: loaded.len(),
        phase: Phase::Import(0),
        imports,
    });
    Ok(())
}

pub(super) fn load<'a>(
    tera: &'a tera::Tera,
    loaded: &mut Loaded<'a>,
    name: &'a str,
    limit: usize,
    budget: &mut Budget,
) -> Result<usize, Error> {
    if loaded.contains_key(name) {
        return Ok(0);
    }
    let mut frames = Vec::new();
    let mut maximum = 0;
    push_load(tera, loaded, &mut frames, name, limit, budget)?;
    while !frames.is_empty() {
        budget.step()?;
        maximum = maximum.max(frames.len());
        let index = frames.len() - 1;
        let owner = frames[index].name;
        let template = &tera.templates[owner];
        match frames[index].phase {
            Phase::Import(index_) => {
                if let Some((target, alias)) = template.imported_macro_files.get(index_) {
                    budget.charge(64)?;
                    frames[index]
                        .imports
                        .insert(alias.as_str(), target.as_str());
                    frames[index].phase = Phase::ImportReturn(index_, target);
                    push_load(tera, loaded, &mut frames, target, limit, budget)?;
                } else {
                    budget.charge(64)?;
                    loaded.insert(owner, std::mem::take(&mut frames[index].imports));
                    frames[index].phase = Phase::Parent(0);
                }
            }
            Phase::ImportReturn(index_, child) => {
                for (&alias, &target) in &loaded[child] {
                    if !frames[index].imports.contains_key(alias) {
                        budget.charge(64)?;
                        frames[index].imports.insert(alias, target);
                    }
                }
                frames[index].phase = Phase::Import(index_ + 1);
            }
            Phase::Parent(index_) => {
                if let Some(parent) = template.parents.get(index_) {
                    frames[index].phase = Phase::ParentReturn(index_, parent);
                    push_load(tera, loaded, &mut frames, parent, limit, budget)?;
                } else {
                    frames.pop();
                }
            }
            Phase::ParentReturn(index_, parent) => {
                // Native cache publication is after imports, before parents.
                let count = loaded[parent]
                    .keys()
                    .filter(|alias| !loaded[owner].contains_key(**alias))
                    .count();
                budget.charge(count.saturating_mul(64))?;
                let additions: Vec<_> = loaded[parent]
                    .iter()
                    .filter(|(alias, _)| !loaded[owner].contains_key(**alias))
                    .map(|(&alias, &target)| (alias, target))
                    .collect();
                for (alias, target) in additions {
                    loaded
                        .get_mut(owner)
                        .expect("published owner")
                        .insert(alias, target);
                }
                frames[index].phase = Phase::Parent(index_ + 1);
            }
        }
    }
    Ok(maximum)
}

impl<'a> Execution<'a, '_> {
    pub(super) fn index_cache(&mut self, id: usize) -> Result<(), Error> {
        let loaded = &self.caches[id];
        let full = Projection::Complete.fingerprint(loaded, self.budget)?;
        let pure = Projection::Nonempty.fingerprint(loaded, self.budget)?;
        self.full_index.insert(full, id, self.budget)?;
        self.pure_index.insert(pure, id, self.budget)
    }

    pub(super) fn pure_scope(&mut self, scope: Return<'a>) -> Result<Return<'a>, Error> {
        if let Some(&cache) = self.pure_caches.get(&scope.cache) {
            return Ok(Return { cache, ..scope });
        }
        // Pure code can read macro namespaces but cannot invoke the loader.
        // Empty loaded-template entries therefore have no observable effect;
        // retain every nonempty namespace map and the exact block lookup owner.
        let loaded = &self.caches[scope.cache];
        let fingerprint = Projection::Nonempty.fingerprint(loaded, self.budget)?;
        let canonical = self
            .pure_index
            .find(
                fingerprint,
                &self.caches,
                loaded,
                Projection::Nonempty,
                self.budget,
            )?
            .expect("every retained cache is indexed");
        self.budget.charge(64)?;
        self.pure_caches.insert(scope.cache, canonical);
        Ok(Return {
            cache: canonical,
            ..scope
        })
    }
    pub(super) fn cache_for(
        &mut self,
        cache: usize,
        name: &'a str,
    ) -> Result<(usize, usize), Error> {
        if self.caches[cache].contains_key(name) {
            return Ok((cache, 0));
        }
        if let Some(&result) = self.loads.get(&(cache, name)) {
            return Ok(result);
        }
        let storage_before = self.budget.remaining;
        let slots = self.caches[cache]
            .values()
            .map(|map| map.len() + 1)
            .sum::<usize>();
        self.budget.charge(slots.saturating_mul(64))?;
        let mut loaded = self.caches[cache].clone();
        let nesting = load(self.tera, &mut loaded, name, self.limit, self.budget)?;
        let fingerprint = Projection::Complete.fingerprint(&loaded, self.budget)?;
        if let Some(index) = self.full_index.find(
            fingerprint,
            &self.caches,
            &loaded,
            Projection::Complete,
            self.budget,
        )? {
            // The clone and its loader frames are discarded, not retained analysis
            // state. Keep all work charged; only the memoized result survives.
            drop(loaded);
            self.budget.remaining = storage_before;
            self.budget.charge(64)?;
            self.loads.insert((cache, name), (index, nesting));
            return Ok((index, nesting));
        }
        self.budget.charge(64)?;
        let id = self.caches.len();
        self.caches.push(loaded);
        self.index_cache(id)?;
        self.budget.charge(64)?;
        self.loads.insert((cache, name), (id, nesting));
        Ok((id, nesting))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/template_cache.rs"]
mod tests;
