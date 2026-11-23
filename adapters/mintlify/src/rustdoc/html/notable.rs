//! Turn Rustdoc's literal notable-trait tooltips into checked, linked static sections.

use super::{Node, State, Tag, attribute, label, state, tokenize};
use crate::Error;
use std::collections::BTreeMap;

impl State<'_> {
    pub(super) fn notable_link(&mut self, tag: &Tag) -> Result<Option<String>, Error> {
        let Some(key) = attribute(tag, "data-notable-ty").filter(|_| self.inside) else {
            return Ok(None);
        };
        if self.fragment
            || self.notable_complete
            || tag.name.as_ref() != "a"
            || attribute(tag, "href") != Some("#")
            || attribute(tag, "class") != Some("tooltip")
            || tag.attrs.len() != 3
            || !label(key)
        {
            return Err(self.fail("unqualified Rustdoc notable-trait link"));
        }
        let next = format!("rustdoc-notable-traits-{}", self.notable.len());
        Ok(Some(
            self.notable.entry(key.to_owned()).or_insert(next).clone(),
        ))
    }

    pub(super) fn notable_data(&mut self, tag: &Tag) -> Result<bool, Error> {
        let name = tag.name.as_ref();
        if self.fragment && matches!(name, "script" | "h1" | "body" | "main") {
            return Err(self.fail("unsupported Rustdoc notable-trait fragment"));
        }
        if !self.inside || name != "script" || attribute(tag, "id") != Some("notable-traits-data") {
            return Ok(false);
        }
        if self.notable_json.is_some()
            || self.notable_complete
            || tag.attrs.len() != 2
            || attribute(tag, "type") != Some("text/json")
        {
            return Err(self.fail("unqualified Rustdoc notable-trait data envelope"));
        }
        self.notable_json = Some(String::new());
        Ok(true)
    }

    pub(super) fn expand_notable(&mut self) -> Result<(), Error> {
        let Some(literal) = self.notable_json.take() else {
            return Err(self.fail("missing Rustdoc notable-trait data"));
        };
        let value = gazette_core::json::parse(literal.as_bytes())
            .map_err(|_| self.fail("invalid Rustdoc notable-trait JSON"))?;
        let entries = value
            .as_object()
            .ok_or_else(|| self.fail("invalid Rustdoc notable-trait map"))?;
        if entries.len() != self.notable.len()
            || entries.keys().any(|key| !self.notable.contains_key(key))
        {
            return Err(self.fail("Rustdoc notable-trait links and data disagree"));
        }
        for (key, value) in entries {
            let html = value
                .as_str()
                .filter(|html| !html.is_empty())
                .ok_or_else(|| self.fail("invalid Rustdoc notable-trait fragment"))?;
            let mut fragment = state(self.context, self.maximum);
            fragment.inside = true;
            fragment.fragment = true;
            let fragment = tokenize(html, fragment)?;
            // Rustdoc supplies an HTML fragment whose final pre/code tags are open.
            // The common tokenizer closes only that exact pair, retaining all nodes.
            self.charge(fragment.decoded)?;
            self.tokens = self
                .tokens
                .checked_add(fragment.tokens)
                .filter(|count| *count <= self.maximum.min(250_000))
                .ok_or_else(|| self.fail("Rustdoc token ceiling exceeded"))?;
            let anchor = self.notable[key].clone();
            if !self.page.anchors.insert(anchor.clone())
                || fragment
                    .page
                    .anchors
                    .iter()
                    .any(|id| !self.page.anchors.insert(id.clone()))
            {
                return Err(self.fail("duplicate Rustdoc notable-trait anchor"));
            }
            self.page.nodes.push(Node::Start {
                name: "div".into(),
                attributes: BTreeMap::from([("id".into(), anchor)]),
            });
            self.page.nodes.extend(fragment.page.nodes);
            self.page.nodes.push(Node::End("div".into()));
            self.page.hrefs.extend(fragment.page.hrefs);
        }
        self.notable_complete = true;
        Ok(())
    }
}
