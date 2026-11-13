use rustium_css::{parse_selector_text, preprocess, CssLimits, Tokenizer};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "a", "b", "div", ".", "#", "*", " ", ",", ">", "+", "~", "[", "]", "(", ")", ":", "::", "{",
    "}", ";", "\"", "'", "\\", "\\41 ", "/*", "*/", "url(", "url(x)", "@media", "-", "--", "1",
    "1.5", "-1", "%", "px", "e3", "é", "\u{1F600}", "\0", "\r\n", "\n", "<!--", "-->", "!", "=",
];

fn random_text(rng: &mut XorShift) -> String {
    let mut text = String::new();
    for _ in 0..rng.below(60) {
        text.push_str(PIECES[rng.below(PIECES.len())]);
    }
    text
}

#[test]
fn tokenizer_always_makes_progress() {
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    for case in 0..3000 {
        let raw = random_text(&mut rng);
        let text = preprocess(&raw);
        let count = Tokenizer::new(&text).take(text.len() + 1).count();
        assert!(
            count <= text.len(),
            "case {case}: {count} tokens from {} bytes: {text:?}",
            text.len()
        );
    }
}

#[test]
fn selector_parser_never_panics_and_respects_limits() {
    let limits = CssLimits::default();
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for case in 0..3000 {
        let text = random_text(&mut rng);
        if let Ok(list) = parse_selector_text(&text, &limits) {
            assert!(
                !list.is_empty() && list.len() <= limits.max_selectors_per_rule,
                "case {case}: {text:?}"
            );
            for selector in &list {
                assert!(
                    !selector.compounds.is_empty()
                        && selector.compounds.len() <= limits.max_compounds_per_selector,
                    "case {case}: {text:?}"
                );
            }
        }
    }
}