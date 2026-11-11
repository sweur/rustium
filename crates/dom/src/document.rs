use crate::error::DomError;
use crate::iter::{Children, Descendants};
use crate::node::{ElementData, Namespace, Node, NodeId, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuirksMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// Per-document resource limits. `max_nodes` counts allocated slots
/// (including the root and detached nodes), not just attached nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_nodes: usize,
    /// Maximum depth; the root has depth 0. Exact for inserts of childless
    /// nodes; subtree moves are checked by `Document::validate_depth`.
    pub max_depth: usize,
    /// Total bytes of tag names, text and attributes across the document.
    pub max_string_bytes: usize,
    pub max_attributes_per_element: usize,
}

impl Limits {
    /// Ids are `index + 1` in a `u32`, so the node count must stay below this.
    pub const MAX_NODES_CEILING: usize = (u32::MAX - 1) as usize;

    /// Fixed cost charged to the string budget per attribute, on top of the
    /// bytes of its name and value (rough allowance for per-attribute heap data).
    pub const ATTRIBUTE_OVERHEAD_BYTES: usize = 32;

    pub fn validate(&self) -> Result<(), DomError> {
        if self.max_nodes == 0 || self.max_nodes > Self::MAX_NODES_CEILING {
            Err(DomError::InvalidLimits)
        } else {
            Ok(())
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_nodes: 50_000,
            max_depth: 256,
            max_string_bytes: 4 * 1024 * 1024,
            max_attributes_per_element: 256,
        }
    }
}

/// Arena-allocated document tree. Index 0 is the root. Nodes are never freed
/// before the document is dropped; detached nodes stay allocated and count
/// against `max_nodes`.
#[derive(Debug)]
pub struct Document {
    nodes: Vec<Node>,
    limits: Limits,
    string_bytes: usize,
    quirks_mode: QuirksMode,
}

impl Document {
    pub fn new(limits: Limits) -> Result<Document, DomError> {
        limits.validate()?;
        Ok(Document {
            nodes: vec![Node::new(NodeKind::Document)],
            limits,
            string_bytes: 0,
            quirks_mode: QuirksMode::NoQuirks,
        })
    }

    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    /// Number of allocated nodes, including the root and detached nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DomError> {
        self.get(id).ok_or(DomError::InvalidNodeId)
    }

    pub fn quirks_mode(&self) -> QuirksMode {
        self.quirks_mode
    }

    pub fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.quirks_mode = mode;
    }

    /// Creates a detached HTML element with no attributes.
    pub fn create_element(&mut self, tag: &str) -> Result<NodeId, DomError> {
        self.create_element_with(Namespace::Html, tag, &[])
    }

    /// Creates a detached element. All limits are checked before allocating.
    pub fn create_element_with(
        &mut self,
        namespace: Namespace,
        tag: &str,
        attributes: &[(&str, &str)],
    ) -> Result<NodeId, DomError> {
        if attributes.len() > self.limits.max_attributes_per_element {
            return Err(DomError::AttributeLimitExceeded {
                limit: self.limits.max_attributes_per_element,
            });
        }
        let mut bytes = tag.len();
        for (name, value) in attributes {
            bytes = bytes
                .checked_add(name.len())
                .and_then(|b| b.checked_add(value.len()))
                .and_then(|b| b.checked_add(Limits::ATTRIBUTE_OVERHEAD_BYTES))
                .ok_or(DomError::StringBudgetExceeded {
                    limit: self.limits.max_string_bytes,
                })?;
        }
        let total = self.check_room(bytes)?;
        let data = ElementData::new(namespace, tag, attributes);
        self.push_node(NodeKind::Element(Box::new(data)), total)
    }

    pub fn create_text(&mut self, text: &str) -> Result<NodeId, DomError> {
        let total = self.check_room(text.len())?;
        self.push_node(NodeKind::Text(String::from(text)), total)
    }

    /// Appends to an existing text node (used to coalesce adjacent text).
    pub fn append_text(&mut self, node: NodeId, text: &str) -> Result<(), DomError> {
        if !matches!(self.node(node)?.kind, NodeKind::Text(_)) {
            return Err(DomError::NotText);
        }
        let total = self
            .string_bytes
            .checked_add(text.len())
            .filter(|total| *total <= self.limits.max_string_bytes)
            .ok_or(DomError::StringBudgetExceeded {
                limit: self.limits.max_string_bytes,
            })?;
        match &mut self.node_mut(node)?.kind {
            NodeKind::Text(existing) => existing.push_str(text),
            _ => return Err(DomError::NotText),
        }
        self.string_bytes = total;
        Ok(())
    }

    /// Adds the attributes whose names the element does not have yet (the first
    /// occurrence of a name wins). All limits are checked before any change.
    pub fn add_missing_attributes(
        &mut self,
        node: NodeId,
        attributes: &[(&str, &str)],
    ) -> Result<(), DomError> {
        let NodeKind::Element(element) = &self.node(node)?.kind else {
            return Err(DomError::NotElement);
        };
        let mut to_add: Vec<(&str, &str)> = Vec::new();
        for &(name, value) in attributes {
            if element.attribute(name).is_none() && !to_add.iter().any(|(n, _)| *n == name) {
                to_add.push((name, value));
            }
        }
        if to_add.is_empty() {
            return Ok(());
        }
        let limit = self.limits.max_attributes_per_element;
        if element.attributes().len().saturating_add(to_add.len()) > limit {
            return Err(DomError::AttributeLimitExceeded { limit });
        }
        let mut bytes = 0usize;
        for (name, value) in &to_add {
            bytes = bytes
                .checked_add(name.len())
                .and_then(|b| b.checked_add(value.len()))
                .and_then(|b| b.checked_add(Limits::ATTRIBUTE_OVERHEAD_BYTES))
                .ok_or(DomError::StringBudgetExceeded {
                    limit: self.limits.max_string_bytes,
                })?;
        }
        let total = self
            .string_bytes
            .checked_add(bytes)
            .filter(|total| *total <= self.limits.max_string_bytes)
            .ok_or(DomError::StringBudgetExceeded {
                limit: self.limits.max_string_bytes,
            })?;
        match &mut self.node_mut(node)?.kind {
            NodeKind::Element(element) => element.push_attributes(&to_add),
            _ => return Err(DomError::NotElement),
        }
        self.string_bytes = total;
        Ok(())
    }

    /// Appends a detached `child` (with or without descendants) as the last
    /// child of `parent`. All checks run before any mutation.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_node = self.node(parent)?;
        let child_node = self.node(child)?;

        if parent == child || !matches!(parent_node.kind, NodeKind::Document | NodeKind::Element(_))
        {
            return Err(DomError::InvalidParent);
        }
        if child == NodeId::ROOT || child_node.parent.is_some() {
            return Err(DomError::AlreadyAttached);
        }
        let child_is_leaf = child_node.first_child.is_none();
        if !child_is_leaf && self.is_ancestor_or_self(child, parent) {
            return Err(DomError::HierarchyCycle);
        }
        if child_is_leaf && self.depth_of(parent) + 1 > self.limits.max_depth {
            return Err(DomError::DepthLimitExceeded {
                limit: self.limits.max_depth,
            });
        }
        let last_child = parent_node.last_child;

        match last_child {
            Some(last) => self.node_mut(last)?.next_sibling = Some(child),
            None => self.node_mut(parent)?.first_child = Some(child),
        }
        self.node_mut(parent)?.last_child = Some(child);
        let child_mut = self.node_mut(child)?;
        child_mut.parent = Some(parent);
        child_mut.prev_sibling = last_child;
        Ok(())
    }

    /// Inserts a detached `node` immediately before `reference`.
    pub fn insert_before(&mut self, reference: NodeId, node: NodeId) -> Result<(), DomError> {
        let reference_node = self.node(reference)?;
        let new_node = self.node(node)?;

        let Some(parent) = reference_node.parent else {
            return Err(DomError::NotAttached);
        };
        if node == NodeId::ROOT || new_node.parent.is_some() {
            return Err(DomError::AlreadyAttached);
        }
        let node_is_leaf = new_node.first_child.is_none();
        if !node_is_leaf && self.is_ancestor_or_self(node, parent) {
            return Err(DomError::HierarchyCycle);
        }
        if node_is_leaf && self.depth_of(parent) + 1 > self.limits.max_depth {
            return Err(DomError::DepthLimitExceeded {
                limit: self.limits.max_depth,
            });
        }
        let prev = reference_node.prev_sibling;

        match prev {
            Some(p) => self.node_mut(p)?.next_sibling = Some(node),
            None => self.node_mut(parent)?.first_child = Some(node),
        }
        self.node_mut(reference)?.prev_sibling = Some(node);
        let node_mut = self.node_mut(node)?;
        node_mut.parent = Some(parent);
        node_mut.prev_sibling = prev;
        node_mut.next_sibling = Some(reference);
        Ok(())
    }

    /// Unlinks a node (and its subtree) from its parent. A node without a
    /// parent is left alone. The node stays allocated.
    pub fn detach(&mut self, node: NodeId) -> Result<(), DomError> {
        let current = self.node(node)?;
        let (Some(parent), prev, next) =
            (current.parent, current.prev_sibling, current.next_sibling)
        else {
            return Ok(());
        };

        match prev {
            Some(p) => self.node_mut(p)?.next_sibling = next,
            None => self.node_mut(parent)?.first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n)?.prev_sibling = prev,
            None => self.node_mut(parent)?.last_child = prev,
        }
        let detached = self.node_mut(node)?;
        detached.parent = None;
        detached.prev_sibling = None;
        detached.next_sibling = None;
        Ok(())
    }

    /// Moves all children of `from` to the end of `to`'s child list, in order.
    /// O(number of children moved). Depth is not checked here; call
    /// `validate_depth` after a batch of moves.
    pub fn move_children(&mut self, from: NodeId, to: NodeId) -> Result<(), DomError> {
        let from_node = self.node(from)?;
        let to_node = self.node(to)?;

        if from == to || !matches!(to_node.kind, NodeKind::Document | NodeKind::Element(_)) {
            return Err(DomError::InvalidParent);
        }
        let (Some(first), Some(last)) = (from_node.first_child, from_node.last_child) else {
            return Ok(());
        };
        if self.is_ancestor_or_self(from, to) {
            return Err(DomError::HierarchyCycle);
        }
        let to_last = to_node.last_child;

        let mut cursor = Some(first);
        while let Some(child) = cursor {
            let node = self.node_mut(child)?;
            node.parent = Some(to);
            cursor = node.next_sibling;
        }
        match to_last {
            Some(tail) => self.node_mut(tail)?.next_sibling = Some(first),
            None => self.node_mut(to)?.first_child = Some(first),
        }
        self.node_mut(first)?.prev_sibling = to_last;
        self.node_mut(to)?.last_child = Some(last);
        let from_mut = self.node_mut(from)?;
        from_mut.first_child = None;
        from_mut.last_child = None;
        Ok(())
    }

    /// Direct children of `id`, in order.
    pub fn children(&self, id: NodeId) -> Result<Children<'_>, DomError> {
        let first = self.node(id)?.first_child;
        Ok(Children::new(self, first))
    }

    /// Proper descendants of `id` (not `id` itself) in pre-order.
    /// Iterative and stackless: uses parent links, no recursion.
    pub fn descendants(&self, id: NodeId) -> Result<Descendants<'_>, DomError> {
        let first = self.node(id)?.first_child;
        Ok(Descendants::new(self, id, first))
    }

    /// Checks that every node reachable from the root is within `max_depth`.
    /// One iterative O(n) pass. Detached subtrees are not part of the document
    /// and are not checked.
    pub fn validate_depth(&self) -> Result<(), DomError> {
        let max = self.limits.max_depth;
        let mut depth = 0usize;
        let mut cursor = NodeId::ROOT;
        loop {
            let node = self.get(cursor).ok_or(DomError::InvalidNodeId)?;
            if let Some(child) = node.first_child {
                depth += 1;
                if depth > max {
                    return Err(DomError::DepthLimitExceeded { limit: max });
                }
                cursor = child;
                continue;
            }
            // Leaf: climb until a node with a next sibling is found.
            loop {
                if cursor == NodeId::ROOT {
                    return Ok(());
                }
                let current = self.get(cursor).ok_or(DomError::InvalidNodeId)?;
                if let Some(sibling) = current.next_sibling {
                    cursor = sibling;
                    break;
                }
                match current.parent {
                    Some(parent) => {
                        cursor = parent;
                        depth = depth.saturating_sub(1);
                    }
                    None => return Ok(()),
                }
            }
        }
    }

    /// Releases unused capacity (text slack and spare node slots).
    pub fn shrink_to_fit(&mut self) {
        for node in &mut self.nodes {
            if let NodeKind::Text(text) = &mut node.kind {
                text.shrink_to_fit();
            }
        }
        self.nodes.shrink_to_fit();
    }

    pub(crate) fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, DomError> {
        self.nodes
            .get_mut(id.index())
            .ok_or(DomError::InvalidNodeId)
    }

    /// Checks node and string limits BEFORE anything is allocated.
    /// Returns the new total of string bytes on success.
    fn check_room(&self, string_len: usize) -> Result<usize, DomError> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(DomError::NodeLimitExceeded {
                limit: self.limits.max_nodes,
            });
        }
        self.string_bytes
            .checked_add(string_len)
            .filter(|total| *total <= self.limits.max_string_bytes)
            .ok_or(DomError::StringBudgetExceeded {
                limit: self.limits.max_string_bytes,
            })
    }

    fn push_node(&mut self, kind: NodeKind, new_string_total: usize) -> Result<NodeId, DomError> {
        let id = NodeId::from_index(self.nodes.len()).ok_or(DomError::NodeLimitExceeded {
            limit: self.limits.max_nodes,
        })?;
        self.nodes.push(Node::new(kind));
        self.string_bytes = new_string_total;
        Ok(id)
    }

    /// Depth of `id` (topmost ancestor = 0), by walking parent links. Capped at
    /// `max_depth + 1` steps so an over-deep or corrupted chain cannot loop long.
    fn depth_of(&self, id: NodeId) -> usize {
        let mut depth = 0usize;
        let mut cursor = self.get(id).and_then(|n| n.parent);
        while let Some(p) = cursor {
            depth += 1;
            if depth > self.limits.max_depth {
                break;
            }
            cursor = self.get(p).and_then(|n| n.parent);
        }
        depth
    }

    /// True if `candidate` is `start` or one of its ancestors. Fails closed
    /// (returns true) if the chain is longer than the node count.
    fn is_ancestor_or_self(&self, candidate: NodeId, start: NodeId) -> bool {
        let mut cursor = Some(start);
        let mut steps = 0usize;
        while let Some(id) = cursor {
            if id == candidate {
                return true;
            }
            steps += 1;
            if steps > self.nodes.len() {
                return true;
            }
            cursor = self.get(id).and_then(|n| n.parent);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(max_nodes: usize, max_depth: usize, max_string_bytes: usize) -> Document {
        Document::new(Limits {
            max_nodes,
            max_depth,
            max_string_bytes,
            ..Limits::default()
        })
        .unwrap()
    }

    fn attach(doc: &mut Document, parent: NodeId, tag: &str) -> NodeId {
        let id = doc.create_element(tag).unwrap();
        doc.append_child(parent, id).unwrap();
        id
    }

    fn tag_of(doc: &Document, id: NodeId) -> String {
        match doc.node(id).unwrap().kind() {
            NodeKind::Element(el) => el.tag().to_string(),
            other => panic!("expected an element, found {other:?}"),
        }
    }

    fn text_of(doc: &Document, id: NodeId) -> String {
        match doc.node(id).unwrap().kind() {
            NodeKind::Text(text) => text.clone(),
            other => panic!("expected text, found {other:?}"),
        }
    }

    #[test]
    fn empty_document_has_only_the_root() {
        let doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        assert_eq!(doc.node_count(), 1);
        let node = doc.node(root).unwrap();
        assert!(matches!(node.kind(), NodeKind::Document));
        assert_eq!(node.parent(), None);
        assert_eq!(node.first_child(), None);
        assert_eq!(doc.children(root).unwrap().count(), 0);
        assert_eq!(doc.descendants(root).unwrap().count(), 0);
    }

    #[test]
    fn limits_are_validated() {
        assert_eq!(Limits::default().validate(), Ok(()));
        let zero = Limits {
            max_nodes: 0,
            ..Limits::default()
        };
        assert_eq!(zero.validate(), Err(DomError::InvalidLimits));
        let huge = Limits {
            max_nodes: Limits::MAX_NODES_CEILING + 1,
            ..Limits::default()
        };
        assert_eq!(huge.validate(), Err(DomError::InvalidLimits));
        assert_eq!(Document::new(zero).unwrap_err(), DomError::InvalidLimits);
    }

    #[test]
    fn creates_detached_elements_and_text() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let el = doc.create_element("div").unwrap();
        let text = doc.create_text("hi").unwrap();
        assert_eq!(doc.node_count(), 3);
        assert_eq!(tag_of(&doc, el), "div");
        assert_eq!(text_of(&doc, text), "hi");
        match doc.node(el).unwrap().kind() {
            NodeKind::Element(data) => {
                assert_eq!(data.namespace(), Namespace::Html);
                assert!(data.attributes().is_empty());
            }
            other => panic!("expected an element, found {other:?}"),
        }
        assert_eq!(doc.node(el).unwrap().parent(), None);
        assert_eq!(doc.node(text).unwrap().parent(), None);
    }

    #[test]
    fn append_sets_parent_and_child_links() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let div = attach(&mut doc, root, "div");
        let text = doc.create_text("hi").unwrap();
        doc.append_child(div, text).unwrap();

        let root_node = doc.node(root).unwrap();
        assert_eq!(root_node.first_child(), Some(div));
        assert_eq!(root_node.last_child(), Some(div));
        let div_node = doc.node(div).unwrap();
        assert_eq!(div_node.parent(), Some(root));
        assert_eq!(div_node.first_child(), Some(text));
        assert_eq!(div_node.last_child(), Some(text));
        assert_eq!(div_node.next_sibling(), None);
        assert_eq!(div_node.prev_sibling(), None);
        let text_node = doc.node(text).unwrap();
        assert_eq!(text_node.parent(), Some(div));
        assert_eq!(text_node.first_child(), None);
    }

    #[test]
    fn siblings_keep_insertion_order() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let b = attach(&mut doc, root, "b");
        let c = attach(&mut doc, root, "c");
        assert_eq!(doc.children(root).unwrap().collect::<Vec<_>>(), [a, b, c]);
        assert_eq!(doc.node(a).unwrap().next_sibling(), Some(b));
        assert_eq!(doc.node(b).unwrap().next_sibling(), Some(c));
        assert_eq!(doc.node(c).unwrap().next_sibling(), None);
        assert_eq!(doc.node(a).unwrap().prev_sibling(), None);
        assert_eq!(doc.node(b).unwrap().prev_sibling(), Some(a));
        assert_eq!(doc.node(c).unwrap().prev_sibling(), Some(b));
        assert_eq!(doc.node(root).unwrap().last_child(), Some(c));
    }

    #[test]
    fn descendants_are_preorder_and_stop_at_the_subtree_boundary() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let a1 = attach(&mut doc, a, "a1");
        let a2 = attach(&mut doc, a, "a2");
        let b = attach(&mut doc, root, "b");
        let b1 = attach(&mut doc, b, "b1");
        assert_eq!(
            doc.descendants(root).unwrap().collect::<Vec<_>>(),
            [a, a1, a2, b, b1]
        );
        assert_eq!(doc.descendants(a).unwrap().collect::<Vec<_>>(), [a1, a2]);
        assert_eq!(doc.descendants(a1).unwrap().count(), 0);
    }

    #[test]
    fn invalid_ids_are_rejected_everywhere() {
        let mut big = Document::new(Limits::default()).unwrap();
        let big_root = big.root();
        attach(&mut big, big_root, "a");
        let foreign = attach(&mut big, big_root, "b"); // index 2

        let mut small = Document::new(Limits::default()).unwrap(); // 1 node
        let small_root = small.root();
        assert_eq!(small.node(foreign).unwrap_err(), DomError::InvalidNodeId);
        assert!(matches!(
            small.children(foreign),
            Err(DomError::InvalidNodeId)
        ));
        assert!(matches!(
            small.descendants(foreign),
            Err(DomError::InvalidNodeId)
        ));
        assert_eq!(
            small.append_child(foreign, small_root),
            Err(DomError::InvalidNodeId)
        );
        assert_eq!(
            small.append_child(small_root, foreign),
            Err(DomError::InvalidNodeId)
        );
        assert_eq!(small.detach(foreign), Err(DomError::InvalidNodeId));
        assert_eq!(
            small.insert_before(foreign, small_root),
            Err(DomError::InvalidNodeId)
        );
        assert_eq!(
            small.move_children(foreign, small_root),
            Err(DomError::InvalidNodeId)
        );
        assert_eq!(
            small.append_text(foreign, "x"),
            Err(DomError::InvalidNodeId)
        );
    }

    #[test]
    fn node_limit_counts_every_allocated_slot() {
        let mut doc = doc_with(3, 10, 1024);
        let root = doc.root();
        attach(&mut doc, root, "a"); // 2 nodes
        doc.create_element("detached").unwrap(); // 3 nodes; detached still counts
        assert_eq!(
            doc.create_element("c"),
            Err(DomError::NodeLimitExceeded { limit: 3 })
        );
        assert_eq!(
            doc.create_text("t"),
            Err(DomError::NodeLimitExceeded { limit: 3 })
        );
        assert_eq!(doc.node_count(), 3);
    }

    #[test]
    fn depth_limit_is_enforced_and_failure_changes_nothing() {
        let mut doc = doc_with(100, 2, 1024);
        let root = doc.root();
        let a = attach(&mut doc, root, "a"); // depth 1
        let b = attach(&mut doc, a, "b"); // depth 2
        let c = doc.create_element("c").unwrap();
        assert_eq!(
            doc.append_child(b, c),
            Err(DomError::DepthLimitExceeded { limit: 2 })
        );
        assert_eq!(doc.node(c).unwrap().parent(), None);
        assert_eq!(doc.node(c).unwrap().prev_sibling(), None);
        assert_eq!(doc.node(b).unwrap().first_child(), None);
        assert_eq!(doc.node(b).unwrap().last_child(), None);
    }

    #[test]
    fn string_budget_is_enforced_before_allocation() {
        let mut doc = doc_with(100, 10, 5);
        doc.create_text("abc").unwrap(); // 3 bytes
        assert_eq!(
            doc.create_element("div"), // would make 6
            Err(DomError::StringBudgetExceeded { limit: 5 })
        );
        assert_eq!(doc.node_count(), 2);
        doc.create_text("ab").unwrap(); // exactly 5
        assert_eq!(
            doc.create_text("x"),
            Err(DomError::StringBudgetExceeded { limit: 5 })
        );
    }

    #[test]
    fn invalid_parents_are_rejected() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let text = doc.create_text("t").unwrap();
        doc.append_child(root, text).unwrap();
        let el = doc.create_element("e").unwrap();
        assert_eq!(doc.append_child(text, el), Err(DomError::InvalidParent));
        assert_eq!(doc.append_child(el, el), Err(DomError::InvalidParent));
    }

    #[test]
    fn attach_rules_are_enforced() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        assert_eq!(doc.append_child(root, a), Err(DomError::AlreadyAttached));
        assert_eq!(doc.append_child(a, root), Err(DomError::AlreadyAttached));
    }

    #[test]
    fn attaching_a_subtree_moves_it_whole() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let p = doc.create_element("p").unwrap();
        let c = doc.create_element("c").unwrap();
        doc.append_child(p, c).unwrap();
        doc.append_child(root, p).unwrap();
        assert_eq!(doc.descendants(root).unwrap().collect::<Vec<_>>(), [p, c]);
        assert_eq!(doc.node(c).unwrap().parent(), Some(p));
        assert_eq!(doc.node(p).unwrap().parent(), Some(root));
    }

    #[test]
    fn wide_tree_reaches_the_node_limit() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let max = Limits::default().max_nodes;
        for _ in 1..max {
            attach(&mut doc, root, "li");
        }
        assert_eq!(doc.node_count(), max);
        assert_eq!(doc.children(root).unwrap().count(), max - 1);
        let last = doc.node(root).unwrap().last_child().unwrap();
        assert_eq!(doc.node(last).unwrap().next_sibling(), None);
        assert_eq!(
            doc.create_element("one-too-many"),
            Err(DomError::NodeLimitExceeded { limit: max })
        );
    }

    #[test]
    fn deep_chain_traverses_and_drops_and_hits_the_depth_limit() {
        let depth = 10_000;
        // String budget is generous: 10_000 one-byte tags need 10_000 bytes.
        let mut doc = doc_with(depth + 2, depth, 1 << 20);
        let mut parent = doc.root();
        for _ in 0..depth {
            parent = attach(&mut doc, parent, "n");
        }
        assert_eq!(doc.node_count(), depth + 1);
        assert_eq!(doc.descendants(doc.root()).unwrap().count(), depth);

        let extra = doc.create_element("x").unwrap();
        assert_eq!(
            doc.append_child(parent, extra),
            Err(DomError::DepthLimitExceeded { limit: depth })
        );
        // `doc` is dropped here; Vec<Node> drop is iterative, not recursive.
    }

    #[test]
    fn elements_carry_namespace_and_attributes() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let svg = doc
            .create_element_with(
                Namespace::Svg,
                "svg",
                &[("viewBox", "0 0 1 1"), ("id", "x")],
            )
            .unwrap();
        let NodeKind::Element(data) = doc.node(svg).unwrap().kind() else {
            panic!("expected an element");
        };
        assert_eq!(data.namespace(), Namespace::Svg);
        assert_eq!(data.tag(), "svg");
        assert_eq!(data.attributes().len(), 2);
        assert_eq!(data.attribute("id"), Some("x"));
        assert_eq!(data.attribute("viewBox"), Some("0 0 1 1"));
        assert_eq!(data.attribute("missing"), None);
    }

    #[test]
    fn attribute_count_is_limited() {
        let mut doc = Document::new(Limits {
            max_attributes_per_element: 2,
            ..Limits::default()
        })
        .unwrap();
        assert_eq!(
            doc.create_element_with(Namespace::Html, "a", &[("a", "1"), ("b", "2"), ("c", "3")]),
            Err(DomError::AttributeLimitExceeded { limit: 2 })
        );
        assert_eq!(doc.node_count(), 1);
        doc.create_element_with(Namespace::Html, "a", &[("a", "1"), ("b", "2")])
            .unwrap();
        assert_eq!(doc.node_count(), 2);
    }

    #[test]
    fn attributes_are_charged_to_the_string_budget() {
        // One element "x" with attribute ("a", "b") costs this many bytes.
        let one = 1 + (1 + 1 + Limits::ATTRIBUTE_OVERHEAD_BYTES);
        let mut doc = doc_with(100, 10, one + 1);
        doc.create_element_with(Namespace::Html, "x", &[("a", "b")])
            .unwrap();
        assert_eq!(
            doc.create_element_with(Namespace::Html, "x", &[("a", "b")]),
            Err(DomError::StringBudgetExceeded { limit: one + 1 })
        );
        assert_eq!(doc.node_count(), 2);
    }

    #[test]
    fn text_append_coalesces_and_respects_the_budget() {
        let mut doc = doc_with(100, 10, 5);
        let t = doc.create_text("abc").unwrap();
        doc.append_text(t, "d").unwrap();
        assert_eq!(text_of(&doc, t), "abcd");
        assert_eq!(
            doc.append_text(t, "ef"),
            Err(DomError::StringBudgetExceeded { limit: 5 })
        );
        assert_eq!(text_of(&doc, t), "abcd");
        doc.append_text(t, "e").unwrap(); // exactly 5
        assert_eq!(text_of(&doc, t), "abcde");

        let mut other = Document::new(Limits::default()).unwrap();
        let root = other.root();
        let el = other.create_element("p").unwrap();
        assert_eq!(other.append_text(el, "x"), Err(DomError::NotText));
        assert_eq!(other.append_text(root, "x"), Err(DomError::NotText));
    }

    #[test]
    fn detach_unlinks_and_is_idempotent() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let b = attach(&mut doc, root, "b");
        let c = attach(&mut doc, root, "c");

        doc.detach(b).unwrap(); // middle
        assert_eq!(doc.children(root).unwrap().collect::<Vec<_>>(), [a, c]);
        assert_eq!(doc.node(a).unwrap().next_sibling(), Some(c));
        assert_eq!(doc.node(c).unwrap().prev_sibling(), Some(a));
        let b_node = doc.node(b).unwrap();
        assert_eq!(b_node.parent(), None);
        assert_eq!(b_node.prev_sibling(), None);
        assert_eq!(b_node.next_sibling(), None);
        assert_eq!(doc.node_count(), 4); // detached nodes stay allocated

        doc.detach(b).unwrap(); // already detached: no-op
        doc.detach(root).unwrap(); // root has no parent: no-op

        doc.detach(c).unwrap(); // last of several
        assert_eq!(doc.node(root).unwrap().last_child(), Some(a));
        assert_eq!(doc.node(a).unwrap().next_sibling(), None);

        doc.detach(a).unwrap(); // only child
        assert_eq!(doc.node(root).unwrap().first_child(), None);
        assert_eq!(doc.node(root).unwrap().last_child(), None);

        // Detached nodes can be attached again.
        doc.append_child(root, b).unwrap();
        doc.append_child(root, a).unwrap();
        doc.append_child(root, c).unwrap();
        assert_eq!(doc.children(root).unwrap().collect::<Vec<_>>(), [b, a, c]);
        doc.detach(b).unwrap(); // first of several
        assert_eq!(doc.node(root).unwrap().first_child(), Some(a));
        assert_eq!(doc.node(a).unwrap().prev_sibling(), None);
    }

    #[test]
    fn insert_before_orders_nodes_and_checks_arguments() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let c = attach(&mut doc, root, "c");
        let b = doc.create_element("b").unwrap();
        doc.insert_before(c, b).unwrap();
        assert_eq!(doc.children(root).unwrap().collect::<Vec<_>>(), [a, b, c]);
        assert_eq!(doc.node(a).unwrap().next_sibling(), Some(b));
        assert_eq!(doc.node(b).unwrap().prev_sibling(), Some(a));
        assert_eq!(doc.node(b).unwrap().next_sibling(), Some(c));
        assert_eq!(doc.node(c).unwrap().prev_sibling(), Some(b));

        let z = doc.create_element("z").unwrap();
        doc.insert_before(a, z).unwrap();
        assert_eq!(
            doc.children(root).unwrap().collect::<Vec<_>>(),
            [z, a, b, c]
        );
        assert_eq!(doc.node(root).unwrap().first_child(), Some(z));
        assert_eq!(doc.node(z).unwrap().prev_sibling(), None);
        assert_eq!(doc.node(a).unwrap().prev_sibling(), Some(z));

        let loose = doc.create_element("loose").unwrap();
        let also_loose = doc.create_element("also").unwrap();
        assert_eq!(
            doc.insert_before(loose, also_loose),
            Err(DomError::NotAttached)
        );
        assert_eq!(doc.insert_before(root, loose), Err(DomError::NotAttached));
        assert_eq!(doc.insert_before(a, c), Err(DomError::AlreadyAttached));
    }

    #[test]
    fn cycles_are_rejected() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let b = attach(&mut doc, a, "b");
        doc.detach(a).unwrap(); // detached subtree a -> b

        assert_eq!(doc.append_child(b, a), Err(DomError::HierarchyCycle));
        assert_eq!(doc.insert_before(b, a), Err(DomError::HierarchyCycle));
        assert_eq!(doc.move_children(a, b), Err(DomError::HierarchyCycle));
        assert_eq!(doc.move_children(a, a), Err(DomError::InvalidParent));
    }

    #[test]
    fn move_children_splices_in_order() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let a1 = attach(&mut doc, a, "a1");
        let a2 = attach(&mut doc, a, "a2");
        let b = attach(&mut doc, root, "b");
        let b1 = attach(&mut doc, b, "b1");

        doc.move_children(a, b).unwrap();
        assert_eq!(doc.children(b).unwrap().collect::<Vec<_>>(), [b1, a1, a2]);
        assert_eq!(doc.node(a).unwrap().first_child(), None);
        assert_eq!(doc.node(a).unwrap().last_child(), None);
        assert_eq!(doc.node(a1).unwrap().parent(), Some(b));
        assert_eq!(doc.node(a2).unwrap().parent(), Some(b));
        assert_eq!(doc.node(a1).unwrap().prev_sibling(), Some(b1));
        assert_eq!(doc.node(b1).unwrap().next_sibling(), Some(a1));
        assert_eq!(doc.node(a1).unwrap().next_sibling(), Some(a2));
        assert_eq!(doc.node(a2).unwrap().prev_sibling(), Some(a1));
        assert_eq!(doc.node(a2).unwrap().next_sibling(), None);
        assert_eq!(doc.node(b).unwrap().last_child(), Some(a2));
        assert_eq!(
            doc.descendants(root).unwrap().collect::<Vec<_>>(),
            [a, b, b1, a1, a2]
        );

        doc.move_children(a, b).unwrap(); // nothing left to move: no-op

        let t = doc.create_text("t").unwrap();
        doc.append_child(root, t).unwrap();
        assert_eq!(doc.move_children(b, t), Err(DomError::InvalidParent));

        let e = attach(&mut doc, root, "e"); // empty target
        doc.move_children(b, e).unwrap();
        assert_eq!(doc.children(e).unwrap().collect::<Vec<_>>(), [b1, a1, a2]);
        assert_eq!(doc.node(e).unwrap().first_child(), Some(b1));
    }

    #[test]
    fn validate_depth_catches_moved_subtrees() {
        let mut doc = doc_with(100, 3, 1024);
        let root = doc.root();
        let a = attach(&mut doc, root, "a");
        let b = attach(&mut doc, a, "b");
        let c = attach(&mut doc, b, "c"); // depth 3
        assert_eq!(doc.validate_depth(), Ok(()));

        let d = doc.create_element("d").unwrap();
        let e = doc.create_element("e").unwrap();
        let f = doc.create_element("f").unwrap();
        doc.append_child(d, e).unwrap();
        doc.append_child(e, f).unwrap();
        doc.append_child(c, d).unwrap(); // subtree move: not checked per operation
        assert_eq!(
            doc.validate_depth(),
            Err(DomError::DepthLimitExceeded { limit: 3 })
        );
    }

    #[test]
    fn shrink_to_fit_keeps_content() {
        let mut doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        let t = doc.create_text("a").unwrap();
        doc.append_text(t, "bc").unwrap();
        doc.append_child(root, t).unwrap();
        for _ in 0..10 {
            attach(&mut doc, root, "x");
        }
        doc.shrink_to_fit();
        assert_eq!(text_of(&doc, t), "abc");
        assert_eq!(doc.children(root).unwrap().count(), 11);
        assert_eq!(doc.node_count(), 12);
    }

    #[test]
    fn quirks_mode_round_trips() {
        let mut doc = Document::new(Limits::default()).unwrap();
        assert_eq!(doc.quirks_mode(), QuirksMode::NoQuirks);
        doc.set_quirks_mode(QuirksMode::Quirks);
        assert_eq!(doc.quirks_mode(), QuirksMode::Quirks);
    }

    #[test]
    fn add_missing_attributes_adds_only_absent_names() {
        let mut doc = Document::new(Limits {
            max_attributes_per_element: 3,
            ..Limits::default()
        })
        .unwrap();
        let el = doc
            .create_element_with(Namespace::Html, "html", &[("id", "1")])
            .unwrap();
        doc.add_missing_attributes(el, &[("id", "2"), ("class", "c"), ("class", "d")])
            .unwrap();
        let NodeKind::Element(data) = doc.node(el).unwrap().kind() else {
            panic!("expected an element");
        };
        assert_eq!(data.attributes().len(), 2);
        assert_eq!(data.attribute("id"), Some("1"));
        assert_eq!(data.attribute("class"), Some("c"));

        // Nothing new to add is a no-op; exceeding the cap changes nothing.
        doc.add_missing_attributes(el, &[("id", "9")]).unwrap();
        assert_eq!(
            doc.add_missing_attributes(el, &[("a", "1"), ("b", "2")]),
            Err(DomError::AttributeLimitExceeded { limit: 3 })
        );
        let NodeKind::Element(data) = doc.node(el).unwrap().kind() else {
            panic!("expected an element");
        };
        assert_eq!(data.attributes().len(), 2);

        let text = doc.create_text("t").unwrap();
        assert_eq!(
            doc.add_missing_attributes(text, &[("a", "1")]),
            Err(DomError::NotElement)
        );
    }
}
