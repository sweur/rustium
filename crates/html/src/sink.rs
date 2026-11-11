//! The html5ever `TreeSink` that builds a `rustium_dom::Document`.
//!
//! TreeSink methods take `&self` and cannot return errors, so the first
//! failure is recorded in `State` and every later operation becomes a cheap
//! no-op. The sink never panics on parser input.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use html5ever::interface::ElemName;
use html5ever::interface::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode as HtmlQuirks, TreeSink,
};
use html5ever::tendril::{ByteTendril, StrTendril, TendrilSink};
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{
    Attribute, LocalName, Namespace, ParseOpts, QualName, local_name, ns, parse_document,
};
use rustium_dom::{Document, DomError, Namespace as DomNamespace, NodeId, NodeKind, QuirksMode};

use crate::{ParseError, ParseLimits, Parsed};

/// Shared state, owned by `parse` and borrowed by the sink.
pub(crate) struct State {
    doc: RefCell<Document>,
    failure: RefCell<Option<DomError>>,
    parse_errors: Cell<usize>,
    max_attributes: usize,
    /// Name returned for handles that are not real elements (ignored nodes or
    /// anything created after a failure), so `elem_name` never panics.
    fallback: Rc<ElementHandle>,
}

impl State {
    pub(crate) fn new(limits: &ParseLimits) -> Result<State, ParseError> {
        let doc = Document::new(limits.dom).map_err(ParseError::Dom)?;
        let fallback = Rc::new(ElementHandle {
            id: doc.root(),
            name: QualName::new(None, ns!(html), local_name!("div")),
            mathml_annotation_xml_integration_point: false,
        });
        Ok(State {
            doc: RefCell::new(doc),
            failure: RefCell::new(None),
            parse_errors: Cell::new(0),
            max_attributes: limits.dom.max_attributes_per_element,
            fallback,
        })
    }

    pub(crate) fn failed(&self) -> bool {
        self.failure.borrow().is_some()
    }

    fn fail(&self, err: DomError) {
        let mut slot = self.failure.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    /// Runs one document operation unless a failure was already recorded.
    /// Records the failure and returns `None` if the operation fails.
    fn run<T>(&self, op: impl FnOnce(&mut Document) -> Result<T, DomError>) -> Option<T> {
        if self.failed() {
            return None;
        }
        let result = {
            let mut guard = self.doc.borrow_mut();
            op(&mut guard)
        };
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                self.fail(err);
                None
            }
        }
    }

    /// Consumes the state: fails on a recorded error, then checks depth
    /// (subtree moves are not depth-checked one by one) and trims memory.
    pub(crate) fn into_result(self) -> Result<Parsed, ParseError> {
        if let Some(err) = self.failure.into_inner() {
            return Err(ParseError::Dom(err));
        }
        let mut document = self.doc.into_inner();
        document.validate_depth().map_err(ParseError::Dom)?;
        document.shrink_to_fit();
        Ok(Parsed {
            document,
            parse_errors: self.parse_errors.get(),
        })
    }
}

/// Feeds `input` to html5ever in chunks, stopping early after a failure.
pub(crate) fn drive(state: &State, input: &[u8], chunk_bytes: usize) {
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            // JavaScript is disabled, so <noscript> content is parsed as markup.
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parser = parse_document(Sink { state }, opts).from_utf8();
    for chunk in input.chunks(chunk_bytes) {
        parser.process(ByteTendril::from_slice(chunk));
        if state.failed() {
            return;
        }
    }
    parser.finish();
}

#[derive(Debug)]
struct ElementHandle {
    id: NodeId,
    name: QualName,
    mathml_annotation_xml_integration_point: bool,
}

/// What html5ever holds on to for a node. Cheap to clone.
#[derive(Clone)]
enum Handle {
    /// Comments, processing instructions, and anything created after a failure.
    Ignored,
    Document(NodeId),
    Element(Rc<ElementHandle>),
}

impl Handle {
    fn node_id(&self) -> Option<NodeId> {
        match self {
            Handle::Ignored => None,
            Handle::Document(id) => Some(*id),
            Handle::Element(element) => Some(element.id),
        }
    }
}

/// Owned element name returned from `elem_name`.
#[derive(Debug)]
struct ElementName(Rc<ElementHandle>);

impl ElemName for ElementName {
    fn ns(&self) -> &Namespace {
        &self.0.name.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.0.name.local
    }
}

struct Sink<'s> {
    state: &'s State,
}

fn namespace_of(uri: &str) -> DomNamespace {
    // The HTML tree builder only creates elements in these three namespaces.
    match uri {
        "http://www.w3.org/2000/svg" => DomNamespace::Svg,
        "http://www.w3.org/1998/Math/MathML" => DomNamespace::MathMl,
        _ => DomNamespace::Html,
    }
}

/// Attribute names as stored in the DOM: `local`, or `prefix:local`.
fn attribute_names(attrs: &[Attribute]) -> Vec<Cow<'_, str>> {
    attrs
        .iter()
        .map(|attr| {
            let local: &str = &attr.name.local;
            match &attr.name.prefix {
                Some(prefix) if !prefix.is_empty() => Cow::Owned(format!("{prefix}:{local}")),
                _ => Cow::Borrowed(local),
            }
        })
        .collect()
}

fn attribute_pairs<'a>(
    names: &'a [Cow<'_, str>],
    attrs: &'a [Attribute],
) -> Vec<(&'a str, &'a str)> {
    names
        .iter()
        .zip(attrs)
        .map(|(name, attr)| {
            let value: &str = &attr.value;
            (name.as_ref(), value)
        })
        .collect()
}

/// Appends `text` to `existing` if that is a text node (coalescing), otherwise
/// creates a text node and hands it to `attach`.
fn merge_or_create_text(
    doc: &mut Document,
    existing: Option<NodeId>,
    text: &str,
    attach: impl FnOnce(&mut Document, NodeId) -> Result<(), DomError>,
) -> Result<(), DomError> {
    let target = match existing {
        Some(id) if matches!(doc.node(id)?.kind(), NodeKind::Text(_)) => Some(id),
        _ => None,
    };
    match target {
        Some(id) => doc.append_text(id, text),
        None => {
            let created = doc.create_text(text)?;
            attach(doc, created)
        }
    }
}

impl TreeSink for Sink<'_> {
    type Handle = Handle;
    type Output = ();
    type ElemName<'a>
        = ElementName
    where
        Self: 'a;

    fn finish(self) {}

    fn parse_error(&self, _msg: Cow<'static, str>) {
        let count = &self.state.parse_errors;
        count.set(count.get().saturating_add(1));
    }

    fn get_document(&self) -> Handle {
        Handle::Document(self.state.doc.borrow().root())
    }

    fn set_quirks_mode(&self, mode: HtmlQuirks) {
        let mode = match mode {
            HtmlQuirks::Quirks => QuirksMode::Quirks,
            HtmlQuirks::LimitedQuirks => QuirksMode::LimitedQuirks,
            HtmlQuirks::NoQuirks => QuirksMode::NoQuirks,
        };
        self.state.doc.borrow_mut().set_quirks_mode(mode);
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        match (x.node_id(), y.node_id()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> ElementName {
        match target {
            Handle::Element(element) => ElementName(Rc::clone(element)),
            _ => ElementName(Rc::clone(&self.state.fallback)),
        }
    }

    /// Template contents are stored as children of the template element itself.
    fn get_template_contents(&self, target: &Handle) -> Handle {
        target.clone()
    }

    fn is_mathml_annotation_xml_integration_point(&self, target: &Handle) -> bool {
        match target {
            Handle::Element(element) => element.mathml_annotation_xml_integration_point,
            _ => false,
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        if self.state.failed() {
            return Handle::Ignored;
        }
        if attrs.len() > self.state.max_attributes {
            self.state.fail(DomError::AttributeLimitExceeded {
                limit: self.state.max_attributes,
            });
            return Handle::Ignored;
        }
        let tag: &str = &name.local;
        let namespace_uri: &str = &name.ns;
        let namespace = namespace_of(namespace_uri);
        let names = attribute_names(&attrs);
        let pairs = attribute_pairs(&names, &attrs);
        let created = self
            .state
            .run(|doc| doc.create_element_with(namespace, tag, &pairs));
        match created {
            Some(id) => Handle::Element(Rc::new(ElementHandle {
                id,
                name,
                mathml_annotation_xml_integration_point: flags
                    .mathml_annotation_xml_integration_point,
            })),
            None => Handle::Ignored,
        }
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        Handle::Ignored
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        Handle::Ignored
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        let Some(parent_id) = parent.node_id() else {
            return;
        };
        match child {
            NodeOrText::AppendText(text) => {
                self.state.run(|doc| {
                    let last = doc.node(parent_id)?.last_child();
                    merge_or_create_text(doc, last, &text, |doc, id| {
                        doc.append_child(parent_id, id)
                    })
                });
            }
            NodeOrText::AppendNode(node) => {
                let Some(node_id) = node.node_id() else {
                    return;
                };
                self.state.run(|doc| {
                    doc.detach(node_id)?;
                    doc.append_child(parent_id, node_id)
                });
            }
        }
    }

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let Some(sibling_id) = sibling.node_id() else {
            return;
        };
        match child {
            NodeOrText::AppendText(text) => {
                self.state.run(|doc| {
                    let previous = doc.node(sibling_id)?.prev_sibling();
                    merge_or_create_text(doc, previous, &text, |doc, id| {
                        doc.insert_before(sibling_id, id)
                    })
                });
            }
            NodeOrText::AppendNode(node) => {
                let Some(node_id) = node.node_id() else {
                    return;
                };
                self.state.run(|doc| {
                    doc.detach(node_id)?;
                    doc.insert_before(sibling_id, node_id)
                });
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        let has_parent = match element.node_id() {
            Some(id) => self
                .state
                .doc
                .borrow()
                .node(id)
                .map(|node| node.parent().is_some())
                .unwrap_or(false),
            None => false,
        };
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    /// Doctypes are not represented; quirks mode is recorded separately.
    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        let Some(id) = target.node_id() else {
            return;
        };
        if attrs.len() > self.state.max_attributes {
            self.state.fail(DomError::AttributeLimitExceeded {
                limit: self.state.max_attributes,
            });
            return;
        }
        let names = attribute_names(&attrs);
        let pairs = attribute_pairs(&names, &attrs);
        self.state.run(|doc| doc.add_missing_attributes(id, &pairs));
    }

    fn remove_from_parent(&self, target: &Handle) {
        let Some(id) = target.node_id() else {
            return;
        };
        self.state.run(|doc| doc.detach(id));
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let (Some(from), Some(to)) = (node.node_id(), new_parent.node_id()) else {
            return;
        };
        self.state.run(|doc| doc.move_children(from, to));
    }
}
