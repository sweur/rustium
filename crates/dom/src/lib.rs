//! Rustium DOM: an arena-allocated document tree.

#![forbid(unsafe_code)]

mod document;
mod error;
mod iter;
mod node;

pub use document::{Document, Limits, QuirksMode};
pub use error::DomError;
pub use iter::{Children, Descendants};
pub use node::{Attribute, ElementData, Namespace, Node, NodeId, NodeKind};
