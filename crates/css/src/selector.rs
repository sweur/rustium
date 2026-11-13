use std::fmt;

use crate::limits::CssLimits;
use crate::token::Token;
use crate::tokenizer::{Tokenizer, preprocess};

/// Selector specificity (ids, classes, types), packed so that comparing the
/// numbers compares the tuples. Each part saturates at 1023.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Specificity(u32);

impl Specificity {
    const MAX_PART: u32 = 1023;

    pub fn new(ids: u32, classes: u32, types: u32) -> Specificity {
        let ids = ids.min(Self::MAX_PART);
        let classes = classes.min(Self::MAX_PART);
        let types = types.min(Self::MAX_PART);
        Specificity((ids << 20) | (classes << 10) | types)
    }

    pub fn ids(self) -> u32 {
        self.0 >> 20
    }

    pub fn classes(self) -> u32 {
        (self.0 >> 10) & Self::MAX_PART
    }

    pub fn types(self) -> u32 {
        self.0 & Self::MAX_PART
    }
}

/// One compound selector: an optional type plus ids and classes, all of which
/// must match the same element. A universal `*` has no tag, ids or classes.
/// Names are stored as written; the matcher decides about case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compound {
    pub tag: Option<Box<str>>,
    pub ids: Vec<Box<str>>,
    pub classes: Vec<Box<str>>,
}

/// A complex selector made of compounds joined by descendant combinators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    /// Ancestor-most compound first; the subject (rightmost) compound last.
    pub compounds: Vec<Compound>,
    pub specificity: Specificity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorError {
    /// Not a selector at all (empty, stray punctuation, bad structure).
    Invalid,
    /// A valid CSS selector that Rustium does not support yet.
    Unsupported,
    /// A configured limit was exceeded.
    TooComplex,
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectorError::Invalid => f.write_str("invalid selector"),
            SelectorError::Unsupported => f.write_str("unsupported selector"),
            SelectorError::TooComplex => f.write_str("selector exceeds a configured limit"),
        }
    }
}

impl std::error::Error for SelectorError {}

#[derive(Default)]
struct CompoundBuilder {
    tag: Option<Box<str>>,
    ids: Vec<Box<str>>,
    classes: Vec<Box<str>>,
    parts: usize,
}

impl CompoundBuilder {
    fn is_empty(&self) -> bool {
        self.parts == 0
    }

    fn finish(self) -> Compound {
        Compound {
            tag: self.tag,
            ids: self.ids,
            classes: self.classes,
        }
    }

    fn add_part(&mut self, limits: &CssLimits) -> Result<(), SelectorError> {
        self.parts += 1;
        if self.parts > limits.max_simple_selectors_per_compound {
            return Err(SelectorError::TooComplex);
        }
        Ok(())
    }
}

fn boxed(name: &str, limits: &CssLimits) -> Result<Box<str>, SelectorError> {
    if name.len() > limits.max_name_bytes {
        return Err(SelectorError::TooComplex);
    }
    Ok(Box::from(name))
}

fn push_compound(
    compounds: &mut Vec<Compound>,
    builder: &mut CompoundBuilder,
    limits: &CssLimits,
) -> Result<(), SelectorError> {
    compounds.push(std::mem::take(builder).finish());
    if compounds.len() > limits.max_compounds_per_selector {
        return Err(SelectorError::TooComplex);
    }
    Ok(())
}

/// Parses one selector starting at `start`. Returns it with the index of the
/// terminating comma (or the end of the slice).
fn parse_one(
    tokens: &[Token<'_>],
    start: usize,
    limits: &CssLimits,
) -> Result<(Selector, usize), SelectorError> {
    let mut compounds: Vec<Compound> = Vec::new();
    let mut current = CompoundBuilder::default();
    let (mut ids, mut classes, mut types) = (0u32, 0u32, 0u32);
    let mut i = start;
    loop {
        match tokens.get(i) {
            None | Some(Token::Comma) => break,
            Some(Token::Whitespace) => {
                if !current.is_empty() {
                    push_compound(&mut compounds, &mut current, limits)?;
                }
            }
            Some(Token::Ident(name)) => {
                if !current.is_empty() {
                    return Err(SelectorError::Invalid);
                }
                current.tag = Some(boxed(name, limits)?);
                current.add_part(limits)?;
                types += 1;
            }
            Some(Token::Delim('*')) => {
                if !current.is_empty() {
                    return Err(SelectorError::Invalid);
                }
                current.add_part(limits)?;
            }
            Some(Token::Hash { value, is_id: true }) => {
                current.ids.push(boxed(value, limits)?);
                current.add_part(limits)?;
                ids += 1;
            }
            Some(Token::Delim('.')) => match tokens.get(i + 1) {
                Some(Token::Ident(name)) => {
                    current.classes.push(boxed(name, limits)?);
                    current.add_part(limits)?;
                    classes += 1;
                    i += 1;
                }
                _ => return Err(SelectorError::Invalid),
            },
            Some(
                Token::Colon
                | Token::LeftSquare
                | Token::RightSquare
                | Token::LeftParen
                | Token::Function(_)
                | Token::Delim('>' | '+' | '~' | '|'),
            ) => return Err(SelectorError::Unsupported),
            Some(_) => return Err(SelectorError::Invalid),
        }
        i += 1;
    }
    if !current.is_empty() {
        push_compound(&mut compounds, &mut current, limits)?;
    }
    if compounds.is_empty() {
        return Err(SelectorError::Invalid);
    }
    let selector = Selector {
        compounds,
        specificity: Specificity::new(ids, classes, types),
    };
    Ok((selector, i))
}

/// Parses a comma-separated selector list from the tokens of a rule prelude.
/// Any invalid or unsupported selector rejects the whole list.
pub fn parse_selector_list(
    prelude: &[Token<'_>],
    limits: &CssLimits,
) -> Result<Vec<Selector>, SelectorError> {
    if prelude.len() > limits.max_prelude_tokens {
        return Err(SelectorError::TooComplex);
    }
    let mut selectors = Vec::new();
    let mut start = 0;
    loop {
        let (selector, end) = parse_one(prelude, start, limits)?;
        selectors.push(selector);
        if selectors.len() > limits.max_selectors_per_rule {
            return Err(SelectorError::TooComplex);
        }
        match prelude.get(end) {
            Some(Token::Comma) => start = end + 1,
            _ => return Ok(selectors),
        }
    }
}

/// Tokenizes and parses selector text (for tests and future selector queries).
pub fn parse_selector_text(text: &str, limits: &CssLimits) -> Result<Vec<Selector>, SelectorError> {
    let text = preprocess(text);
    let tokens: Vec<Token<'_>> = Tokenizer::new(&text)
        .take(limits.max_prelude_tokens + 1)
        .collect();
    parse_selector_list(&tokens, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> CssLimits {
        CssLimits::default()
    }

    fn parse(text: &str) -> Vec<Selector> {
        parse_selector_text(text, &limits()).unwrap()
    }

    fn error(text: &str) -> SelectorError {
        parse_selector_text(text, &limits()).unwrap_err()
    }

    fn strs(values: &[Box<str>]) -> Vec<&str> {
        values.iter().map(|s| &**s).collect()
    }

    #[test]
    fn type_selector() {
        let list = parse("div");
        assert_eq!(list.len(), 1);
        let selector = &list[0];
        assert_eq!(selector.compounds.len(), 1);
        assert_eq!(selector.compounds[0].tag.as_deref(), Some("div"));
        assert!(selector.compounds[0].ids.is_empty());
        assert!(selector.compounds[0].classes.is_empty());
        assert_eq!(selector.specificity, Specificity::new(0, 0, 1));
    }

    #[test]
    fn class_and_id_selectors() {
        let list = parse(".a.b#c");
        let compound = &list[0].compounds[0];
        assert_eq!(compound.tag, None);
        assert_eq!(strs(&compound.classes), ["a", "b"]);
        assert_eq!(strs(&compound.ids), ["c"]);
        assert_eq!(list[0].specificity, Specificity::new(1, 2, 0));

        let list = parse("a.x#y.z");
        assert_eq!(list[0].compounds[0].tag.as_deref(), Some("a"));
        assert_eq!(list[0].specificity, Specificity::new(1, 2, 1));
    }

    #[test]
    fn descendant_chains() {
        let list = parse("ul li a");
        let tags: Vec<Option<&str>> = list[0].compounds.iter().map(|c| c.tag.as_deref()).collect();
        assert_eq!(tags, [Some("ul"), Some("li"), Some("a")]);
        assert_eq!(list[0].specificity, Specificity::new(0, 0, 3));
        assert_eq!(parse("div   p\n\tspan")[0].compounds.len(), 3);
    }

    #[test]
    fn selector_lists_and_surrounding_whitespace() {
        assert_eq!(parse("a, b ,c").len(), 3);
        let list = parse("  a  ,  b  ");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].compounds[0].tag.as_deref(), Some("a"));
        assert_eq!(list[1].compounds[0].tag.as_deref(), Some("b"));
    }

    #[test]
    fn universal_selector() {
        let list = parse("*");
        assert_eq!(list[0].compounds[0].tag, None);
        assert_eq!(list[0].specificity, Specificity::new(0, 0, 0));
        let list = parse("*.x");
        assert_eq!(strs(&list[0].compounds[0].classes), ["x"]);
        assert_eq!(list[0].specificity, Specificity::new(0, 1, 0));
        assert_eq!(parse("* *")[0].compounds.len(), 2);
    }

    #[test]
    fn unsupported_selectors_are_rejected() {
        for text in [
            "a > b", "a>b", "a + b", "a ~ b", "[x]", "a[x]", "a:hover", "::before", "a|b",
            ":not(a)",
        ] {
            assert_eq!(error(text), SelectorError::Unsupported, "{text}");
        }
    }

    #[test]
    fn invalid_selectors_are_rejected() {
        for text in [
            "", "   ", ",", "a,", ",a", "a,,b", ".", ". a", "#", "a b ,", "1a", "\"x\"", "a*",
            ".1", "#1x",
        ] {
            assert_eq!(error(text), SelectorError::Invalid, "{text:?}");
        }
    }

    #[test]
    fn limits_are_enforced_exactly() {
        let at = |text: String| parse_selector_text(&text, &limits());
        assert!(at(vec!["a"; 32].join(",")).is_ok());
        assert_eq!(at(vec!["a"; 33].join(",")), Err(SelectorError::TooComplex));
        assert!(at(["a"; 8].join(" ")).is_ok());
        assert_eq!(at(["a"; 9].join(" ")), Err(SelectorError::TooComplex));
        assert!(at(".a".repeat(16)).is_ok());
        assert_eq!(at(".a".repeat(17)), Err(SelectorError::TooComplex));
        assert!(at(format!(".{}", "n".repeat(256))).is_ok());
        assert_eq!(
            at(format!(".{}", "n".repeat(257))),
            Err(SelectorError::TooComplex)
        );
        assert_eq!(at("a ".repeat(3000)), Err(SelectorError::TooComplex));
    }

    #[test]
    fn specificity_ordering() {
        let id = parse("#a")[0].specificity;
        let classes = parse(".a.b.c.d.e.f.g.h.i.j")[0].specificity;
        let types = parse("a b c d e f g h")[0].specificity;
        assert!(id > classes);
        assert!(classes > types);
        assert_eq!(
            parse("ul#nav li.item a")[0].specificity,
            Specificity::new(1, 1, 3)
        );
        let packed = Specificity::new(1, 2, 3);
        assert_eq!((packed.ids(), packed.classes(), packed.types()), (1, 2, 3));
        assert_eq!(Specificity::new(5000, 0, 0).ids(), 1023);
        assert!(Specificity::new(1, 0, 0) > Specificity::new(0, 1023, 1023));
    }

    #[test]
    fn escapes_are_decoded_and_case_is_kept() {
        assert_eq!(strs(&parse(".a\\:b")[0].compounds[0].classes), ["a:b"]);
        assert_eq!(strs(&parse(".Foo")[0].compounds[0].classes), ["Foo"]);
        assert_eq!(parse("DIV")[0].compounds[0].tag.as_deref(), Some("DIV"));
    }
}
