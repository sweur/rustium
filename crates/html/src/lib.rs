//! Rustium HTML: parses untrusted UTF-8 HTML into a `rustium_dom::Document`.
//!
//! Policies (Phase 2): input larger than `max_input_bytes` is rejected before
//! parsing; any document limit breach fails the whole parse (no partial
//! documents); comments, doctypes and processing instructions are not
//! represented; scripting is disabled; non-UTF-8 bytes become U+FFFD.

#![forbid(unsafe_code)]

mod sink;

use rustium_dom::{Document, DomError, Limits};

/// Limits for one `parse` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum input size in bytes, checked before any parsing work.
    pub max_input_bytes: usize,
    /// The input is fed to the parser in chunks of this many bytes (must be >= 1).
    pub chunk_bytes: usize,
    /// Per-document limits (nodes, depth, string bytes, attributes).
    pub dom: Limits,
}

impl Default for ParseLimits {
    fn default() -> Self {
        ParseLimits {
            max_input_bytes: 4 * 1024 * 1024,
            chunk_bytes: 16 * 1024,
            dom: Limits::default(),
        }
    }
}

impl ParseLimits {
    pub fn validate(&self) -> Result<(), ParseError> {
        if self.chunk_bytes == 0 {
            return Err(ParseError::InvalidLimits);
        }
        self.dom.validate().map_err(ParseError::Dom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// `ParseLimits` itself is invalid (for example `chunk_bytes == 0`).
    InvalidLimits,
    InputTooLarge {
        limit: usize,
        actual: usize,
    },
    /// A document limit was exceeded, or the DOM rejected an operation.
    Dom(DomError),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidLimits => f.write_str("parse limits are invalid"),
            ParseError::InputTooLarge { limit, actual } => {
                write!(
                    f,
                    "input of {actual} bytes exceeds the limit of {limit} bytes"
                )
            }
            ParseError::Dom(err) => write!(f, "document error: {err}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A successfully parsed document.
#[derive(Debug)]
pub struct Parsed {
    pub document: Document,
    /// Number of conformance errors the parser reported. Counted, never stored.
    pub parse_errors: usize,
}

/// Parses `input` as an HTML document. Fails closed: on any limit breach no
/// partial document is returned.
pub fn parse(input: &[u8], limits: &ParseLimits) -> Result<Parsed, ParseError> {
    limits.validate()?;
    if input.len() > limits.max_input_bytes {
        return Err(ParseError::InputTooLarge {
            limit: limits.max_input_bytes,
            actual: input.len(),
        });
    }
    let state = sink::State::new(limits)?;
    sink::drive(&state, input, limits.chunk_bytes);
    state.into_result()
}
