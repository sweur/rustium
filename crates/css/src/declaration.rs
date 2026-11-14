//! Declarations: `name: value [!important]`, shorthand expansion, and the
//! declaration-list parser (used for `style=""` attributes and rule blocks).

use crate::color::parse_color_prefix;
use crate::error::CssError;
use crate::limits::CssLimits;
use crate::token::Token;
use crate::tokenizer::{Tokenizer, preprocess};
use crate::value::{PropertyId, Value};
use crate::values::{css_wide, parse_longhand, single, trim};

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: PropertyId,
    pub value: Value,
    pub important: bool,
}

/// The declarations of one block, in source order.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedDeclarations {
    pub declarations: Vec<Declaration>,
    /// Source declarations that were unknown, invalid, unsupported or too long.
    pub dropped: usize,
    /// More declarations than `max_declarations_per_rule`. The callers decide
    /// what that means: a rule is dropped, an inline style is an error.
    pub exceeded_declaration_limit: bool,
}

#[derive(Clone, Copy)]
enum Shorthand {
    Margin,
    Padding,
    Border,
    Background,
}

impl Shorthand {
    fn from_name(name: &str) -> Option<Shorthand> {
        if name.eq_ignore_ascii_case("margin") {
            Some(Shorthand::Margin)
        } else if name.eq_ignore_ascii_case("padding") {
            Some(Shorthand::Padding)
        } else if name.eq_ignore_ascii_case("border") {
            Some(Shorthand::Border)
        } else if name.eq_ignore_ascii_case("background") {
            Some(Shorthand::Background)
        } else {
            None
        }
    }
}

const MARGINS: [PropertyId; 4] = [
    PropertyId::MarginTop,
    PropertyId::MarginRight,
    PropertyId::MarginBottom,
    PropertyId::MarginLeft,
];
const PADDINGS: [PropertyId; 4] = [
    PropertyId::PaddingTop,
    PropertyId::PaddingRight,
    PropertyId::PaddingBottom,
    PropertyId::PaddingLeft,
];
const BORDER_WIDTHS: [PropertyId; 4] = [
    PropertyId::BorderTopWidth,
    PropertyId::BorderRightWidth,
    PropertyId::BorderBottomWidth,
    PropertyId::BorderLeftWidth,
];
const BORDER_STYLE_PROPERTIES: [PropertyId; 4] = [
    PropertyId::BorderTopStyle,
    PropertyId::BorderRightStyle,
    PropertyId::BorderBottomStyle,
    PropertyId::BorderLeftStyle,
];
const BORDER_COLORS: [PropertyId; 4] = [
    PropertyId::BorderTopColor,
    PropertyId::BorderRightColor,
    PropertyId::BorderBottomColor,
    PropertyId::BorderLeftColor,
];

fn is_css_wide(value: &Value) -> bool {
    matches!(value, Value::Inherit | Value::Initial | Value::Unset)
}

/// `margin` / `padding`: one to four values, expanded top, right, bottom, left.
fn expand_box(
    sides: [PropertyId; 4],
    tokens: &[Token<'_>],
    important: bool,
    limits: &CssLimits,
) -> Option<Vec<Declaration>> {
    if let Some(wide) = css_wide(tokens) {
        return Some(
            sides
                .iter()
                .map(|&property| Declaration {
                    property,
                    value: wide.clone(),
                    important,
                })
                .collect(),
        );
    }
    let parts: Vec<&Token<'_>> = tokens
        .iter()
        .filter(|token| !matches!(token, Token::Whitespace))
        .collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut values: Vec<Value> = Vec::with_capacity(4);
    for part in &parts {
        let value = parse_longhand(sides[0], std::slice::from_ref(*part), limits)?;
        if is_css_wide(&value) {
            return None;
        }
        values.push(value);
    }
    let order: [usize; 4] = match values.len() {
        1 => [0, 0, 0, 0],
        2 => [0, 1, 0, 1],
        3 => [0, 1, 2, 1],
        _ => [0, 1, 2, 3],
    };
    Some(
        sides
            .iter()
            .zip(order)
            .map(|(&property, index)| Declaration {
                property,
                value: values[index].clone(),
                important,
            })
            .collect(),
    )
}

fn border_declarations(
    width: &Value,
    style: &Value,
    color: &Value,
    important: bool,
) -> Vec<Declaration> {
    let mut out = Vec::with_capacity(12);
    for ((&w, &s), &c) in BORDER_WIDTHS
        .iter()
        .zip(BORDER_STYLE_PROPERTIES.iter())
        .zip(BORDER_COLORS.iter())
    {
        for (property, value) in [(w, width), (s, style), (c, color)] {
            out.push(Declaration {
                property,
                value: value.clone(),
                important,
            });
        }
    }
    out
}

/// `border: <width> || <style> || <color>`: each at most once, any order.
/// Omitted parts become `initial`.
fn expand_border(
    tokens: &[Token<'_>],
    important: bool,
    limits: &CssLimits,
) -> Option<Vec<Declaration>> {
    if let Some(wide) = css_wide(tokens) {
        return Some(border_declarations(&wide, &wide, &wide, important));
    }
    let literal = |value: Option<Value>| value.filter(|v| !is_css_wide(v));
    let (mut width, mut style, mut color) = (None, None, None);
    let mut index = 0;
    while index < tokens.len() {
        let rest = tokens.get(index..)?;
        let first = rest.first()?;
        if matches!(first, Token::Whitespace) {
            index += 1;
            continue;
        }
        let one = std::slice::from_ref(first);
        if let Some(value) = literal(parse_longhand(PropertyId::BorderTopWidth, one, limits)) {
            if width.replace(value).is_some() {
                return None;
            }
            index += 1;
        } else if let Some(value) = literal(parse_longhand(PropertyId::BorderTopStyle, one, limits))
        {
            if style.replace(value).is_some() {
                return None;
            }
            index += 1;
        } else if let Some((parsed, used)) = parse_color_prefix(rest) {
            if color.replace(Value::Color(parsed)).is_some() {
                return None;
            }
            index += used;
        } else {
            return None;
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return None;
    }
    Some(border_declarations(
        &width.unwrap_or(Value::Initial),
        &style.unwrap_or(Value::Initial),
        &color.unwrap_or(Value::Initial),
        important,
    ))
}

/// `background`: only a color (or `none`) is supported.
fn expand_background(tokens: &[Token<'_>], important: bool) -> Option<Vec<Declaration>> {
    let value = if let Some(wide) = css_wide(tokens) {
        wide
    } else if matches!(single(tokens), Some(Token::Ident(name)) if name.eq_ignore_ascii_case("none"))
    {
        Value::Initial
    } else {
        let (color, used) = parse_color_prefix(tokens)?;
        if used != tokens.len() {
            return None;
        }
        Value::Color(color)
    };
    Some(vec![Declaration {
        property: PropertyId::BackgroundColor,
        value,
        important,
    }])
}

fn expand(
    shorthand: Shorthand,
    tokens: &[Token<'_>],
    important: bool,
    limits: &CssLimits,
) -> Option<Vec<Declaration>> {
    match shorthand {
        Shorthand::Margin => expand_box(MARGINS, tokens, important, limits),
        Shorthand::Padding => expand_box(PADDINGS, tokens, important, limits),
        Shorthand::Border => expand_border(tokens, important, limits),
        Shorthand::Background => expand_background(tokens, important),
    }
}

/// Removes a trailing `!important` and returns what precedes it.
fn strip_important<'t, 'a>(tokens: &'t [Token<'a>]) -> Option<&'t [Token<'a>]> {
    let (last, rest) = trim(tokens).split_last()?;
    let Token::Ident(word) = last else {
        return None;
    };
    if !word.eq_ignore_ascii_case("important") {
        return None;
    }
    let (bang, before) = trim(rest).split_last()?;
    if !matches!(bang, Token::Delim('!')) {
        return None;
    }
    Some(trim(before))
}

/// Parses one declaration. Returns false if it must be dropped.
fn parse_declaration(tokens: &[Token<'_>], limits: &CssLimits, out: &mut Vec<Declaration>) -> bool {
    let Some((Token::Ident(name), rest)) = trim(tokens).split_first() else {
        return false;
    };
    let Some((Token::Colon, value)) = trim(rest).split_first() else {
        return false;
    };
    let mut value = trim(value);
    let mut important = false;
    if let Some(stripped) = strip_important(value) {
        value = stripped;
        important = true;
    }
    if value.is_empty() {
        return false;
    }
    if let Some(property) = PropertyId::from_name(name) {
        return match parse_longhand(property, value, limits) {
            Some(parsed) => {
                out.push(Declaration {
                    property,
                    value: parsed,
                    important,
                });
                true
            }
            None => false,
        };
    }
    if let Some(shorthand) = Shorthand::from_name(name) {
        return match expand(shorthand, value, important, limits) {
            Some(list) => {
                out.extend(list);
                true
            }
            None => false,
        };
    }
    false
}

fn finish_declaration(
    tokens: &[Token<'_>],
    overflow: bool,
    limits: &CssLimits,
    out: &mut ParsedDeclarations,
) {
    if out.exceeded_declaration_limit {
        return;
    }
    if overflow {
        out.dropped += 1;
        return;
    }
    if tokens
        .iter()
        .all(|token| matches!(token, Token::Whitespace))
    {
        return;
    }
    let before = out.declarations.len();
    if !parse_declaration(tokens, limits, &mut out.declarations) {
        out.dropped += 1;
        return;
    }
    if out.declarations.len() > limits.max_declarations_per_rule {
        out.declarations.truncate(before);
        out.exceeded_declaration_limit = true;
    }
}

/// Reads declarations until the end of input, or, with `in_block`, until the
/// closing `}` of the block (which is consumed). Nesting is tracked so that
/// semicolons inside parentheses, brackets, functions and braces do not split
/// declarations. Excess nesting fails the whole parse.
pub(crate) fn parse_declaration_block<'a>(
    tokens: &mut Tokenizer<'a>,
    in_block: bool,
    limits: &CssLimits,
) -> Result<ParsedDeclarations, CssError> {
    let mut out = ParsedDeclarations::default();
    let mut current: Vec<Token<'a>> = Vec::new();
    let mut overflow = false;
    let mut depth = 0usize;
    loop {
        let token = tokens.next();
        let at_end = match &token {
            None => true,
            Some(Token::RightCurly) => in_block && depth == 0,
            Some(_) => false,
        };
        if at_end || (depth == 0 && matches!(token, Some(Token::Semicolon))) {
            finish_declaration(&current, overflow, limits, &mut out);
            current.clear();
            overflow = false;
            if at_end {
                return Ok(out);
            }
            continue;
        }
        let Some(token) = token else {
            return Ok(out);
        };
        match &token {
            Token::Function(_) | Token::LeftParen | Token::LeftSquare | Token::LeftCurly => {
                depth += 1;
                if depth > limits.max_nesting_depth {
                    return Err(CssError::LimitExceeded("nesting depth"));
                }
            }
            Token::RightParen | Token::RightSquare | Token::RightCurly => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        if current.len() < limits.max_value_tokens {
            current.push(token);
        } else {
            overflow = true;
        }
    }
}

/// Parses a declaration list such as the value of a `style` attribute.
pub fn parse_declaration_list(
    text: &str,
    limits: &CssLimits,
) -> Result<ParsedDeclarations, CssError> {
    if text.len() > limits.max_stylesheet_bytes {
        return Err(CssError::InputTooLarge {
            limit: limits.max_stylesheet_bytes,
            actual: text.len(),
        });
    }
    let text = preprocess(text);
    let mut tokenizer = Tokenizer::new(&text);
    let parsed = parse_declaration_block(&mut tokenizer, false, limits)?;
    if parsed.exceeded_declaration_limit {
        return Err(CssError::LimitExceeded("declarations per rule"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{BorderStyle, Color, ColorValue, FontStyle, Length, LineHeight, TextAlign};
    use std::collections::HashSet;

    fn parse(text: &str) -> ParsedDeclarations {
        parse_declaration_list(text, &CssLimits::default()).unwrap()
    }

    fn decl(property: PropertyId, value: Value) -> Declaration {
        Declaration {
            property,
            value,
            important: false,
        }
    }

    fn only(text: &str) -> Declaration {
        let mut parsed = parse(text);
        assert_eq!(parsed.declarations.len(), 1, "{text}");
        assert_eq!(parsed.dropped, 0, "{text}");
        parsed.declarations.remove(0)
    }

    fn dropped(text: &str) {
        let parsed = parse(text);
        assert!(
            parsed.declarations.is_empty(),
            "{text}: {:?}",
            parsed.declarations
        );
        assert_eq!(parsed.dropped, 1, "{text}");
    }

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Value {
        Value::Color(ColorValue::Rgba(Color { r, g, b, a }))
    }

    fn px(value: f32) -> Value {
        Value::Length(Length::Px(value))
    }

    fn pairs(text: &str) -> Vec<(PropertyId, Value)> {
        parse(text)
            .declarations
            .into_iter()
            .map(|d| (d.property, d.value))
            .collect()
    }

    #[test]
    fn named_colors_and_color_keywords() {
        let red = decl(PropertyId::Color, rgba(255, 0, 0, 255));
        assert_eq!(only("color: red"), red);
        assert_eq!(only("COLOR: ReD"), red);
        assert_eq!(
            only("background-color: transparent"),
            decl(PropertyId::BackgroundColor, rgba(0, 0, 0, 0))
        );
        assert_eq!(
            only("color: currentcolor"),
            decl(PropertyId::Color, Value::Color(ColorValue::CurrentColor))
        );
        assert_eq!(only("color: grey").value, rgba(128, 128, 128, 255));
        assert_eq!(only("color: orange").value, rgba(255, 165, 0, 255));
        dropped("color: rebeccapurple");
        dropped("color: red blue");
        dropped("color: 5");
    }

    #[test]
    fn hex_colors() {
        assert_eq!(only("color: #fff").value, rgba(255, 255, 255, 255));
        assert_eq!(only("color: #0f08").value, rgba(0, 255, 0, 136));
        assert_eq!(only("color: #112233").value, rgba(17, 34, 51, 255));
        assert_eq!(only("color: #11223344").value, rgba(17, 34, 51, 68));
        for bad in [
            "color: #12",
            "color: #12345",
            "color: #ggg",
            "color: #1234567",
            "color: #",
        ] {
            dropped(bad);
        }
    }

    #[test]
    fn rgb_functions() {
        assert_eq!(only("color: rgb(255, 0, 0)").value, rgba(255, 0, 0, 255));
        assert_eq!(only("color: rgb(255 0 0)").value, rgba(255, 0, 0, 255));
        assert_eq!(only("color: rgb(100%, 0%, 0%)").value, rgba(255, 0, 0, 255));
        assert_eq!(only("color: rgba(0, 0, 0, 0.5)").value, rgba(0, 0, 0, 128));
        assert_eq!(only("color: rgb(0 0 0 / 50%)").value, rgba(0, 0, 0, 128));
        assert_eq!(only("color: rgb(300, -5, 0)").value, rgba(255, 0, 0, 255));
        assert_eq!(only("color: RGB(1,2,3)").value, rgba(1, 2, 3, 255));
        for bad in [
            "color: rgb(1, 2)",
            "color: rgb(1 2 3 4)",
            "color: rgb(1, 2 3)",
            "color: rgb(a, b, c)",
            "color: hsl(0, 0%, 0%)",
            "color: rgb(1, 2, 3",
            "color: rgb(1,2,3) x",
        ] {
            dropped(bad);
        }
    }

    #[test]
    fn lengths() {
        assert_eq!(only("margin-top: 10px").value, px(10.0));
        assert_eq!(only("margin-top: -5px").value, px(-5.0));
        assert_eq!(
            only("margin-left: 1.5em").value,
            Value::Length(Length::Em(1.5))
        );
        assert_eq!(
            only("width: 50%").value,
            Value::Length(Length::Percent(50.0))
        );
        assert_eq!(only("width: auto").value, Value::Auto);
        assert_eq!(only("width: 0").value, px(0.0));
        assert_eq!(only("margin-top: auto").value, Value::Auto);
        for bad in [
            "padding-top: -1px",
            "height: -1px",
            "width: 10",
            "width: 10pt",
            "width: 1rem",
            "padding-top: auto",
            "width: calc(1px)",
            "width: 1px 2px",
        ] {
            dropped(bad);
        }
    }

    #[test]
    fn keyword_properties() {
        assert_eq!(
            only("display: BLOCK"),
            decl(
                PropertyId::Display,
                Value::Display(crate::value::Display::Block)
            )
        );
        assert_eq!(
            only("display: none").value,
            Value::Display(crate::value::Display::None)
        );
        dropped("display: flex");
        assert_eq!(
            only("text-align: center").value,
            Value::TextAlign(TextAlign::Center)
        );
        dropped("text-align: start");
        assert_eq!(
            only("font-style: italic").value,
            Value::FontStyle(FontStyle::Italic)
        );
        assert_eq!(
            only("border-top-style: dashed"),
            decl(
                PropertyId::BorderTopStyle,
                Value::BorderStyle(BorderStyle::Dashed)
            )
        );
        dropped("border-top-style: wavy");
        assert_eq!(only("border-left-width: thin").value, px(1.0));
        assert_eq!(only("border-left-width: medium").value, px(3.0));
        assert_eq!(only("border-left-width: thick").value, px(5.0));
        assert_eq!(only("border-left-width: 2px").value, px(2.0));
        dropped("border-left-width: 1%");
        dropped("border-left-width: -1px");
    }

    #[test]
    fn font_size_and_weight() {
        assert_eq!(only("font-size: 16px").value, px(16.0));
        assert_eq!(only("font-size: large").value, px(18.0));
        assert_eq!(only("font-size: xx-small").value, px(9.0));
        assert_eq!(
            only("font-size: 1.5em").value,
            Value::Length(Length::Em(1.5))
        );
        assert_eq!(
            only("font-size: 150%").value,
            Value::Length(Length::Percent(150.0))
        );
        dropped("font-size: -1px");
        dropped("font-size: huge");
        assert_eq!(only("font-weight: normal").value, Value::FontWeight(400));
        assert_eq!(only("font-weight: bold").value, Value::FontWeight(700));
        assert_eq!(only("font-weight: 600").value, Value::FontWeight(600));
        dropped("font-weight: bolder");
        dropped("font-weight: 0");
        dropped("font-weight: 1001");
    }

    #[test]
    fn line_height_values() {
        assert_eq!(
            only("line-height: normal").value,
            Value::LineHeight(LineHeight::Normal)
        );
        assert_eq!(
            only("line-height: 1.5").value,
            Value::LineHeight(LineHeight::Number(1.5))
        );
        assert_eq!(
            only("line-height: 20px").value,
            Value::LineHeight(LineHeight::Length(Length::Px(20.0)))
        );
        assert_eq!(
            only("line-height: 120%").value,
            Value::LineHeight(LineHeight::Length(Length::Percent(120.0)))
        );
        assert_eq!(
            only("line-height: 0").value,
            Value::LineHeight(LineHeight::Number(0.0))
        );
        dropped("line-height: -1");
        dropped("line-height: -1px");
        dropped("line-height: foo");
    }

    #[test]
    fn font_family_lists() {
        let families = |text: &str| -> Vec<String> {
            match only(text).value {
                Value::FontFamily(list) => list.iter().map(|name| name.to_string()).collect(),
                other => panic!("expected a font family list, found {other:?}"),
            }
        };
        assert_eq!(
            families("font-family: \"Helvetica Neue\", Arial, sans-serif"),
            ["Helvetica Neue", "Arial", "sans-serif"]
        );
        assert_eq!(
            families("font-family: Times New Roman, serif"),
            ["Times New Roman", "serif"]
        );
        assert_eq!(families("font-family: 'a'"), ["a"]);
        for bad in [
            "font-family: a,,b",
            "font-family: 12px",
            "font-family: ,",
            "font-family: a b, 5",
        ] {
            dropped(bad);
        }
        let sixteen: Vec<String> = (0..16).map(|i| format!("f{i}")).collect();
        assert_eq!(
            families(&format!("font-family: {}", sixteen.join(","))).len(),
            16
        );
        let seventeen: Vec<String> = (0..17).map(|i| format!("f{i}")).collect();
        dropped(&format!("font-family: {}", seventeen.join(",")));
    }

    #[test]
    fn css_wide_keywords() {
        assert_eq!(only("color: inherit").value, Value::Inherit);
        assert_eq!(only("display: INITIAL").value, Value::Initial);
        assert_eq!(only("width: unset").value, Value::Unset);
    }

    #[test]
    fn important_flag() {
        let important = Declaration {
            property: PropertyId::Color,
            value: rgba(255, 0, 0, 255),
            important: true,
        };
        assert_eq!(only("color: red !important"), important);
        assert_eq!(only("color: red!important"), important);
        assert_eq!(only("color: red ! IMPORTANT"), important);
        dropped("color: red !important extra");
        dropped("color: !important");
        dropped("color: red !");
    }

    #[test]
    fn margin_and_padding_shorthands() {
        let four = |a: f32, b: f32, c: f32, d: f32| {
            vec![
                (PropertyId::MarginTop, px(a)),
                (PropertyId::MarginRight, px(b)),
                (PropertyId::MarginBottom, px(c)),
                (PropertyId::MarginLeft, px(d)),
            ]
        };
        assert_eq!(pairs("margin: 10px"), four(10.0, 10.0, 10.0, 10.0));
        assert_eq!(pairs("margin: 1px 2px"), four(1.0, 2.0, 1.0, 2.0));
        assert_eq!(pairs("margin: 1px 2px 3px"), four(1.0, 2.0, 3.0, 2.0));
        assert_eq!(pairs("margin: 1px 2px 3px 4px"), four(1.0, 2.0, 3.0, 4.0));
        assert_eq!(
            pairs("margin: auto 0"),
            vec![
                (PropertyId::MarginTop, Value::Auto),
                (PropertyId::MarginRight, px(0.0)),
                (PropertyId::MarginBottom, Value::Auto),
                (PropertyId::MarginLeft, px(0.0)),
            ]
        );
        assert_eq!(
            pairs("padding: 1px"),
            vec![
                (PropertyId::PaddingTop, px(1.0)),
                (PropertyId::PaddingRight, px(1.0)),
                (PropertyId::PaddingBottom, px(1.0)),
                (PropertyId::PaddingLeft, px(1.0)),
            ]
        );
        assert_eq!(
            pairs("margin: inherit"),
            vec![
                (PropertyId::MarginTop, Value::Inherit),
                (PropertyId::MarginRight, Value::Inherit),
                (PropertyId::MarginBottom, Value::Inherit),
                (PropertyId::MarginLeft, Value::Inherit),
            ]
        );
        for bad in [
            "margin: 1px 2px 3px 4px 5px",
            "margin: 1px inherit",
            "margin: ",
            "padding: 1px -2px",
            "padding: auto",
            "margin: 1px, 2px",
        ] {
            dropped(bad);
        }
    }

    fn side_values_are(declarations: &[Declaration], suffix: &str, expected: &Value) -> bool {
        let matching: Vec<&Declaration> = declarations
            .iter()
            .filter(|d| d.property.name().ends_with(suffix))
            .collect();
        matching.len() == 4 && matching.iter().all(|d| d.value == *expected)
    }

    #[test]
    fn border_shorthand() {
        let red = rgba(255, 0, 0, 255);
        let solid = Value::BorderStyle(BorderStyle::Solid);

        let all = parse("border: 1px solid red").declarations;
        assert_eq!(all.len(), 12);
        assert!(side_values_are(&all, "-width", &px(1.0)));
        assert!(side_values_are(&all, "-style", &solid));
        assert!(side_values_are(&all, "-color", &red));

        let any_order = parse("border: red 2px dashed").declarations;
        assert!(side_values_are(&any_order, "-width", &px(2.0)));
        assert!(side_values_are(
            &any_order,
            "-style",
            &Value::BorderStyle(BorderStyle::Dashed)
        ));
        assert!(side_values_are(&any_order, "-color", &red));

        let style_only = parse("border: solid").declarations;
        assert!(side_values_are(&style_only, "-width", &Value::Initial));
        assert!(side_values_are(&style_only, "-style", &solid));
        assert!(side_values_are(&style_only, "-color", &Value::Initial));

        let zero = parse("border: 0").declarations;
        assert!(side_values_are(&zero, "-width", &px(0.0)));
        assert!(side_values_are(&zero, "-style", &Value::Initial));

        let none = parse("border: none").declarations;
        assert!(side_values_are(
            &none,
            "-style",
            &Value::BorderStyle(BorderStyle::None)
        ));
        assert!(side_values_are(&none, "-width", &Value::Initial));

        let function = parse("border: 1px solid rgb(1, 2, 3)").declarations;
        assert!(side_values_are(&function, "-color", &rgba(1, 2, 3, 255)));

        let inherit = parse("border: inherit").declarations;
        assert_eq!(inherit.len(), 12);
        assert!(inherit.iter().all(|d| d.value == Value::Inherit));

        for bad in [
            "border: 1px 2px",
            "border: 5",
            "border: solid solid",
            "border: red blue",
            "border: 1px inherit",
            "border:",
        ] {
            dropped(bad);
        }
    }

    #[test]
    fn background_shorthand() {
        assert_eq!(
            only("background: red"),
            decl(PropertyId::BackgroundColor, rgba(255, 0, 0, 255))
        );
        assert_eq!(only("background: none").value, Value::Initial);
        assert_eq!(only("background: inherit").value, Value::Inherit);
        dropped("background: url(x.png) red");
        dropped("background: red url(x.png)");
        dropped("background: no-repeat");
    }

    #[test]
    fn unknown_and_invalid_declarations_are_dropped_and_counted() {
        let parsed = parse("foo: bar; color: red; : x; color; --x: 1; background-image: none");
        assert_eq!(parsed.declarations.len(), 1);
        assert_eq!(parsed.dropped, 5);
    }

    #[test]
    fn semicolons_inside_urls_and_blocks_do_not_split() {
        let parsed = parse("color: red; background: url(data:image/png;base64,AA==); width: 1px");
        assert_eq!(parsed.declarations.len(), 2);
        assert_eq!(parsed.dropped, 1);

        let parsed = parse("x: {a;b}; color: blue");
        assert_eq!(parsed.declarations.len(), 1);
        assert_eq!(parsed.dropped, 1);

        let parsed = parse("color: red;;;width: 1px;");
        assert_eq!(parsed.declarations.len(), 2);
        assert_eq!(parsed.dropped, 0);
    }

    #[test]
    fn declaration_limit() {
        let limits = CssLimits::default();
        let parsed = parse_declaration_list(&"color: red;".repeat(256), &limits).unwrap();
        assert_eq!(parsed.declarations.len(), 256);
        assert_eq!(
            parse_declaration_list(&"color: red;".repeat(257), &limits).unwrap_err(),
            CssError::LimitExceeded("declarations per rule")
        );
        // Shorthands count by what they expand to: 21 * 12 = 252, 22 * 12 = 264.
        assert!(parse_declaration_list(&"border: 0;".repeat(21), &limits).is_ok());
        assert_eq!(
            parse_declaration_list(&"border: 0;".repeat(22), &limits).unwrap_err(),
            CssError::LimitExceeded("declarations per rule")
        );
    }

    #[test]
    fn nesting_depth_limit() {
        let limits = CssLimits::default();
        let text = |count: usize| format!("color: {}", "(".repeat(count));
        assert_eq!(
            parse_declaration_list(&text(33), &limits).unwrap_err(),
            CssError::LimitExceeded("nesting depth")
        );
        let parsed = parse_declaration_list(&text(32), &limits).unwrap();
        assert_eq!(parsed.dropped, 1);
    }

    #[test]
    fn very_long_values_are_dropped() {
        let parsed = parse(&format!("color: red{}", " red".repeat(300)));
        assert!(parsed.declarations.is_empty());
        assert_eq!(parsed.dropped, 1);
    }

    #[test]
    fn oversize_input_is_rejected() {
        let limits = CssLimits::default();
        assert_eq!(
            parse_declaration_list(&"a".repeat(1_048_577), &limits).unwrap_err(),
            CssError::InputTooLarge {
                limit: 1_048_576,
                actual: 1_048_577
            }
        );
    }

    #[test]
    fn property_table_is_consistent() {
        assert_eq!(PropertyId::COUNT, 31);
        let mut names = HashSet::new();
        for (i, property) in PropertyId::ALL.iter().enumerate() {
            assert_eq!(property.index(), i);
            assert_eq!(PropertyId::from_name(property.name()), Some(*property));
            assert_eq!(
                PropertyId::from_name(&property.name().to_uppercase()),
                Some(*property)
            );
            names.insert(property.name());
        }
        assert_eq!(names.len(), PropertyId::COUNT);
        assert_eq!(PropertyId::from_name("nonsense"), None);
        assert!(PropertyId::Color.is_inherited());
        assert!(PropertyId::FontFamily.is_inherited());
        assert!(!PropertyId::Display.is_inherited());
        assert!(!PropertyId::MarginTop.is_inherited());
        assert!(!PropertyId::BackgroundColor.is_inherited());
    }

    #[test]
    fn report_value_sizes() {
        // Informational. Run with `-- --nocapture` to see the numbers.
        println!("size_of::<Value>() = {}", std::mem::size_of::<Value>());
        println!(
            "size_of::<Declaration>() = {}",
            std::mem::size_of::<Declaration>()
        );
    }
}
