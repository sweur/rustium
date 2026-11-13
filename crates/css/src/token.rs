use std::borrow::Cow;

/// A CSS token (CSS Syntax Level 3, simplified). End of input is the end of
/// the iterator. Text borrows from the source unless an escape had to be decoded.
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    Ident(Cow<'a, str>),
    /// A name followed by `(`; the `(` is part of the token.
    Function(Cow<'a, str>),
    AtKeyword(Cow<'a, str>),
    /// `#name`; `is_id` is true when the name is also a valid identifier.
    Hash { value: Cow<'a, str>, is_id: bool },
    String(Cow<'a, str>),
    BadString,
    /// `url(...)` with an unquoted argument.
    Url(Cow<'a, str>),
    BadUrl,
    Delim(char),
    Number { value: f32, is_integer: bool },
    Percentage(f32),
    Dimension {
        value: f32,
        is_integer: bool,
        unit: Cow<'a, str>,
    },
    Whitespace,
    /// `<!--`
    Cdo,
    /// `-->`
    Cdc,
    Colon,
    Semicolon,
    Comma,
    LeftSquare,
    RightSquare,
    LeftParen,
    RightParen,
    LeftCurly,
    RightCurly,
}