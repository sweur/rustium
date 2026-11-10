//! Rustium DOM: an arena-allocated, append-only document tree.

#![forbid(unsafe_code)]

mod document;
mod error;
mod iter;
mod node;

pub use document::{Document, Limits};
pub use error::DomError;
pub use iter::{Children, Descendants};
pub use node::{Node, NodeId, NodeKind};
