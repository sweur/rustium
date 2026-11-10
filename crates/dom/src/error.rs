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
    AttributeLimitExceeded {
        limit: usize,
    },
    /// Parent is a text node, or parent and child are the same node.
    InvalidParent,
    /// The node is the root or already has a parent.
    AlreadyAttached,
    /// The reference node has no parent (it is detached, or it is the root).
    NotAttached,
    /// The operation would make a node its own ancestor.
    HierarchyCycle,
    /// The operation needs a text node.
    NotText,
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
            DomError::AttributeLimitExceeded { limit } => {
                write!(f, "attribute limit of {limit} per element exceeded")
            }
            DomError::InvalidParent => f.write_str("parent cannot hold this child"),
            DomError::AlreadyAttached => f.write_str("node is the root or already has a parent"),
            DomError::NotAttached => f.write_str("reference node has no parent"),
            DomError::HierarchyCycle => f.write_str("operation would create a cycle"),
            DomError::NotText => f.write_str("node is not a text node"),
        }
    }
}

impl std::error::Error for DomError {}
