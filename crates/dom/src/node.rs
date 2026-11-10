//! Node identifiers and node data. Link fields are crate-private so only
//! `Document` can change the tree structure.

use std::num::NonZeroU32;

/// Opaque handle to a node. Stores `index + 1` so `Option<NodeId>` is 4 bytes.
/// There is no public constructor: ids only come from a `Document`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// The document root always lives at index 0.
    pub(crate) const ROOT: NodeId = NodeId(NonZeroU32::MIN);

    pub(crate) fn from_index(index: usize) -> Option<NodeId> {
        let raw = u32::try_from(index).ok()?.checked_add(1)?;
        NonZeroU32::new(raw).map(NodeId)
    }

    pub(crate) fn index(self) -> usize {
        // u32 -> usize is lossless on 32- and 64-bit targets; value is >= 1.
        (self.0.get() - 1) as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element { tag: Box<str> },
    Text(Box<str>),
}

#[derive(Debug)]
pub struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    pub(crate) kind: NodeKind,
}

impl Node {
    pub(crate) fn new(kind: NodeKind) -> Node {
        Node {
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            kind,
        }
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn first_child(&self) -> Option<NodeId> {
        self.first_child
    }

    pub fn last_child(&self) -> Option<NodeId> {
        self.last_child
    }

    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next_sibling
    }

    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn option_node_id_uses_the_niche() {
        assert_eq!(size_of::<NodeId>(), 4);
        assert_eq!(size_of::<Option<NodeId>>(), 4);
    }

    #[test]
    fn report_node_size() {
        // Informational. Run with `-- --nocapture` to see the numbers.
        println!("size_of::<Node>() = {}", size_of::<Node>());
        println!("size_of::<NodeKind>() = {}", size_of::<NodeKind>());
    }

    #[test]
    fn id_index_round_trip_and_overflow() {
        assert_eq!(NodeId::from_index(0), Some(NodeId::ROOT));
        assert_eq!(NodeId::from_index(41).unwrap().index(), 41);
        assert_eq!(NodeId::from_index(u32::MAX as usize), None);
        assert_eq!(NodeId::from_index(usize::MAX), None);
    }
}
