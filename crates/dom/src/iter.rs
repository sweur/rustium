use crate::document::Document;
use crate::node::NodeId;

/// Iterator over the direct children of a node.
#[derive(Debug)]
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl<'a> Children<'a> {
    pub(crate) fn new(doc: &'a Document, first: Option<NodeId>) -> Self {
        Children { doc, next: first }
    }
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.get(current).and_then(|n| n.next_sibling);
        Some(current)
    }
}

/// Pre-order iterator over the proper descendants of a node.
/// O(1) extra memory: it climbs parent links instead of keeping a stack.
#[derive(Debug)]
pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl<'a> Descendants<'a> {
    pub(crate) fn new(doc: &'a Document, root: NodeId, first: Option<NodeId>) -> Self {
        Descendants {
            doc,
            root,
            next: first,
        }
    }

    fn advance(&self, current: NodeId) -> Option<NodeId> {
        if let Some(child) = self.doc.get(current)?.first_child {
            return Some(child);
        }
        let mut cursor = current;
        loop {
            if cursor == self.root {
                return None; // never leave the starting subtree
            }
            let node = self.doc.get(cursor)?;
            if let Some(sibling) = node.next_sibling {
                return Some(sibling);
            }
            cursor = node.parent?;
        }
    }
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.advance(current);
        Some(current)
    }
}
