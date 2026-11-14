//! Typed CSS values and the property table.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorValue {
    Rgba(Color),
    CurrentColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Percent(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Hidden,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Length(Length),
}

/// A parsed, validated declaration value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Inherit,
    Initial,
    Unset,
    Auto,
    Display(Display),
    Color(ColorValue),
    Length(Length),
    FontWeight(u16),
    FontStyle(FontStyle),
    FontFamily(Vec<Box<str>>),
    LineHeight(LineHeight),
    TextAlign(TextAlign),
    BorderStyle(BorderStyle),
}

/// Supported longhand properties. Shorthands are expanded while parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyId {
    Display,
    Color,
    BackgroundColor,
    FontSize,
    FontWeight,
    FontStyle,
    FontFamily,
    LineHeight,
    TextAlign,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    Width,
    Height,
}

impl PropertyId {
    /// Every property, in declaration order (so `ALL[p.index()] == p`).
    pub const ALL: [PropertyId; 31] = [
        PropertyId::Display,
        PropertyId::Color,
        PropertyId::BackgroundColor,
        PropertyId::FontSize,
        PropertyId::FontWeight,
        PropertyId::FontStyle,
        PropertyId::FontFamily,
        PropertyId::LineHeight,
        PropertyId::TextAlign,
        PropertyId::MarginTop,
        PropertyId::MarginRight,
        PropertyId::MarginBottom,
        PropertyId::MarginLeft,
        PropertyId::PaddingTop,
        PropertyId::PaddingRight,
        PropertyId::PaddingBottom,
        PropertyId::PaddingLeft,
        PropertyId::BorderTopWidth,
        PropertyId::BorderRightWidth,
        PropertyId::BorderBottomWidth,
        PropertyId::BorderLeftWidth,
        PropertyId::BorderTopStyle,
        PropertyId::BorderRightStyle,
        PropertyId::BorderBottomStyle,
        PropertyId::BorderLeftStyle,
        PropertyId::BorderTopColor,
        PropertyId::BorderRightColor,
        PropertyId::BorderBottomColor,
        PropertyId::BorderLeftColor,
        PropertyId::Width,
        PropertyId::Height,
    ];

    pub const COUNT: usize = Self::ALL.len();

    /// Dense index in `0..COUNT`, for side tables.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PropertyId::Display => "display",
            PropertyId::Color => "color",
            PropertyId::BackgroundColor => "background-color",
            PropertyId::FontSize => "font-size",
            PropertyId::FontWeight => "font-weight",
            PropertyId::FontStyle => "font-style",
            PropertyId::FontFamily => "font-family",
            PropertyId::LineHeight => "line-height",
            PropertyId::TextAlign => "text-align",
            PropertyId::MarginTop => "margin-top",
            PropertyId::MarginRight => "margin-right",
            PropertyId::MarginBottom => "margin-bottom",
            PropertyId::MarginLeft => "margin-left",
            PropertyId::PaddingTop => "padding-top",
            PropertyId::PaddingRight => "padding-right",
            PropertyId::PaddingBottom => "padding-bottom",
            PropertyId::PaddingLeft => "padding-left",
            PropertyId::BorderTopWidth => "border-top-width",
            PropertyId::BorderRightWidth => "border-right-width",
            PropertyId::BorderBottomWidth => "border-bottom-width",
            PropertyId::BorderLeftWidth => "border-left-width",
            PropertyId::BorderTopStyle => "border-top-style",
            PropertyId::BorderRightStyle => "border-right-style",
            PropertyId::BorderBottomStyle => "border-bottom-style",
            PropertyId::BorderLeftStyle => "border-left-style",
            PropertyId::BorderTopColor => "border-top-color",
            PropertyId::BorderRightColor => "border-right-color",
            PropertyId::BorderBottomColor => "border-bottom-color",
            PropertyId::BorderLeftColor => "border-left-color",
            PropertyId::Width => "width",
            PropertyId::Height => "height",
        }
    }

    /// ASCII case-insensitive lookup of a longhand property name.
    pub fn from_name(name: &str) -> Option<PropertyId> {
        Self::ALL
            .iter()
            .copied()
            .find(|property| property.name().eq_ignore_ascii_case(name))
    }

    /// Whether the property inherits from the parent by default.
    pub fn is_inherited(self) -> bool {
        matches!(
            self,
            PropertyId::Color
                | PropertyId::FontSize
                | PropertyId::FontWeight
                | PropertyId::FontStyle
                | PropertyId::FontFamily
                | PropertyId::LineHeight
                | PropertyId::TextAlign
        )
    }
}
