//! Validation of longhand property values from tokens.

use crate::color::parse_color_prefix;
use crate::limits::CssLimits;
use crate::token::Token;
use crate::value::{
    BorderStyle, Display, FontStyle, Length, LineHeight, PropertyId, TextAlign, Value,
};

const BORDER_STYLES: [(&str, BorderStyle); 10] = [
    ("none", BorderStyle::None),
    ("hidden", BorderStyle::Hidden),
    ("solid", BorderStyle::Solid),
    ("dashed", BorderStyle::Dashed),
    ("dotted", BorderStyle::Dotted),
    ("double", BorderStyle::Double),
    ("groove", BorderStyle::Groove),
    ("ridge", BorderStyle::Ridge),
    ("inset", BorderStyle::Inset),
    ("outset", BorderStyle::Outset),
];

/// Removes leading and trailing whitespace tokens.
pub(crate) fn trim<'t, 'a>(tokens: &'t [Token<'a>]) -> &'t [Token<'a>] {
    let start = tokens
        .iter()
        .position(|token| !matches!(token, Token::Whitespace))
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|token| !matches!(token, Token::Whitespace))
        .map_or(start, |i| i + 1);
    tokens.get(start..end).unwrap_or(&[])
}

/// The only token of an already trimmed slice.
pub(crate) fn single<'t, 'a>(tokens: &'t [Token<'a>]) -> Option<&'t Token<'a>> {
    match tokens {
        [only] => Some(only),
        _ => None,
    }
}

/// `inherit`, `initial` or `unset` as the entire value.
pub(crate) fn css_wide(tokens: &[Token<'_>]) -> Option<Value> {
    let Some(Token::Ident(name)) = single(tokens) else {
        return None;
    };
    if name.eq_ignore_ascii_case("inherit") {
        Some(Value::Inherit)
    } else if name.eq_ignore_ascii_case("initial") {
        Some(Value::Initial)
    } else if name.eq_ignore_ascii_case("unset") {
        Some(Value::Unset)
    } else {
        None
    }
}

fn keyword<T: Copy>(tokens: &[Token<'_>], table: &[(&str, T)]) -> Option<T> {
    let Some(Token::Ident(name)) = single(tokens) else {
        return None;
    };
    table
        .iter()
        .find(|(known, _)| name.eq_ignore_ascii_case(known))
        .map(|(_, value)| *value)
}

/// Supported units: px, em, %, and a bare zero.
fn length_from_token(token: &Token<'_>) -> Option<Length> {
    match token {
        Token::Number { value, .. } if *value == 0.0 => Some(Length::Px(0.0)),
        Token::Dimension { value, unit, .. } => {
            if unit.eq_ignore_ascii_case("px") {
                Some(Length::Px(*value))
            } else if unit.eq_ignore_ascii_case("em") {
                Some(Length::Em(*value))
            } else {
                None
            }
        }
        Token::Percentage(percent) => Some(Length::Percent(*percent)),
        _ => None,
    }
}

fn is_non_negative(length: Length) -> bool {
    let (Length::Px(value) | Length::Em(value) | Length::Percent(value)) = length;
    value >= 0.0
}

fn auto_or_length(tokens: &[Token<'_>], allow_negative: bool) -> Option<Value> {
    let token = single(tokens)?;
    if let Token::Ident(name) = token {
        return name.eq_ignore_ascii_case("auto").then_some(Value::Auto);
    }
    let length = length_from_token(token)?;
    (allow_negative || is_non_negative(length)).then_some(Value::Length(length))
}

fn padding(tokens: &[Token<'_>]) -> Option<Value> {
    let length = length_from_token(single(tokens)?)?;
    is_non_negative(length).then_some(Value::Length(length))
}

fn whole_color(tokens: &[Token<'_>]) -> Option<Value> {
    let (color, used) = parse_color_prefix(tokens)?;
    (used == tokens.len()).then_some(Value::Color(color))
}

fn border_width(tokens: &[Token<'_>]) -> Option<Value> {
    let token = single(tokens)?;
    if let Token::Ident(name) = token {
        let px = if name.eq_ignore_ascii_case("thin") {
            1.0
        } else if name.eq_ignore_ascii_case("medium") {
            3.0
        } else if name.eq_ignore_ascii_case("thick") {
            5.0
        } else {
            return None;
        };
        return Some(Value::Length(Length::Px(px)));
    }
    match length_from_token(token)? {
        length @ (Length::Px(_) | Length::Em(_)) if is_non_negative(length) => {
            Some(Value::Length(length))
        }
        _ => None,
    }
}

fn font_size(tokens: &[Token<'_>]) -> Option<Value> {
    const KEYWORDS: [(&str, f32); 7] = [
        ("xx-small", 9.0),
        ("x-small", 10.0),
        ("small", 13.0),
        ("medium", 16.0),
        ("large", 18.0),
        ("x-large", 24.0),
        ("xx-large", 32.0),
    ];
    let token = single(tokens)?;
    if let Token::Ident(name) = token {
        return KEYWORDS
            .iter()
            .find(|(known, _)| name.eq_ignore_ascii_case(known))
            .map(|(_, px)| Value::Length(Length::Px(*px)));
    }
    let length = length_from_token(token)?;
    is_non_negative(length).then_some(Value::Length(length))
}

fn font_weight(tokens: &[Token<'_>]) -> Option<Value> {
    match single(tokens)? {
        Token::Ident(name) if name.eq_ignore_ascii_case("normal") => Some(Value::FontWeight(400)),
        Token::Ident(name) if name.eq_ignore_ascii_case("bold") => Some(Value::FontWeight(700)),
        Token::Number { value, .. } if (1.0..=1000.0).contains(value) => {
            Some(Value::FontWeight(value.round() as u16))
        }
        _ => None,
    }
}

fn line_height(tokens: &[Token<'_>]) -> Option<Value> {
    match single(tokens)? {
        Token::Ident(name) if name.eq_ignore_ascii_case("normal") => {
            Some(Value::LineHeight(LineHeight::Normal))
        }
        Token::Number { value, .. } if *value >= 0.0 => {
            Some(Value::LineHeight(LineHeight::Number(*value)))
        }
        other => {
            let length = length_from_token(other)?;
            is_non_negative(length).then_some(Value::LineHeight(LineHeight::Length(length)))
        }
    }
}

fn font_family(tokens: &[Token<'_>], limits: &CssLimits) -> Option<Value> {
    let mut families: Vec<Box<str>> = Vec::new();
    for group in tokens.split(|token| matches!(token, Token::Comma)) {
        let name: String = match trim(group) {
            [] => return None,
            [Token::String(quoted)] => quoted.to_string(),
            words => {
                let mut parts: Vec<&str> = Vec::new();
                for token in words {
                    match token {
                        Token::Ident(word) => parts.push(word),
                        Token::Whitespace => {}
                        _ => return None,
                    }
                }
                if parts.is_empty() {
                    return None;
                }
                parts.join(" ")
            }
        };
        if name.is_empty() || name.len() > limits.max_name_bytes {
            return None;
        }
        families.push(name.into_boxed_str());
        if families.len() > limits.max_font_families {
            return None;
        }
    }
    Some(Value::FontFamily(families))
}

/// Parses the value of one longhand property. `None` means the declaration
/// is invalid and must be dropped.
pub(crate) fn parse_longhand(
    property: PropertyId,
    tokens: &[Token<'_>],
    limits: &CssLimits,
) -> Option<Value> {
    let tokens = trim(tokens);
    if let Some(wide) = css_wide(tokens) {
        return Some(wide);
    }
    match property {
        PropertyId::Display => keyword(
            tokens,
            &[
                ("block", Display::Block),
                ("inline", Display::Inline),
                ("none", Display::None),
            ],
        )
        .map(Value::Display),
        PropertyId::Color
        | PropertyId::BackgroundColor
        | PropertyId::BorderTopColor
        | PropertyId::BorderRightColor
        | PropertyId::BorderBottomColor
        | PropertyId::BorderLeftColor => whole_color(tokens),
        PropertyId::MarginTop
        | PropertyId::MarginRight
        | PropertyId::MarginBottom
        | PropertyId::MarginLeft => auto_or_length(tokens, true),
        PropertyId::PaddingTop
        | PropertyId::PaddingRight
        | PropertyId::PaddingBottom
        | PropertyId::PaddingLeft => padding(tokens),
        PropertyId::BorderTopWidth
        | PropertyId::BorderRightWidth
        | PropertyId::BorderBottomWidth
        | PropertyId::BorderLeftWidth => border_width(tokens),
        PropertyId::BorderTopStyle
        | PropertyId::BorderRightStyle
        | PropertyId::BorderBottomStyle
        | PropertyId::BorderLeftStyle => keyword(tokens, &BORDER_STYLES).map(Value::BorderStyle),
        PropertyId::Width | PropertyId::Height => auto_or_length(tokens, false),
        PropertyId::FontSize => font_size(tokens),
        PropertyId::FontWeight => font_weight(tokens),
        PropertyId::FontStyle => keyword(
            tokens,
            &[
                ("normal", FontStyle::Normal),
                ("italic", FontStyle::Italic),
                ("oblique", FontStyle::Oblique),
            ],
        )
        .map(Value::FontStyle),
        PropertyId::FontFamily => font_family(tokens, limits),
        PropertyId::LineHeight => line_height(tokens),
        PropertyId::TextAlign => keyword(
            tokens,
            &[
                ("left", TextAlign::Left),
                ("right", TextAlign::Right),
                ("center", TextAlign::Center),
                ("justify", TextAlign::Justify),
            ],
        )
        .map(Value::TextAlign),
    }
}
