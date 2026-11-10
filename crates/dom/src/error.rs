use std::fmt;

/// Errors returned by `Document` operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomError {
    /// The id does not refer to a node in this document.
    InvalidNodeId,
    /// `Limits` failed validation.
    InvalidLimits,
    NodeLimitExceeded {
        limit: usize,
    },
    DepthLimitExceeded {
        limit: usize,
    },
    StringBudgetExceeded {
        limit: usize,
    },
    /// Parent is a text node, or parent and child are the same node.
    InvalidParent,
    /// Child is the root or already has a parent.
    AlreadyAttached,
    /// Phase 1 only attaches detached nodes that have no children.
    CannotAttachSubtree,
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomError::InvalidNodeId => {
                f.write_str("node id does not refer to a node in this document")
            }
            DomError::InvalidLimits => f.write_str("document limits are invalid"),
            DomError::NodeLimitExceeded { limit } => write!(f, "node limit of {limit} exceeded"),
            DomError::DepthLimitExceeded { limit } => write!(f, "depth limit of {limit} exceeded"),
            DomError::StringBudgetExceeded { limit } => {
                write!(f, "string budget of {limit} bytes exceeded")
            }
            DomError::InvalidParent => f.write_str("parent cannot hold this child"),
            DomError::AlreadyAttached => f.write_str("child is the root or already has a parent"),
            DomError::CannotAttachSubtree => {
                f.write_str("only childless, detached nodes can be attached in this version")
            }
        }
    }
}

impl std::error::Error for DomError {}
