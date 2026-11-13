//! Rustium CSS: tokenizer and selector parser (Phase 3, stage A1).
//! Std only. Input is untrusted; every limit is checked before allocation.

#![forbid(unsafe_code)]

mod limits;
mod selector;
mod token;
mod tokenizer;

pub use limits::CssLimits;
pub use selector::{
    parse_selector_list, parse_selector_text, Compound, Selector, SelectorError, Specificity,
};
pub use token::Token;
pub use tokenizer::{preprocess, Tokenizer};