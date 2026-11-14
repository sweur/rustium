use crate::token::Token;
use crate::value::{Color, ColorValue};

/// CSS 2.1 color names (plus `grey`). Other names are not supported yet.
const NAMED: &[(&str, [u8; 3])] = &[
    ("black", [0, 0, 0]),
    ("silver", [192, 192, 192]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("white", [255, 255, 255]),
    ("maroon", [128, 0, 0]),
    ("red", [255, 0, 0]),
    ("purple", [128, 0, 128]),
    ("fuchsia", [255, 0, 255]),
    ("green", [0, 128, 0]),
    ("lime", [0, 255, 0]),
    ("olive", [128, 128, 0]),
    ("yellow", [255, 255, 0]),
    ("navy", [0, 0, 128]),
    ("blue", [0, 0, 255]),
    ("teal", [0, 128, 128]),
    ("aqua", [0, 255, 255]),
    ("orange", [255, 165, 0]),
];

fn opaque(rgb: [u8; 3]) -> Color {
    Color {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: 255,
    }
}

fn parse_hex(digits: &str) -> Option<Color> {
    let length = digits.len();
    if !matches!(length, 3 | 4 | 6 | 8) {
        return None;
    }
    let mut d = [0u8; 8];
    for (slot, c) in d.iter_mut().zip(digits.chars()) {
        *slot = u8::try_from(c.to_digit(16)?).ok()?;
    }
    Some(if length <= 4 {
        Color {
            r: d[0] * 17,
            g: d[1] * 17,
            b: d[2] * 17,
            a: if length == 4 { d[3] * 17 } else { 255 },
        }
    } else {
        Color {
            r: d[0] * 16 + d[1],
            g: d[2] * 16 + d[3],
            b: d[4] * 16 + d[5],
            a: if length == 8 { d[6] * 16 + d[7] } else { 255 },
        }
    })
}

fn channel(token: &Token<'_>) -> Option<u8> {
    let value = match token {
        Token::Number { value, .. } => *value,
        Token::Percentage(percent) => *percent * 255.0 / 100.0,
        _ => return None,
    };
    Some(value.round().clamp(0.0, 255.0) as u8)
}

fn alpha(token: &Token<'_>) -> Option<u8> {
    let value = match token {
        Token::Number { value, .. } => *value,
        Token::Percentage(percent) => *percent / 100.0,
        _ => return None,
    };
    Some((value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Arguments of `rgb(...)` / `rgba(...)`, comma or space separated.
fn parse_rgb_args(args: &[Token<'_>]) -> Option<Color> {
    let significant: Vec<&Token<'_>> = args
        .iter()
        .filter(|token| !matches!(token, Token::Whitespace))
        .collect();
    let count = significant.len();
    let legacy = significant
        .iter()
        .any(|token| matches!(token, Token::Comma));
    let (shape_ok, has_alpha) = if legacy {
        let commas_ok = (count == 5 || count == 7)
            && significant
                .iter()
                .enumerate()
                .all(|(i, token)| matches!(token, Token::Comma) == (i % 2 == 1));
        (commas_ok, count == 7)
    } else {
        let slash_ok = count == 3 || (count == 5 && matches!(significant[3], Token::Delim('/')));
        (slash_ok, count == 5)
    };
    if !shape_ok {
        return None;
    }
    let stride = if legacy { 2 } else { 1 };
    let alpha_index = if legacy { 6 } else { 4 };
    Some(Color {
        r: channel(significant[0])?,
        g: channel(significant[stride])?,
        b: channel(significant[2 * stride])?,
        a: if has_alpha {
            alpha(significant[alpha_index])?
        } else {
            255
        },
    })
}

/// Parses a color at the start of `tokens` (which must not start with
/// whitespace). Returns the color and how many tokens it used.
pub(crate) fn parse_color_prefix(tokens: &[Token<'_>]) -> Option<(ColorValue, usize)> {
    match tokens.first()? {
        Token::Ident(name) => {
            let color = if name.eq_ignore_ascii_case("currentcolor") {
                ColorValue::CurrentColor
            } else if name.eq_ignore_ascii_case("transparent") {
                ColorValue::Rgba(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                })
            } else {
                let (_, rgb) = NAMED
                    .iter()
                    .find(|(known, _)| name.eq_ignore_ascii_case(known))?;
                ColorValue::Rgba(opaque(*rgb))
            };
            Some((color, 1))
        }
        Token::Hash { value, .. } => parse_hex(value).map(|color| (ColorValue::Rgba(color), 1)),
        Token::Function(name)
            if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") =>
        {
            let end = tokens
                .iter()
                .position(|token| matches!(token, Token::RightParen))?;
            let color = parse_rgb_args(tokens.get(1..end)?)?;
            Some((ColorValue::Rgba(color), end + 1))
        }
        _ => None,
    }
}
