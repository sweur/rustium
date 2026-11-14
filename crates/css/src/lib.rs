//! Rustium CSS: tokenizer, selectors, typed values and declarations.
//! Std only. Input is untrusted; every limit is checked before allocation.

#![forbid(unsafe_code)]

mod color;
mod declaration;
mod error;
mod limits;
mod selector;
mod token;
mod tokenizer;
mod value;
mod values;

pub use declaration::{Declaration, ParsedDeclarations, parse_declaration_list};
pub use error::CssError;
pub use limits::CssLimits;
pub use selector::{
    Compound, Selector, SelectorError, Specificity, parse_selector_list, parse_selector_text,
};
pub use token::Token;
pub use tokenizer::{Tokenizer, preprocess};
pub use value::{
    BorderStyle, Color, ColorValue, Display, FontStyle, Length, LineHeight, PropertyId, TextAlign,
    Value,
};
