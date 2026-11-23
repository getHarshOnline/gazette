//! Bounded admission over native ASTs, not an evaluator or hostile-code sandbox.
//! Loader phases mirror pinned Tera. Return summaries join possible native block
//! tops because `render_block` retains frames; the join forgets correlations but
//! never substitutes a macro definition owner for the active lookup owner.

use crate::diagnostic::TemplateDiagnostic;
use gazette_core::render::{Error, Limits};
use std::collections::{BTreeMap, BTreeSet};
use tera::ast::{Expr, ExprVal, FunctionCall, Node};

mod cache;
use cache::{CacheIndex, Loaded, load};

fn recursive(template: &str, reason: &str) -> Error {
    Error::Template(Box::new(TemplateDiagnostic::admission(template, reason)))
}
fn depth() -> Error {
    Error::Limit("template execution depth")
}

/// Precede Tera's recursive inheritance builder; missing parents retain its
/// native diagnostic instead of being interpreted by another validator.
pub(crate) fn inheritance(
    templates: &BTreeMap<String, tera::Template>,
    limit: usize,
) -> Result<(), Error> {
    for name in templates.keys() {
        let mut current = name.as_str();
        let mut seen = BTreeSet::new();
        while let Some(template) = templates.get(current) {
            if !seen.insert(current) {
                return Err(recursive(
                    current,
                    "recursive template inheritance is not allowed",
                ));
            }
            if seen.len() > limit {
                return Err(depth());
            }
            let Some(parent) = &template.parent else {
                break;
            };
            current = parent;
        }
    }
    Ok(())
}

/// Fixed profile ceilings also bound branching context/cache combinations.
/// Borrowed metadata is charged before insertion. These are not a CPU sandbox.
struct Budget {
    remaining: usize,
    work: usize,
}
impl Budget {
    fn new(limits: Limits) -> Result<Self, Error> {
        let bytes = limits
            .templates
            .total_bytes
            .saturating_mul(16)
            .clamp(64 << 10, 64 << 20);
        let bytes = usize::try_from(bytes)
            .map_err(|_| Error::Limit("template safety analysis budget representation"))?;
        Ok(Self {
            remaining: bytes,
            work: bytes,
        })
    }
    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or(Error::Limit("template safety analysis storage"))?;
        Ok(())
    }
    fn step(&mut self) -> Result<(), Error> {
        self.work = self
            .work
            .checked_sub(1)
            .ok_or(Error::Limit("template safety analysis work"))?;
        Ok(())
    }
    fn temporary(&self, bytes: usize) -> Result<(), Error> {
        if bytes > self.remaining {
            return Err(Error::Limit("template safety analysis storage"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Block<'a> {
    name: &'a str,
    owner: &'a str,
    level: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Return<'a> {
    top: Option<Block<'a>>,
    cache: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key<'a> {
    active: &'a str,
    body: usize,
    expression: bool,
    scope: Return<'a>,
}
#[derive(Clone, Copy)]
enum Ast<'a> {
    Nodes(&'a [Node]),
    Expression(&'a Expr),
}
struct Body<'a> {
    key: Key<'a>,
    ast: Ast<'a>,
    label: Option<&'a str>,
    edges: BTreeMap<usize, usize>,
    returns: BTreeSet<Return<'a>>,
    callers: BTreeSet<usize>,
    local_depth: usize,
}
struct Execution<'a, 'b> {
    tera: &'a tera::Tera,
    original: &'a str,
    root: &'a str,
    limit: usize,
    caches: Vec<Loaded<'a>>,
    loads: BTreeMap<(usize, &'a str), (usize, usize)>,
    bodies: Vec<Body<'a>>,
    keys: BTreeMap<Key<'a>, usize>,
    transparent: BTreeMap<(usize, bool), bool>,
    pure_caches: BTreeMap<usize, usize>,
    full_index: CacheIndex,
    pure_index: CacheIndex,
    reachable_tops: BTreeSet<Option<Block<'a>>>,
    pending: BTreeSet<usize>,
    super_consumers: BTreeSet<usize>,
    budget: &'b mut Budget,
}

pub(crate) fn admit(tera: &tera::Tera, limits: Limits) -> Result<(), Error> {
    let mut budget = Budget::new(limits)?;
    let mut names: Vec<_> = tera.templates.keys().map(String::as_str).collect();
    names.sort_unstable();
    // Every selected template is a rendering entry. Uncalled definitions are
    // inert: their body is entered only through a resolved native macro call.
    for original in names {
        let storage_before = budget.remaining;
        let mut loaded = BTreeMap::new();
        load(
            tera,
            &mut loaded,
            original,
            limits.execution_depth,
            &mut budget,
        )?;
        let root = tera.templates[original]
            .parents
            .last()
            .map(String::as_str)
            .unwrap_or(original);
        let mut analysis = Execution {
            tera,
            original,
            root,
            limit: limits.execution_depth,
            caches: vec![loaded],
            loads: BTreeMap::new(),
            bodies: Vec::new(),
            keys: BTreeMap::new(),
            transparent: BTreeMap::new(),
            pure_caches: BTreeMap::new(),
            full_index: CacheIndex::default(),
            pure_index: CacheIndex::default(),
            reachable_tops: BTreeSet::from([None]),
            pending: BTreeSet::new(),
            super_consumers: BTreeSet::new(),
            budget: &mut budget,
        };
        analysis.index_cache(0)?;
        analysis.body(
            original,
            &tera.templates[root].ast,
            Return {
                top: None,
                cache: 0,
            },
            None,
        )?;
        analysis.run()?;
        // No per-entry analysis metadata escapes. Reclaim only storage after
        // the owning maps/sets drop; cumulative admission work is never reset.
        drop(analysis);
        budget.remaining = storage_before;
    }
    Ok(())
}

impl<'a> Execution<'a, '_> {
    fn body(
        &mut self,
        active: &'a str,
        nodes: &'a [Node],
        scope: Return<'a>,
        label: Option<&'a str>,
    ) -> Result<usize, Error> {
        self.vertex(active, Ast::Nodes(nodes), scope, label)
    }
    fn vertex(
        &mut self,
        active: &'a str,
        ast: Ast<'a>,
        scope: Return<'a>,
        label: Option<&'a str>,
    ) -> Result<usize, Error> {
        let (body, expression) = match ast {
            Ast::Nodes(nodes) => (nodes.as_ptr() as usize, false),
            Ast::Expression(expr) => (std::ptr::from_ref(expr) as usize, true),
        };
        let key = Key {
            active,
            body,
            expression,
            scope,
        };
        if let Some(&id) = self.keys.get(&key) {
            return Ok(id);
        }
        // Retained body/key plus one reusable pending-work slot.
        self.budget.charge(256 + 64)?;
        let id = self.bodies.len();
        self.pending.insert(id);
        self.keys.insert(key, id);
        self.bodies.push(Body {
            key,
            ast,
            label,
            edges: BTreeMap::new(),
            returns: BTreeSet::new(),
            callers: BTreeSet::new(),
            local_depth: 1,
        });
        Ok(id)
    }
    fn call(
        &mut self,
        from: usize,
        active: &'a str,
        nodes: &'a [Node],
        scope: Return<'a>,
        weight: usize,
        label: Option<&'a str>,
    ) -> Result<BTreeSet<Return<'a>>, Error> {
        // Prove absence of loader/block effects through the complete helper
        // closure, then share depth analysis only across equivalent namespace
        // states. Native target lookup and each caller's state are preserved;
        // data values are never interpreted here.
        let transparent = self.scope_transparent(Ast::Nodes(nodes))?;
        let entered = if transparent {
            self.pure_scope(scope)?
        } else {
            scope
        };
        let target = self.body(active, nodes, entered, label)?;
        let returned = self.edge(from, target, weight)?;
        if transparent && !returned.is_empty() {
            Ok(BTreeSet::from([scope]))
        } else {
            Ok(returned)
        }
    }
    fn scope_transparent(&mut self, ast: Ast<'a>) -> Result<bool, Error> {
        let identity = match ast {
            Ast::Nodes(nodes) => (nodes.as_ptr() as usize, false),
            Ast::Expression(expr) => (std::ptr::from_ref(expr) as usize, true),
        };
        if let Some(&transparent) = self.transparent.get(&identity) {
            return Ok(transparent);
        }
        let mut pending = vec![(ast, false)];
        let mut visiting = BTreeSet::new();
        let mut transparent = true;
        while let Some((ast, returning)) = pending.pop() {
            self.budget.step()?;
            let current = match ast {
                Ast::Nodes(nodes) => (nodes.as_ptr() as usize, false),
                Ast::Expression(expr) => (std::ptr::from_ref(expr) as usize, true),
            };
            if returning {
                visiting.remove(&current);
                if !self.transparent.contains_key(&current) {
                    self.budget.charge(64)?;
                }
                self.transparent.insert(current, true);
                continue;
            }
            if let Some(&known) = self.transparent.get(&current) {
                if !known {
                    transparent = false;
                    break;
                }
                continue;
            }
            // A cycle is not a purity proof. Defer it to the ordinary call graph,
            // whose recursion/depth checks retain the exact native lookup owner.
            if !visiting.insert(current) {
                transparent = false;
                break;
            }
            pending.push((ast, true));
            let mut children = Vec::new();
            match ast {
                Ast::Nodes(nodes) => {
                    for node in nodes {
                        self.budget.step()?;
                        match node {
                            Node::Include(..)
                            | Node::Block(..)
                            | Node::Super
                            | Node::Extends(..) => {
                                transparent = false;
                                break;
                            }
                            Node::VariableBlock(_, expr) => children.push(Ast::Expression(expr)),
                            Node::Set(_, set) => children.push(Ast::Expression(&set.value)),
                            Node::If(if_, _) => {
                                for (_, condition, body) in &if_.conditions {
                                    children.push(Ast::Expression(condition));
                                    children.push(Ast::Nodes(body));
                                }
                                if let Some((_, body)) = &if_.otherwise {
                                    children.push(Ast::Nodes(body));
                                }
                            }
                            Node::Forloop(_, loop_, _) => {
                                children.push(Ast::Expression(&loop_.container));
                                children.push(Ast::Nodes(&loop_.body));
                                if let Some(body) = &loop_.empty_body {
                                    children.push(Ast::Nodes(body));
                                }
                            }
                            Node::FilterSection(_, section, _) => {
                                children.push(Ast::Nodes(&section.body));
                                children.extend(section.filter.args.values().map(Ast::Expression));
                            }
                            Node::MacroDefinition(..)
                            | Node::ImportMacro(..)
                            | Node::Text(..)
                            | Node::Raw(..)
                            | Node::Comment(..)
                            | Node::Break(..)
                            | Node::Continue(..) => {}
                        }
                    }
                }
                Ast::Expression(expr) => {
                    if let ExprVal::MacroCall(call) = &expr.val {
                        children.extend(call.args.values().map(Ast::Expression));
                        // Lookup depends on the native block/cache context. A
                        // macro call is pure only if every possible definition
                        // of that name is pure; unresolved calls cannot mutate
                        // state. This conservative union never selects a target.
                        let mut definitions = Vec::new();
                        for (name, template) in &self.tera.templates {
                            self.budget.step()?;
                            if let Some(definition) = template.macros.get(&call.name) {
                                definitions.push((name, &definition.body));
                            }
                        }
                        self.budget
                            .temporary(definitions.len().saturating_mul(32))?;
                        definitions.sort_unstable_by(|left, right| left.0.cmp(right.0));
                        children.extend(definitions.into_iter().map(|(_, body)| Ast::Nodes(body)));
                    } else {
                        match &expr.val {
                            ExprVal::Math(value) => children
                                .extend([Ast::Expression(&value.lhs), Ast::Expression(&value.rhs)]),
                            ExprVal::Logic(value) => children
                                .extend([Ast::Expression(&value.lhs), Ast::Expression(&value.rhs)]),
                            ExprVal::In(value) => children
                                .extend([Ast::Expression(&value.lhs), Ast::Expression(&value.rhs)]),
                            ExprVal::Test(value) => {
                                children.extend(value.args.iter().map(Ast::Expression))
                            }
                            ExprVal::FunctionCall(value) => {
                                children.extend(value.args.values().map(Ast::Expression))
                            }
                            ExprVal::Array(values) => {
                                children.extend(values.iter().map(Ast::Expression))
                            }
                            ExprVal::StringConcat(value) => {
                                // Pinned Tera concatenates scalars or function calls in
                                // source order. Function arguments remain full expressions.
                                for value in &value.values {
                                    self.budget.step()?;
                                    if let ExprVal::FunctionCall(call) = value {
                                        children.extend(call.args.values().map(Ast::Expression));
                                    }
                                }
                            }
                            ExprVal::MacroCall(_)
                            | ExprVal::String(_)
                            | ExprVal::Int(_)
                            | ExprVal::Float(_)
                            | ExprVal::Bool(_)
                            | ExprVal::Ident(_) => {}
                        }
                    }
                    for filter in &expr.filters {
                        children.extend(filter.args.values().map(Ast::Expression));
                    }
                }
            }
            if !transparent {
                break;
            }
            pending.extend(children.into_iter().rev().map(|child| (child, false)));
            self.budget.temporary(
                pending
                    .len()
                    .saturating_mul(32)
                    .saturating_add(visiting.len().saturating_mul(64)),
            )?;
        }
        if !self.transparent.contains_key(&identity) {
            self.budget.charge(64)?;
        }
        self.transparent.insert(identity, transparent);
        Ok(transparent)
    }
    fn edge(
        &mut self,
        from: usize,
        target: usize,
        weight: usize,
    ) -> Result<BTreeSet<Return<'a>>, Error> {
        if !self.bodies[from].edges.contains_key(&target) {
            // Both the forward edge and its reverse summary dependency.
            self.budget.charge(128)?;
            self.bodies[target].callers.insert(from);
        }
        self.bodies[from]
            .edges
            .entry(target)
            .and_modify(|old| *old = (*old).max(weight))
            .or_insert(weight);
        self.budget
            .temporary(self.bodies[target].returns.len().saturating_mul(64))?;
        Ok(self.bodies[target].returns.clone())
    }
    fn run(&mut self) -> Result<(), Error> {
        // A body depends only on its callees' monotone return summaries and, for
        // super(), the admitted global block-top join. Settled bodies need no
        // blanket rescan merely because an unrelated path acquired a summary.
        // Visit newer callees before their callers so sibling summaries settle
        // together instead of repeatedly restarting the caller between them.
        while let Some(id) = self.pending.pop_last() {
            self.scan(id)?;
        }
        // Edges retain maximum weight and bodies retain maximum local depth,
        // independently of summary cardinality. Check the complete graph once.
        self.longest_paths()
    }
    fn block_top(&mut self, top: Option<Block<'a>>) -> Result<(), Error> {
        if self.reachable_tops.insert(top) {
            self.budget.charge(64)?;
            self.pending.extend(self.super_consumers.iter().copied());
        }
        Ok(())
    }
    fn scan(&mut self, id: usize) -> Result<(), Error> {
        self.budget.step()?;
        match self.bodies[id].ast {
            Ast::Nodes(nodes) => self.scan_nodes(id, nodes),
            Ast::Expression(expr) => self.scan_expression(id, expr),
        }
    }
    fn record_returns(&mut self, id: usize, scopes: BTreeSet<Return<'a>>) -> Result<(), Error> {
        let mut changed = false;
        for scope in scopes {
            if !self.bodies[id].returns.contains(&scope) {
                self.budget.charge(64)?;
                self.bodies[id].returns.insert(scope);
                changed = true;
            }
        }
        if changed {
            self.pending.extend(self.bodies[id].callers.iter().copied());
        }
        Ok(())
    }
    fn scan_nodes(&mut self, id: usize, nodes: &'a [Node]) -> Result<(), Error> {
        let key = self.bodies[id].key;
        let mut scopes = BTreeSet::from([key.scope]);
        for node in nodes {
            if scopes.is_empty() {
                // Bottom is not a pre-call identity: defer continuation until
                // the callee's monotone return summary is available.
                break;
            }
            self.budget.step()?;
            self.budget.temporary(scopes.len().saturating_mul(64))?;
            let incoming: Vec<_> = scopes.iter().copied().collect();
            if !matches!(
                node,
                Node::Text(_)
                    | Node::Raw(_, _, _)
                    | Node::MacroDefinition(_, _, _)
                    | Node::ImportMacro(_, _, _)
                    | Node::Comment(_, _)
            ) {
                scopes.clear();
            }
            match node {
                Node::VariableBlock(_, expr) => {
                    for scope in incoming {
                        scopes.extend(self.expression(id, key.active, expr, scope)?);
                    }
                }
                Node::Set(_, set) => {
                    for scope in incoming {
                        scopes.extend(self.expression(id, key.active, &set.value, scope)?);
                    }
                }
                Node::Include(_, candidates, ignore_missing) => {
                    if let Some(name) = candidates
                        .iter()
                        .find(|name| self.tera.templates.contains_key(*name))
                    {
                        let name = self
                            .tera
                            .templates
                            .get_key_value(name)
                            .expect("existing include")
                            .0
                            .as_str();
                        for scope in incoming {
                            let (cache, loading) = self.cache_for(scope.cache, name)?;
                            self.bodies[id].local_depth =
                                self.bodies[id].local_depth.max(1 + loading);
                            scopes.extend(self.call(
                                id,
                                name,
                                &self.tera.templates[name].ast,
                                Return { cache, ..scope },
                                1,
                                None,
                            )?);
                        }
                    } else if *ignore_missing {
                        scopes.extend(incoming);
                    }
                }
                Node::Block(_, block, _) => {
                    let template = &self.tera.templates[key.active];
                    let mut selected = None;
                    for level in 0..=template.parents.len() {
                        let owner = if level == 0 {
                            key.active
                        } else {
                            template.parents[level - 1].as_str()
                        };
                        if let Some(definitions) = self.tera.templates[owner]
                            .blocks_definitions
                            .get(&block.name)
                        {
                            selected = Some((owner, level, &definitions[0].1.body));
                            break;
                        }
                    }
                    if let Some((owner, level, body)) = selected {
                        let top = Some(Block {
                            name: &block.name,
                            owner,
                            level,
                        });
                        self.block_top(top)?;
                        for scope in incoming {
                            let entered = Return { top, ..scope };
                            scopes.extend(self.call(id, key.active, body, entered, 1, None)?);
                        }
                    } else {
                        for scope in incoming {
                            scopes.extend(self.call(
                                id,
                                key.active,
                                &block.body,
                                scope,
                                1,
                                None,
                            )?);
                        }
                    }
                }
                Node::Super => {
                    if self.super_consumers.insert(id) {
                        self.budget.charge(64)?;
                    }
                    for scope in incoming {
                        let Some(top) = scope.top else {
                            return Err(recursive(
                                key.active,
                                "super() requires an admitted active template block",
                            ));
                        };
                        let parents = &self.tera.templates[self.original].parents;
                        for level in top.level + 1..=parents.len() {
                            if let Some(definitions) = self.tera.templates
                                [parents[level - 1].as_str()]
                            .blocks_definitions
                            .get(top.name)
                            {
                                let (owner, block) = &definitions[0];
                                let entered = Return {
                                    top: Some(Block {
                                        name: top.name,
                                        owner,
                                        level,
                                    }),
                                    ..scope
                                };
                                self.block_top(entered.top)?;
                                let returned =
                                    self.call(id, key.active, &block.body, entered, 1, None)?;
                                for result in returned {
                                    if level < parents.len() && result.top == entered.top {
                                        scopes.insert(Return {
                                            cache: result.cache,
                                            ..scope
                                        });
                                    } else {
                                        // The terminal super pops the caller as
                                        // well, or a callee retained another
                                        // block. Only then forget deeper-stack
                                        // correlations and join reachable tops.
                                        self.budget.temporary(
                                            self.reachable_tops.len().saturating_mul(64),
                                        )?;
                                        scopes.extend(self.reachable_tops.iter().map(|&top| {
                                            Return {
                                                top,
                                                cache: result.cache,
                                            }
                                        }));
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
                Node::If(if_, _) => {
                    let mut can_reach = true;
                    let mut condition_scopes: BTreeSet<_> = incoming.into_iter().collect();
                    for (_, condition, body) in &if_.conditions {
                        if !can_reach {
                            break;
                        }
                        let before: Vec<_> = condition_scopes.iter().copied().collect();
                        condition_scopes.clear();
                        for scope in before {
                            condition_scopes
                                .extend(self.expression(id, key.active, condition, scope)?);
                        }
                        let truth = match &condition.val {
                            ExprVal::Bool(value) if condition.filters.is_empty() => {
                                Some(*value != condition.negated)
                            }
                            _ => None,
                        };
                        if truth != Some(false) {
                            for &scope in &condition_scopes {
                                scopes.extend(self.call(id, key.active, body, scope, 1, None)?);
                            }
                        }
                        if truth == Some(true) {
                            can_reach = false;
                        }
                    }
                    if can_reach && let Some((_, body)) = &if_.otherwise {
                        for scope in condition_scopes {
                            scopes.extend(self.call(id, key.active, body, scope, 1, None)?);
                        }
                    } else if can_reach {
                        scopes.extend(condition_scopes);
                    }
                }
                Node::Forloop(_, loop_, _) => {
                    let mut iterations = BTreeSet::new();
                    for scope in incoming {
                        iterations.extend(self.expression(
                            id,
                            key.active,
                            &loop_.container,
                            scope,
                        )?);
                    }
                    // Literal arrays have exact native cardinality. Do not
                    // invent a second iteration for [0], or enter []'s body.
                    if let ExprVal::Array(values) = &loop_.container.val
                        && loop_.container.filters.is_empty()
                    {
                        if values.is_empty() {
                            if let Some(body) = &loop_.empty_body {
                                for scope in iterations {
                                    scopes.extend(self.call(id, key.active, body, scope, 1, None)?);
                                }
                            } else {
                                scopes.extend(iterations);
                            }
                        } else {
                            for _ in values {
                                let mut returned = BTreeSet::new();
                                for scope in iterations {
                                    returned.extend(self.call(
                                        id,
                                        key.active,
                                        &loop_.body,
                                        scope,
                                        1,
                                        None,
                                    )?);
                                }
                                iterations = returned;
                            }
                            scopes.extend(iterations);
                        }
                        continue;
                    }
                    if let Some(body) = &loop_.empty_body {
                        for &scope in &iterations {
                            scopes.extend(self.call(id, key.active, body, scope, 1, None)?);
                        }
                    } else {
                        scopes.extend(iterations.iter().copied());
                    }
                    let mut pending: Vec<_> = iterations.iter().copied().collect();
                    while let Some(scope) = pending.pop() {
                        self.budget.step()?;
                        for result in self.call(id, key.active, &loop_.body, scope, 1, None)? {
                            if iterations.insert(result) {
                                self.budget
                                    .temporary(iterations.len().saturating_mul(128))?;
                                pending.push(result);
                            }
                        }
                    }
                    scopes.extend(iterations);
                }
                Node::FilterSection(_, section, _) => {
                    for scope in incoming {
                        let returned = self.call(id, key.active, &section.body, scope, 1, None)?;
                        for scope in returned {
                            scopes.extend(self.function(id, key.active, &section.filter, scope)?);
                        }
                    }
                }
                // Included Extends errors before its body; its loader has already
                // run. An error does not try a later include fallback candidate.
                Node::Extends(_, _) => break,
                Node::Break(_) | Node::Continue(_) => {
                    scopes.extend(incoming);
                    break;
                }
                Node::MacroDefinition(_, _, _)
                | Node::Text(_)
                | Node::ImportMacro(_, _, _)
                | Node::Raw(_, _, _)
                | Node::Comment(_, _) => {}
            }
        }
        self.record_returns(id, scopes)
    }
    fn expression(
        &mut self,
        id: usize,
        active: &'a str,
        expr: &'a Expr,
        scope: Return<'a>,
    ) -> Result<BTreeSet<Return<'a>>, Error> {
        // Scalar/filter expressions proved effect-free neither read nor modify
        // the loader/block state. Analyze their depth once, then preserve each
        // caller's exact state just as for an effect-free body.
        let transparent = self.scope_transparent(Ast::Expression(expr))?;
        let entered = if transparent {
            self.pure_scope(scope)?
        } else {
            scope
        };
        let target = self.vertex(active, Ast::Expression(expr), entered, None)?;
        let returned = self.edge(id, target, 1)?;
        if transparent && !returned.is_empty() {
            Ok(BTreeSet::from([scope]))
        } else {
            Ok(returned)
        }
    }
    fn function(
        &mut self,
        id: usize,
        active: &'a str,
        call: &'a FunctionCall,
        scope: Return<'a>,
    ) -> Result<BTreeSet<Return<'a>>, Error> {
        self.budget.temporary(call.args.len().saturating_mul(16))?;
        self.arguments(
            id,
            active,
            call.args.values().collect(),
            BTreeSet::from([scope]),
            true,
        )
    }
    /// Use the same retained native argument map's iteration order as rendering.
    /// Each expression is evaluated once; pass its returned states to the next
    /// argument before dispatching the already selected macro body.
    fn arguments(
        &mut self,
        id: usize,
        active: &'a str,
        expressions: Vec<&'a Expr>,
        initial: BTreeSet<Return<'a>>,
        unordered: bool,
    ) -> Result<BTreeSet<Return<'a>>, Error> {
        self.budget
            .temporary(expressions.len().saturating_mul(16))?;
        let mut scopes = initial;
        let order_sensitive = unordered && expressions.len() > 1;
        for expr in expressions {
            let mut after = BTreeSet::new();
            for scope in scopes {
                self.budget.step()?;
                let returned = self.expression(id, active, expr, scope)?;
                // Native unordered maps are randomized between independent
                // parses. State-leaking arguments cannot be promised retained-
                // input determinism merely because this one map has an order.
                if order_sensitive && returned.iter().any(|result| *result != scope) {
                    return Err(recursive(
                        active,
                        "order-sensitive template argument effects are not allowed",
                    ));
                }
                after.extend(returned);
                self.budget.temporary(after.len().saturating_mul(128))?;
            }
            scopes = after;
        }
        Ok(scopes)
    }
    fn scan_expression(&mut self, id: usize, expr: &'a Expr) -> Result<(), Error> {
        let key = self.bodies[id].key;
        let mut scopes = BTreeSet::from([key.scope]);
        match &expr.val {
            ExprVal::MacroCall(call) => {
                let owner = key.scope.top.map(|block| block.owner).unwrap_or_else(|| {
                    if self.root != self.original {
                        self.root
                    } else {
                        key.active
                    }
                });
                if let Some(&target) = self.caches[key.scope.cache]
                    .get(owner)
                    .and_then(|map| map.get(call.namespace.as_str()))
                    && let Some(definition) = self.tera.templates[target].macros.get(&call.name)
                {
                    // Tera evaluates only declared supplied arguments. Defaults
                    // are literal-only in its pinned parser. Missing required
                    // args stop natively before body dispatch.
                    let mut missing = false;
                    let mut arguments = Vec::new();
                    self.budget
                        .temporary(definition.args.len().saturating_mul(16))?;
                    for (name, default) in &definition.args {
                        if let Some(expr) = call.args.get(name) {
                            arguments.push(expr);
                        } else if default.is_none() {
                            missing = true;
                            break;
                        }
                    }
                    let after_args =
                        self.arguments(id, key.active, arguments, scopes.clone(), true)?;
                    scopes.clear();
                    if !missing {
                        for scope in after_args {
                            scopes.extend(self.call(
                                id,
                                target,
                                &definition.body,
                                scope,
                                1,
                                Some(&call.name),
                            )?);
                        }
                    }
                } else {
                    scopes.clear();
                }
            }
            ExprVal::Math(value) => {
                scopes =
                    self.arguments(id, key.active, vec![&value.lhs, &value.rhs], scopes, false)?
            }
            ExprVal::Logic(value) => {
                scopes =
                    self.arguments(id, key.active, vec![&value.lhs, &value.rhs], scopes, false)?
            }
            ExprVal::In(value) => {
                scopes =
                    self.arguments(id, key.active, vec![&value.lhs, &value.rhs], scopes, false)?
            }
            ExprVal::Test(value) => {
                self.budget.temporary(value.args.len().saturating_mul(16))?;
                scopes =
                    self.arguments(id, key.active, value.args.iter().collect(), scopes, false)?
            }
            ExprVal::FunctionCall(call) => {
                self.budget.temporary(call.args.len().saturating_mul(16))?;
                scopes =
                    self.arguments(id, key.active, call.args.values().collect(), scopes, true)?
            }
            ExprVal::Array(values) => {
                self.budget.temporary(values.len().saturating_mul(16))?;
                scopes = self.arguments(id, key.active, values.iter().collect(), scopes, false)?
            }
            ExprVal::StringConcat(value) => {
                // Unlike a function's unordered argument map, concatenated
                // function calls run left to right and carry returned scope.
                for value in &value.values {
                    self.budget.step()?;
                    if let ExprVal::FunctionCall(call) = value {
                        let mut after = BTreeSet::new();
                        for scope in scopes {
                            after.extend(self.function(id, key.active, call, scope)?);
                            self.budget.temporary(after.len().saturating_mul(128))?;
                        }
                        scopes = after;
                    }
                }
            }
            ExprVal::String(_)
            | ExprVal::Int(_)
            | ExprVal::Float(_)
            | ExprVal::Bool(_)
            | ExprVal::Ident(_) => {}
        }
        for filter in &expr.filters {
            self.budget
                .temporary(filter.args.len().saturating_mul(16))?;
            scopes =
                self.arguments(id, key.active, filter.args.values().collect(), scopes, true)?;
        }
        self.record_returns(id, scopes)
    }
    fn longest_paths(&mut self) -> Result<(), Error> {
        // Iterative postorder memoization counts every longer incoming path to
        // a shared tail; first-visit DFS depth would undercount that case.
        self.budget
            .temporary(self.bodies.len().saturating_mul(16))?;
        let mut colors = vec![0u8; self.bodies.len()];
        let mut heights = vec![0usize; self.bodies.len()];
        for start in 0..self.bodies.len() {
            if colors[start] == 2 {
                continue;
            }
            let mut pending = vec![(start, false)];
            while let Some((id, done)) = pending.pop() {
                self.budget.step()?;
                if done {
                    let body = &self.bodies[id];
                    let height =
                        body.edges
                            .iter()
                            .fold(body.local_depth, |height, (&target, &weight)| {
                                height.max(weight.saturating_add(heights[target]))
                            });
                    if height > self.limit {
                        return Err(depth());
                    }
                    heights[id] = height;
                    colors[id] = 2;
                } else {
                    if colors[id] == 2 {
                        continue;
                    }
                    if colors[id] == 1 {
                        let body = &self.bodies[id];
                        let reason = body
                            .label
                            .map(|name| format!("recursive template macro `{name}` is not allowed"))
                            .unwrap_or_else(|| {
                                "recursive template include/block execution is not allowed"
                                    .to_owned()
                            });
                        return Err(recursive(body.key.active, &reason));
                    }
                    colors[id] = 1;
                    pending.push((id, true));
                    for &target in self.bodies[id].edges.keys().rev() {
                        pending.push((target, false));
                    }
                }
            }
        }
        Ok(())
    }
}
