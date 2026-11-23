//! Bounded storage for html5ever's native fragment tree; no HTML parsing policy.

use gazette_core::markdown::{Error, Limits};
use html5ever::{
    Attribute, QualName, local_name, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

pub(super) type Handle = Rc<Node>;
pub(super) struct Node {
    pub(super) kind: Kind,
    parent: RefCell<Option<Weak<Node>>>,
    pub(super) children: RefCell<Vec<Handle>>,
}
pub(super) enum Kind {
    Document,
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        template: Option<Handle>,
        integration: bool,
    },
    Text(RefCell<StrTendril>),
    Comment(StrTendril),
    Pi(StrTendril, StrTendril),
}
impl Node {
    fn new(kind: Kind) -> Handle {
        Rc::new(Self {
            kind,
            parent: RefCell::new(None),
            children: RefCell::new(Vec::new()),
        })
    }
}
pub(super) struct Tree {
    pub(super) root: Handle,
}

pub(super) fn parse(html: &str, limits: Limits) -> Result<Tree, Error> {
    if html.len() > limits.output_bytes {
        return Err(Error::Limit("HTML transport input bytes"));
    }
    let sink = Sink {
        document: Node::new(Kind::Document),
        poison: Node::new(Kind::Element {
            name: QualName::new(None, ns!(html), local_name!("div")),
            attrs: RefCell::new(Vec::new()),
            template: None,
            integration: false,
        }),
        failure: RefCell::new(None),
        nodes: Cell::new(2),
        bytes: Cell::new(limits.fact_bytes),
        limits,
    };
    let mut parser = html5ever::parse_fragment(
        sink,
        html5ever::ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("body")),
        Vec::new(),
        true,
    );
    let mut start = 0;
    while start < html.len() {
        let mut end = (start + 256).min(html.len());
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        parser.process(StrTendril::from(&html[start..end]));
        if let Some(error) = parser.tokenizer.sink.sink.failure.borrow().clone() {
            return Err(error);
        }
        start = end;
    }
    let sink = parser.finish();
    if let Some(error) = sink.failure.into_inner() {
        return Err(error);
    }
    let root = sink
        .document
        .children
        .borrow()
        .first()
        .cloned()
        .ok_or(Error::Formatting)?;
    check_depth(&root, 0, limits.depth)?;
    Ok(Tree { root })
}
fn check_depth(node: &Handle, depth: usize, maximum: usize) -> Result<(), Error> {
    if depth > maximum {
        return Err(Error::Limit("HTML transport depth"));
    }
    for child in node.children.borrow().iter() {
        check_depth(child, depth + 1, maximum)?;
    }
    if let Kind::Element {
        template: Some(template),
        ..
    } = &node.kind
    {
        check_depth(template, depth + 1, maximum)?;
    }
    Ok(())
}
struct Sink {
    document: Handle,
    poison: Handle,
    failure: RefCell<Option<Error>>,
    nodes: Cell<usize>,
    bytes: Cell<usize>,
    limits: Limits,
}
impl Sink {
    fn fail(&self, error: Error) {
        if self.failure.borrow().is_none() {
            *self.failure.borrow_mut() = Some(error);
        }
    }
    fn charge(&self, bytes: usize) -> bool {
        if self.failure.borrow().is_some() {
            return false;
        }
        if let Some(remaining) = self.bytes.get().checked_sub(bytes) {
            self.bytes.set(remaining);
            true
        } else {
            self.fail(Error::Limit("HTML transport decoded bytes"));
            false
        }
    }
    fn allocate(&self, kind: Kind) -> Handle {
        if self.failure.borrow().is_some() {
            return self.poison.clone();
        }
        if self.nodes.get() >= self.limits.nodes {
            self.fail(Error::Limit("HTML transport nodes"));
            return self.poison.clone();
        }
        self.nodes.set(self.nodes.get() + 1);
        Node::new(kind)
    }
    fn detach(&self, node: &Handle) {
        if let Some(parent) = node.parent.borrow_mut().take().and_then(|p| p.upgrade()) {
            parent
                .children
                .borrow_mut()
                .retain(|child| !Rc::ptr_eq(child, node));
        }
    }
    fn insert(&self, parent: &Handle, index: Option<usize>, child: NodeOrText<Handle>) {
        if self.failure.borrow().is_some() {
            return;
        }
        let mut ancestor = Some(parent.clone());
        let mut depth = 0;
        while let Some(node) = ancestor {
            depth += 1;
            if depth > self.limits.depth {
                self.fail(Error::Limit("HTML transport depth"));
                return;
            }
            ancestor = node.parent.borrow().as_ref().and_then(Weak::upgrade);
        }
        let child = match child {
            NodeOrText::AppendNode(node) => {
                self.detach(&node);
                node
            }
            NodeOrText::AppendText(text) => {
                if !self.charge(text.len()) {
                    return;
                }
                let previous = {
                    let children = parent.children.borrow();
                    let end = index.unwrap_or(children.len());
                    end.checked_sub(1).and_then(|i| children.get(i)).cloned()
                };
                if let Some(previous) = previous
                    && let Kind::Text(value) = &previous.kind
                {
                    value.borrow_mut().push_tendril(&text);
                    return;
                }
                self.allocate(Kind::Text(RefCell::new(text)))
            }
        };
        if self.failure.borrow().is_some() {
            return;
        }
        *child.parent.borrow_mut() = Some(Rc::downgrade(parent));
        let mut children = parent.children.borrow_mut();
        if let Some(index) = index {
            let index = index.min(children.len());
            children.insert(index, child);
        } else {
            children.push(child);
        }
    }
}
impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a> = &'a QualName;
    fn finish(self) -> Self {
        self
    }
    fn parse_error(&self, _: Cow<'static, str>) {
        self.fail(Error::Formatting);
    }
    fn get_document(&self) -> Handle {
        self.document.clone()
    }
    fn elem_name<'a>(&'a self, node: &'a Handle) -> &'a QualName {
        match &node.kind {
            Kind::Element { name, .. } => name,
            _ => match &self.poison.kind {
                Kind::Element { name, .. } => name,
                _ => unreachable!(),
            },
        }
    }
    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        // Names/namespaces are native interned atoms, not repeated owned
        // decoded strings. Attribute/text payloads have their own fact budget;
        // tree fixed overhead is bounded by nodes rather than output bytes.
        let bytes = attrs.iter().map(|a| a.value.len()).sum::<usize>();
        if !self.charge(bytes) {
            return self.poison.clone();
        }
        let template = flags.template.then(|| self.allocate(Kind::Document));
        self.allocate(Kind::Element {
            name,
            attrs: RefCell::new(attrs),
            template,
            integration: flags.mathml_annotation_xml_integration_point,
        })
    }
    fn create_comment(&self, text: StrTendril) -> Handle {
        if !self.charge(text.len()) {
            return self.poison.clone();
        }
        self.allocate(Kind::Comment(text))
    }
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Handle {
        if !self.charge(target.len() + data.len()) {
            return self.poison.clone();
        }
        self.allocate(Kind::Pi(target, data))
    }
    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        self.insert(parent, None, child);
    }
    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        previous: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if element
            .parent
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some()
        {
            self.append_before_sibling(element, child);
        } else {
            self.append(previous, child);
        }
    }
    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {
        self.fail(Error::Formatting);
    }
    fn get_template_contents(&self, node: &Handle) -> Handle {
        match &node.kind {
            Kind::Element {
                template: Some(template),
                ..
            } => template.clone(),
            _ => self.poison.clone(),
        }
    }
    fn same_node(&self, left: &Handle, right: &Handle) -> bool {
        Rc::ptr_eq(left, right)
    }
    fn set_quirks_mode(&self, _: QuirksMode) {}
    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        if let Some(parent) = sibling.parent.borrow().as_ref().and_then(Weak::upgrade) {
            let index = parent
                .children
                .borrow()
                .iter()
                .position(|node| Rc::ptr_eq(node, sibling));
            self.insert(&parent, index, child);
        } else {
            self.fail(Error::Formatting);
        }
    }
    fn add_attrs_if_missing(&self, node: &Handle, additions: Vec<Attribute>) {
        if let Kind::Element { attrs, .. } = &node.kind {
            let mut attrs = attrs.borrow_mut();
            for attr in additions {
                if !attrs.iter().any(|existing| existing.name == attr.name)
                    && self.charge(attr.value.len())
                {
                    attrs.push(attr);
                }
            }
        }
    }
    fn remove_from_parent(&self, node: &Handle) {
        self.detach(node);
    }
    fn reparent_children(&self, node: &Handle, parent: &Handle) {
        let children = std::mem::take(&mut *node.children.borrow_mut());
        for child in children {
            *child.parent.borrow_mut() = None;
            self.append(parent, NodeOrText::AppendNode(child));
        }
    }
    fn is_mathml_annotation_xml_integration_point(&self, node: &Handle) -> bool {
        matches!(
            node.kind,
            Kind::Element {
                integration: true,
                ..
            }
        )
    }
}
