/// Resource limits for CSS processing. Defaults are provisional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssLimits {
    /// Maximum size of one stylesheet in bytes.
    pub max_stylesheet_bytes: usize,
    pub max_rules: usize,
    pub max_selectors_per_rule: usize,
    pub max_compounds_per_selector: usize,
    pub max_simple_selectors_per_compound: usize,
    pub max_declarations_per_rule: usize,
    /// Maximum nesting of blocks, parentheses and functions.
    pub max_nesting_depth: usize,
    /// Maximum tokens in a rule prelude (the selector text).
    pub max_prelude_tokens: usize,
    /// Maximum bytes of a stored identifier (class, id, tag, font family name).
    pub max_name_bytes: usize,
    /// Maximum tokens in one declaration; longer declarations are dropped.
    pub max_value_tokens: usize,
    /// Maximum families in one `font-family` list.
    pub max_font_families: usize,
}

impl Default for CssLimits {
    fn default() -> Self {
        CssLimits {
            max_stylesheet_bytes: 1024 * 1024,
            max_rules: 10_000,
            max_selectors_per_rule: 32,
            max_compounds_per_selector: 8,
            max_simple_selectors_per_compound: 16,
            max_declarations_per_rule: 256,
            max_nesting_depth: 32,
            max_prelude_tokens: 4096,
            max_name_bytes: 256,
            max_value_tokens: 256,
            max_font_families: 16,
        }
    }
}
