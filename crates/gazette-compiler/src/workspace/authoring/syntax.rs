//! Recovering TOML structure for exact current editor coordinates.

use std::collections::BTreeSet;

use gazette_core::source::Span;
use toml_parser::parser::{Event, EventKind, RecursionGuard, parse_document};
use toml_parser::{ParseError, Source};

use super::model::{AuthoringError, AuthoringPathSegment};
use crate::toml;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FactKind {
    Key,
    PartialKey,
    HeaderKey,
    Value,
    ValueSlot,
    Table,
    ArrayTable,
}

#[derive(Clone, Debug)]
pub(super) struct Fact {
    pub(super) kind: FactKind,
    pub(super) span: Span,
    pub(super) path: Vec<AuthoringPathSegment>,
}

#[derive(Clone, Debug)]
struct Scope {
    span: Span,
    path: Vec<AuthoringPathSegment>,
    existing_fields: BTreeSet<String>,
}

pub(super) struct SyntaxPosition<'a> {
    pub(super) fact: Option<&'a Fact>,
    pub(super) object_path: &'a [AuthoringPathSegment],
    pub(super) existing_fields: &'a BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub(super) struct Syntax {
    facts: Vec<Fact>,
    scopes: Vec<Scope>,
}

impl Syntax {
    pub(super) fn parse(source: &str, limits: toml::Limits) -> Result<Self, AuthoringError> {
        let parsed = Source::new(source);
        let tokens = parsed.lex().into_vec();
        let mut errors = Vec::<ParseError>::new();
        let mut decode_errors = Vec::<ParseError>::new();
        let mut builder = Builder::new(source.len(), limits);
        {
            let mut receiver = |event: Event| builder.event(&parsed, event, &mut decode_errors);
            let depth = u32::try_from(limits.depth).unwrap_or(u32::MAX);
            let mut guarded = RecursionGuard::new(&mut receiver, depth);
            parse_document(&tokens, &mut guarded, &mut errors);
        }
        builder.finish(source.len())
    }

    pub(super) fn fields(&self) -> impl Iterator<Item = (&Fact, Option<&Fact>)> {
        self.facts
            .iter()
            .enumerate()
            .filter_map(move |(index, fact)| {
                if !matches!(fact.kind, FactKind::Key | FactKind::HeaderKey) {
                    return None;
                }
                let next = self.facts.iter().skip(index + 1).find(|candidate| {
                    candidate.path == fact.path
                        && matches!(candidate.kind, FactKind::Key | FactKind::Value)
                });
                Some((
                    fact,
                    next.filter(|candidate| candidate.kind == FactKind::Value),
                ))
            })
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &Fact> {
        self.facts
            .iter()
            .filter(|fact| fact.kind == FactKind::Value)
    }

    pub(super) fn position(&self, offset: usize) -> SyntaxPosition<'_> {
        let fact = self
            .facts
            .iter()
            .filter(|fact| {
                (fact.span.start == fact.span.end && offset == fact.span.start)
                    || (fact.span.start <= offset && offset < fact.span.end)
                    || (matches!(
                        fact.kind,
                        FactKind::Key
                            | FactKind::PartialKey
                            | FactKind::Value
                            | FactKind::ValueSlot
                    ) && fact.span.end == offset)
            })
            .min_by_key(|fact| {
                (
                    fact.span.end - fact.span.start,
                    std::cmp::Reverse(fact.path.len()),
                )
            });
        let scope = self
            .scopes
            .iter()
            .filter(|scope| {
                (scope.span.start <= offset && offset < scope.span.end) || scope.span.end == offset
            })
            .max_by_key(|scope| (scope.span.start, scope.path.len()))
            .unwrap_or(&self.scopes[0]);
        SyntaxPosition {
            fact,
            object_path: &scope.path,
            existing_fields: &scope.existing_fields,
        }
    }
}

#[derive(Clone, Copy)]
enum HeaderKind {
    Table,
    ArrayTable,
}

struct Header {
    kind: HeaderKind,
    start: usize,
    keys: Vec<(String, Span)>,
    valid: bool,
}

struct PendingSlot {
    start: usize,
    path: Vec<AuthoringPathSegment>,
    observed: bool,
}

#[derive(Clone)]
enum Container {
    Array {
        path: Vec<AuthoringPathSegment>,
    },
    InlineTable {
        path: Vec<AuthoringPathSegment>,
        scope: usize,
    },
}

struct Builder {
    facts: Vec<Fact>,
    scopes: Vec<Scope>,
    current_table: Vec<AuthoringPathSegment>,
    current_scope: usize,
    header: Option<Header>,
    pending_keys: Vec<(String, Span)>,
    pending_key_invalid: bool,
    value_path: Option<Vec<AuthoringPathSegment>>,
    slot: Option<PendingSlot>,
    containers: Vec<Container>,
    array_tables: BTreeSet<Vec<String>>,
    complete: bool,
    limit: usize,
    depth: usize,
}

impl Builder {
    fn new(document_len: usize, limits: toml::Limits) -> Self {
        Self {
            facts: Vec::new(),
            scopes: vec![Scope {
                span: Span {
                    start: 0,
                    end: document_len,
                },
                path: Vec::new(),
                existing_fields: BTreeSet::new(),
            }],
            current_table: Vec::new(),
            current_scope: 0,
            header: None,
            pending_keys: Vec::new(),
            pending_key_invalid: false,
            value_path: None,
            slot: None,
            containers: Vec::new(),
            array_tables: BTreeSet::new(),
            complete: true,
            limit: limits.nodes,
            depth: limits.depth,
        }
    }

    fn event(&mut self, source: &Source<'_>, event: Event, errors: &mut Vec<ParseError>) {
        let native = event.span();
        let span = Span {
            start: native.start(),
            end: native.end(),
        };
        match event.kind() {
            EventKind::StdTableOpen => self.start_header(HeaderKind::Table, span.start),
            EventKind::ArrayTableOpen => self.start_header(HeaderKind::ArrayTable, span.start),
            EventKind::StdTableClose | EventKind::ArrayTableClose => {
                self.finish_header(span.end, span.start != span.end);
            }
            EventKind::SimpleKey => self.key(source, event, span, errors),
            EventKind::KeyValSep => self.start_slot(span.end),
            EventKind::Scalar if span.start != span.end => self.scalar(source, span),
            EventKind::Scalar => {}
            EventKind::ArrayOpen => self.open_array(span),
            EventKind::ArrayClose => self.close_container(false, span.end),
            EventKind::InlineTableOpen => self.open_inline(span),
            EventKind::InlineTableClose => self.close_container(true, span.end),
            EventKind::Newline => {
                if self.header.is_some() {
                    self.finish_header(span.start, false);
                }
                if self.containers.is_empty() {
                    self.finish_slot(span.start);
                    self.finish_partial_key();
                    self.clear_value();
                }
            }
            EventKind::ValueSep => {
                self.finish_slot(span.start);
                self.finish_partial_key();
                self.clear_value();
            }
            EventKind::Error => {
                if self.header.is_some() {
                    self.finish_header(span.end, false);
                }
            }
            EventKind::KeySep | EventKind::Whitespace | EventKind::Comment => {}
        }
    }

    fn key(&mut self, source: &Source<'_>, event: Event, span: Span, errors: &mut Vec<ParseError>) {
        if span.start == span.end {
            self.invalidate_key();
            return;
        }
        let Some(raw) = source.get(event) else {
            return;
        };
        let mut key = String::new();
        let before = errors.len();
        raw.decode_key(&mut key, errors);
        if errors.len() != before {
            self.invalidate_key();
        } else if let Some(header) = self.header.as_mut() {
            if header.keys.len() >= self.depth {
                header.valid = false;
            } else if header.valid {
                header.keys.push((key, span));
            }
        } else if !self.pending_key_invalid {
            if self.pending_keys.len() >= self.depth {
                self.invalidate_key();
            } else {
                self.pending_keys.push((key, span));
            }
        }
    }

    fn start_header(&mut self, kind: HeaderKind, start: usize) {
        self.finish_slot(start);
        self.finish_partial_key();
        self.header = Some(Header {
            kind,
            start,
            keys: Vec::new(),
            valid: true,
        });
        self.clear_value();
        self.containers.clear();
    }

    fn finish_header(&mut self, end: usize, complete: bool) {
        let Some(header) = self.header.take() else {
            return;
        };
        if !complete || !header.valid || header.keys.is_empty() {
            return;
        }
        let raw = header
            .keys
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut path = Vec::new();
        let mut prefix = Vec::new();
        for key in &raw {
            prefix.push(key.clone());
            path.push(AuthoringPathSegment::Field(key.clone()));
            let current =
                matches!(header.kind, HeaderKind::ArrayTable) && prefix.len() == raw.len();
            if self.array_tables.contains(&prefix) || current {
                path.push(AuthoringPathSegment::ArrayItem);
            }
        }
        if path.len() > self.depth {
            self.complete = false;
            return;
        }
        if matches!(header.kind, HeaderKind::ArrayTable) {
            self.array_tables.insert(raw);
        }
        let kind = match header.kind {
            HeaderKind::Table => FactKind::Table,
            HeaderKind::ArrayTable => FactKind::ArrayTable,
        };
        self.push(Fact {
            kind,
            span: Span {
                start: header.start,
                end,
            },
            path: path.clone(),
        });
        for (index, (_, span)) in header.keys.iter().enumerate() {
            self.push(Fact {
                kind: FactKind::HeaderKey,
                span: *span,
                path: path_through_key(&path, index),
            });
        }
        if self.current_scope != 0 {
            self.scopes[self.current_scope].span.end = header.start;
        }
        self.current_table = path.clone();
        if self.scopes.len() >= self.limit {
            self.complete = false;
            return;
        }
        self.scopes.push(Scope {
            span: Span { start: end, end },
            path,
            existing_fields: BTreeSet::new(),
        });
        self.current_scope = self.scopes.len() - 1;
    }

    fn start_slot(&mut self, start: usize) {
        self.finish_slot(start);
        if self.pending_key_invalid || self.pending_keys.is_empty() {
            self.clear_value();
            return;
        }
        let mut path = self
            .containers
            .iter()
            .rev()
            .find_map(|container| match container {
                Container::InlineTable { path, .. } => Some(path.clone()),
                Container::Array { .. } => None,
            })
            .unwrap_or_else(|| self.current_table.clone());
        let pending = std::mem::take(&mut self.pending_keys);
        if path.len().saturating_add(pending.len()) > self.depth {
            self.complete = false;
            self.clear_value();
            return;
        }
        for (key, span) in pending {
            path.push(AuthoringPathSegment::Field(key.clone()));
            self.push(Fact {
                kind: FactKind::Key,
                span,
                path: path.clone(),
            });
        }
        if let Some(AuthoringPathSegment::Field(field)) = path.last() {
            let scope = self
                .containers
                .iter()
                .rev()
                .find_map(|container| match container {
                    Container::InlineTable { scope, .. } => Some(*scope),
                    Container::Array { .. } => None,
                })
                .unwrap_or(self.current_scope);
            self.scopes[scope].existing_fields.insert(field.clone());
        }
        self.value_path = Some(path.clone());
        self.slot = Some(PendingSlot {
            start,
            path,
            observed: false,
        });
        self.pending_key_invalid = false;
    }

    fn scalar(&mut self, source: &Source<'_>, span: Span) {
        if self.recover_inline_key(source, span) {
            return;
        }
        if let Some(slot) = self.slot.as_mut() {
            slot.observed = true;
        }
        if let Some(path) = self.take_target() {
            self.push(Fact {
                kind: FactKind::Value,
                span,
                path,
            });
        }
    }

    fn recover_inline_key(&mut self, source: &Source<'_>, span: Span) -> bool {
        let pending = self.slot.as_ref().is_some_and(|slot| !slot.observed)
            && self
                .containers
                .iter()
                .any(|container| matches!(container, Container::InlineTable { .. }));
        if !pending {
            return false;
        }
        let Some(raw) = source.input().get(span.start..span.end) else {
            return false;
        };
        let bare = !raw.is_empty()
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        let separator = source
            .input()
            .get(span.end..)
            .is_some_and(|suffix| suffix.trim_start().starts_with('='));
        if !bare || !separator {
            return false;
        }
        self.finish_slot(span.start);
        self.pending_keys.push((raw.to_owned(), span));
        true
    }

    fn open_array(&mut self, span: Span) {
        if span.start == span.end {
            return;
        }
        if let Some(slot) = self.slot.as_mut() {
            slot.observed = true;
        }
        if let Some(mut path) = self.take_target() {
            path.push(AuthoringPathSegment::ArrayItem);
            self.containers.push(Container::Array { path });
        }
    }

    fn open_inline(&mut self, span: Span) {
        if span.start == span.end {
            return;
        }
        if let Some(slot) = self.slot.as_mut() {
            slot.observed = true;
        }
        if let Some(path) = self.take_target() {
            if self.scopes.len() >= self.limit {
                self.complete = false;
                return;
            }
            self.scopes.push(Scope {
                span: Span {
                    start: span.end,
                    end: span.end,
                },
                path: path.clone(),
                existing_fields: BTreeSet::new(),
            });
            let scope = self.scopes.len() - 1;
            self.containers.push(Container::InlineTable { path, scope });
        }
    }

    fn take_target(&mut self) -> Option<Vec<AuthoringPathSegment>> {
        self.value_path.take().or_else(|| {
            self.containers
                .iter()
                .rev()
                .find_map(|container| match container {
                    Container::Array { path } => Some(path.clone()),
                    Container::InlineTable { .. } => None,
                })
        })
    }

    fn close_container(&mut self, inline: bool, end: usize) {
        self.finish_slot(end);
        let matches = if inline {
            matches!(self.containers.last(), Some(Container::InlineTable { .. }))
        } else {
            matches!(self.containers.last(), Some(Container::Array { .. }))
        };
        if matches {
            if let Some(Container::InlineTable { scope, .. }) = self.containers.last() {
                self.scopes[*scope].span.end = end;
            }
            self.containers.pop();
        }
        self.value_path = None;
    }

    fn finish_slot(&mut self, end: usize) {
        if let Some(PendingSlot {
            start,
            path,
            observed: false,
        }) = self.slot.take()
        {
            self.push(Fact {
                kind: FactKind::ValueSlot,
                span: Span {
                    start,
                    end: end.max(start),
                },
                path,
            });
        }
    }

    fn finish_partial_key(&mut self) {
        if self.pending_key_invalid || self.pending_keys.is_empty() {
            self.pending_keys.clear();
            return;
        }
        let mut path = self
            .containers
            .iter()
            .rev()
            .find_map(|container| match container {
                Container::InlineTable { path, .. } => Some(path.clone()),
                Container::Array { .. } => None,
            })
            .unwrap_or_else(|| self.current_table.clone());
        let pending = std::mem::take(&mut self.pending_keys);
        let last = pending.len().saturating_sub(1);
        for (index, (key, span)) in pending.into_iter().enumerate() {
            path.push(AuthoringPathSegment::Field(key));
            if index == last {
                self.push(Fact {
                    kind: FactKind::PartialKey,
                    span,
                    path: path.clone(),
                });
            }
        }
    }

    fn invalidate_key(&mut self) {
        if let Some(header) = self.header.as_mut() {
            header.valid = false;
            header.keys.clear();
        } else {
            self.pending_key_invalid = true;
            self.pending_keys.clear();
        }
    }

    fn clear_value(&mut self) {
        self.pending_keys.clear();
        self.pending_key_invalid = false;
        self.value_path = None;
        self.slot = None;
    }

    fn finish(mut self, length: usize) -> Result<Syntax, AuthoringError> {
        self.finish_slot(length);
        self.finish_partial_key();
        if self.current_scope != 0 {
            self.scopes[self.current_scope].span.end = length;
        }
        self.facts
            .sort_by_key(|fact| (fact.span.start, fact.span.end, fact_order(fact.kind)));
        if !self.complete {
            return Err(AuthoringError::Limit("structural facts"));
        }
        Ok(Syntax {
            facts: self.facts,
            scopes: self.scopes,
        })
    }

    fn push(&mut self, fact: Fact) {
        if self.facts.len() < self.limit {
            self.facts.push(fact);
        } else {
            self.complete = false;
        }
    }
}

fn path_through_key(path: &[AuthoringPathSegment], key_index: usize) -> Vec<AuthoringPathSegment> {
    let desired = key_index + 1;
    let mut seen = 0;
    path.iter()
        .take_while(|segment| {
            if matches!(segment, AuthoringPathSegment::Field(_)) {
                seen += 1;
            }
            seen <= desired
        })
        .cloned()
        .collect()
}

const fn fact_order(kind: FactKind) -> u8 {
    match kind {
        FactKind::Key | FactKind::PartialKey | FactKind::HeaderKey => 0,
        FactKind::Value => 1,
        FactKind::Table => 2,
        FactKind::ArrayTable => 3,
        FactKind::ValueSlot => 4,
    }
}
