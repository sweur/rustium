use crate::error::DomError;
use crate::iter::{Children, Descendants};
use crate::node::{Node, NodeId, NodeKind};

/// Per-document resource limits. `max_nodes` counts allocated slots
/// (including the root and detached nodes), not just attached nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_nodes: usize,
    /// Maximum depth; the root has depth 0.
    pub max_depth: usize,
    /// Total bytes of all tag names and text across the document.
    pub max_string_bytes: usize,
}

impl Limits {
    /// Ids are `index + 1` in a `u32`, so the node count must stay below this.
    pub const MAX_NODES_CEILING: usize = (u32::MAX - 1) as usize;

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
        }
    }
}

/// Arena-allocated, append-only document tree. Index 0 is the root.
#[derive(Debug)]
pub struct Document {
    nodes: Vec<Node>,
    limits: Limits,
    string_bytes: usize,
}

impl Document {
    pub fn new(limits: Limits) -> Result<Document, DomError> {
        limits.validate()?;
        Ok(Document {
            nodes: vec![Node::new(NodeKind::Document)],
            limits,
            string_bytes: 0,
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

    pub fn create_element(&mut self, tag: &str) -> Result<NodeId, DomError> {
        let total = self.check_room(tag.len())?;
        self.push_node(
            NodeKind::Element {
                tag: Box::from(tag),
            },
            total,
        )
    }

    pub fn create_text(&mut self, text: &str) -> Result<NodeId, DomError> {
        let total = self.check_room(text.len())?;
        self.push_node(NodeKind::Text(Box::from(text)), total)
    }

    /// Appends a detached, childless `child` as the last child of `parent`.
    /// All checks run before any mutation, so a failed call changes nothing.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        let parent_node = self.node(parent)?;
        let child_node = self.node(child)?;

        if parent == child
            || !matches!(
                parent_node.kind,
                NodeKind::Document | NodeKind::Element { .. }
            )
        {
            return Err(DomError::InvalidParent);
        }
        if child == NodeId::ROOT || child_node.parent.is_some() {
            return Err(DomError::AlreadyAttached);
        }
        if child_node.first_child.is_some() {
            return Err(DomError::CannotAttachSubtree);
        }
        if self.depth_of(parent) + 1 > self.limits.max_depth {
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
        self.node_mut(child)?.parent = Some(parent);
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

    /// Depth of `id` (root = 0), by walking parent links. The walk is capped
    /// at `max_depth + 1` steps so a corrupted chain could never loop forever.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(max_nodes: usize, max_depth: usize, max_string_bytes: usize) -> Document {
        Document::new(Limits {
            max_nodes,
            max_depth,
            max_string_bytes,
        })
        .unwrap()
    }

    fn attach(doc: &mut Document, parent: NodeId, tag: &str) -> NodeId {
        let id = doc.create_element(tag).unwrap();
        doc.append_child(parent, id).unwrap();
        id
    }

    #[test]
    fn empty_document_has_only_the_root() {
        let doc = Document::new(Limits::default()).unwrap();
        let root = doc.root();
        assert_eq!(doc.node_count(), 1);
        let node = doc.node(root).unwrap();
        assert_eq!(node.kind(), &NodeKind::Document);
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
        assert_eq!(
            doc.node(el).unwrap().kind(),
            &NodeKind::Element { tag: "div".into() }
        );
        assert_eq!(doc.node(text).unwrap().kind(), &NodeKind::Text("hi".into()));
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

        let p = doc.create_element("p").unwrap();
        let c = doc.create_element("c").unwrap();
        doc.append_child(p, c).unwrap();
        assert_eq!(
            doc.append_child(root, p),
            Err(DomError::CannotAttachSubtree)
        );
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
}
