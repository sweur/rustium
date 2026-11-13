use std::borrow::Cow;

use crate::token::Token;

/// Replaces NUL with U+FFFD, as the CSS preprocessing step requires.
/// Borrows when nothing changes. Call this before tokenizing untrusted text.
pub fn preprocess(input: &str) -> Cow<'_, str> {
    if input.contains('\0') {
        Cow::Owned(input.replace('\0', "\u{FFFD}"))
    } else {
        Cow::Borrowed(input)
    }
}

/// Streaming tokenizer. Every call to `next` consumes at least one byte, so
/// the token count never exceeds the input length.
pub struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
}

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\x0C')
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_non_printable(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0B' | '\x0E'..='\x1F' | '\x7F')
}

fn valid_escape(a: Option<char>, b: Option<char>) -> bool {
    a == Some('\\') && !b.is_some_and(is_newline)
}

fn starts_ident(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('-') => b.is_some_and(|x| is_name_start(x) || x == '-') || valid_escape(b, c),
        Some('\\') => valid_escape(a, b),
        Some(x) => is_name_start(x),
        None => false,
    }
}

fn starts_number(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('+' | '-') => {
            b.is_some_and(|x| x.is_ascii_digit())
                || (b == Some('.') && c.is_some_and(|x| x.is_ascii_digit()))
        }
        Some('.') => b.is_some_and(|x| x.is_ascii_digit()),
        Some(x) => x.is_ascii_digit(),
        None => false,
    }
}

fn finish<'a>(owned: Option<String>, borrowed: &'a str) -> Cow<'a, str> {
    match owned {
        Some(text) => Cow::Owned(text),
        None => Cow::Borrowed(borrowed),
    }
}

impl<'a> Tokenizer<'a> {
    /// `src` should already be passed through `preprocess`.
    pub fn new(src: &'a str) -> Tokenizer<'a> {
        Tokenizer { src, pos: 0 }
    }

    /// Byte offset of the next unread character.
    pub fn position(&self) -> usize {
        self.pos
    }

    fn rest(&self) -> &'a str {
        let src = self.src;
        src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn three_at(&self, pos: usize) -> (Option<char>, Option<char>, Option<char>) {
        let src = self.src;
        let mut chars = src.get(pos..).unwrap_or("").chars();
        (chars.next(), chars.next(), chars.next())
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(is_whitespace) {
            self.pos += 1;
        }
    }

    fn skip_digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    fn skip_comments(&mut self) {
        while self.rest().starts_with("/*") {
            let tail = self.rest().get(2..).unwrap_or("");
            match tail.find("*/") {
                Some(index) => self.pos += 2 + index + 2,
                None => self.pos = self.src.len(),
            }
        }
    }

    /// Consumes the escaped character after a backslash that was already consumed.
    fn consume_escape(&mut self) -> char {
        let Some(c) = self.peek() else {
            return '\u{FFFD}';
        };
        if !c.is_ascii_hexdigit() {
            self.pos += c.len_utf8();
            return c;
        }
        let mut value: u32 = 0;
        let mut digits = 0;
        while digits < 6 {
            let Some(digit) = self.peek().and_then(|ch| ch.to_digit(16)) else {
                break;
            };
            value = value * 16 + digit;
            self.pos += 1;
            digits += 1;
        }
        if let Some(space) = self.peek().filter(|ch| is_whitespace(*ch)) {
            self.pos += space.len_utf8();
            if space == '\r' && self.peek() == Some('\n') {
                self.pos += 1;
            }
        }
        match char::from_u32(value) {
            Some(ch) if value != 0 => ch,
            _ => '\u{FFFD}',
        }
    }

    fn consume_name(&mut self) -> Cow<'a, str> {
        let src = self.src;
        let start = self.pos;
        let mut owned: Option<String> = None;
        loop {
            let (a, b, _) = self.three_at(self.pos);
            match a {
                Some(c) if is_name_char(c) => {
                    if let Some(text) = owned.as_mut() {
                        text.push(c);
                    }
                    self.pos += c.len_utf8();
                }
                Some('\\') if valid_escape(a, b) => {
                    let here = self.pos;
                    let text = owned.get_or_insert_with(|| src.get(start..here).unwrap_or("").to_string());
                    self.pos += 1;
                    let decoded = self.consume_escape();
                    text.push(decoded);
                }
                _ => break,
            }
        }
        finish(owned, src.get(start..self.pos).unwrap_or(""))
    }

    fn consume_number(&mut self) -> (f32, bool) {
        let start = self.pos;
        let mut is_integer = true;
        if matches!(self.peek(), Some('+' | '-')) {
            self.pos += 1;
        }
        self.skip_digits();
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            self.skip_digits();
            is_integer = false;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let has_exponent = match self.peek_at(1) {
                Some(d) if d.is_ascii_digit() => true,
                Some('+' | '-') => self.peek_at(2).is_some_and(|d| d.is_ascii_digit()),
                _ => false,
            };
            if has_exponent {
                self.pos += 1;
                if matches!(self.peek(), Some('+' | '-')) {
                    self.pos += 1;
                }
                self.skip_digits();
                is_integer = false;
            }
        }
        let text = self.src.get(start..self.pos).unwrap_or("0");
        let value = text
            .parse::<f32>()
            .unwrap_or(0.0)
            .clamp(f32::MIN, f32::MAX);
        (value, is_integer)
    }

    fn consume_numeric(&mut self) -> Token<'a> {
        let (value, is_integer) = self.consume_number();
        let (a, b, c) = self.three_at(self.pos);
        if starts_ident(a, b, c) {
            let unit = self.consume_name();
            Token::Dimension {
                value,
                is_integer,
                unit,
            }
        } else if a == Some('%') {
            self.pos += 1;
            Token::Percentage(value)
        } else {
            Token::Number { value, is_integer }
        }
    }

    fn consume_string(&mut self, quote: char) -> Token<'a> {
        let src = self.src;
        self.pos += 1; // opening quote
        let start = self.pos;
        let mut owned: Option<String> = None;
        loop {
            let here = self.pos;
            match self.peek() {
                None => return Token::String(finish(owned, src.get(start..here).unwrap_or(""))),
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Token::String(finish(owned, src.get(start..here).unwrap_or("")));
                }
                Some(c) if is_newline(c) => return Token::BadString,
                Some('\\') => match self.peek_at(1) {
                    None => {
                        // A lone backslash at the end of input is dropped.
                        owned.get_or_insert_with(|| src.get(start..here).unwrap_or("").to_string());
                        self.pos += 1;
                    }
                    Some(c) if is_newline(c) => {
                        // An escaped newline is a line continuation: nothing is added.
                        owned.get_or_insert_with(|| src.get(start..here).unwrap_or("").to_string());
                        self.pos += 1 + c.len_utf8();
                        if c == '\r' && self.peek() == Some('\n') {
                            self.pos += 1;
                        }
                    }
                    Some(_) => {
                        let text = owned
                            .get_or_insert_with(|| src.get(start..here).unwrap_or("").to_string());
                        self.pos += 1;
                        let decoded = self.consume_escape();
                        text.push(decoded);
                    }
                },
                Some(c) => {
                    if let Some(text) = owned.as_mut() {
                        text.push(c);
                    }
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn consume_bad_url_remnants(&mut self) {
        loop {
            let (a, b, _) = self.three_at(self.pos);
            match a {
                None => return,
                Some(')') => {
                    self.pos += 1;
                    return;
                }
                Some('\\') if valid_escape(a, b) => {
                    self.pos += 1;
                    self.consume_escape();
                }
                Some(c) => self.pos += c.len_utf8(),
            }
        }
    }

    /// Consumes an unquoted URL; the `url(` prefix was already consumed.
    fn consume_url(&mut self) -> Token<'a> {
        self.skip_whitespace();
        let src = self.src;
        let start = self.pos;
        let mut owned: Option<String> = None;
        loop {
            let here = self.pos;
            let (a, b, _) = self.three_at(here);
            match a {
                None => return Token::Url(finish(owned, src.get(start..here).unwrap_or(""))),
                Some(')') => {
                    self.pos += 1;
                    return Token::Url(finish(owned, src.get(start..here).unwrap_or("")));
                }
                Some(c) if is_whitespace(c) => {
                    self.skip_whitespace();
                    return match self.peek() {
                        None => Token::Url(finish(owned, src.get(start..here).unwrap_or(""))),
                        Some(')') => {
                            self.pos += 1;
                            Token::Url(finish(owned, src.get(start..here).unwrap_or("")))
                        }
                        Some(_) => {
                            self.consume_bad_url_remnants();
                            Token::BadUrl
                        }
                    };
                }
                Some('"' | '\'' | '(') => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if valid_escape(a, b) {
                        let text = owned
                            .get_or_insert_with(|| src.get(start..here).unwrap_or("").to_string());
                        self.pos += 1;
                        let decoded = self.consume_escape();
                        text.push(decoded);
                    } else {
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                }
                Some(c) => {
                    if let Some(text) = owned.as_mut() {
                        text.push(c);
                    }
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn consume_ident_like(&mut self) -> Token<'a> {
        let name = self.consume_name();
        if self.peek() != Some('(') {
            return Token::Ident(name);
        }
        self.pos += 1;
        if name.eq_ignore_ascii_case("url") {
            let mut probe = self.pos;
            while self
                .src
                .get(probe..)
                .and_then(|s| s.chars().next())
                .is_some_and(is_whitespace)
            {
                probe += 1;
            }
            let quoted = matches!(
                self.src.get(probe..).and_then(|s| s.chars().next()),
                Some('"' | '\'')
            );
            if !quoted {
                return self.consume_url();
            }
        }
        Token::Function(name)
    }

    fn consume_hash(&mut self) -> Token<'a> {
        let (a, b, c) = self.three_at(self.pos + 1);
        if a.is_some_and(is_name_char) || valid_escape(a, b) {
            let is_id = starts_ident(a, b, c);
            self.pos += 1;
            let value = self.consume_name();
            Token::Hash { value, is_id }
        } else {
            self.pos += 1;
            Token::Delim('#')
        }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        self.skip_comments();
        let c = self.peek()?;
        let (a, b, third) = self.three_at(self.pos);
        Some(match c {
            c if is_whitespace(c) => {
                self.skip_whitespace();
                Token::Whitespace
            }
            '"' | '\'' => self.consume_string(c),
            '#' => self.consume_hash(),
            '(' => {
                self.pos += 1;
                Token::LeftParen
            }
            ')' => {
                self.pos += 1;
                Token::RightParen
            }
            '+' | '.' => {
                if starts_number(a, b, third) {
                    self.consume_numeric()
                } else {
                    self.pos += 1;
                    Token::Delim(c)
                }
            }
            ',' => {
                self.pos += 1;
                Token::Comma
            }
            '-' => {
                if starts_number(a, b, third) {
                    self.consume_numeric()
                } else if self.rest().starts_with("-->") {
                    self.pos += 3;
                    Token::Cdc
                } else if starts_ident(a, b, third) {
                    self.consume_ident_like()
                } else {
                    self.pos += 1;
                    Token::Delim('-')
                }
            }
            ':' => {
                self.pos += 1;
                Token::Colon
            }
            ';' => {
                self.pos += 1;
                Token::Semicolon
            }
            '<' => {
                if self.rest().starts_with("<!--") {
                    self.pos += 4;
                    Token::Cdo
                } else {
                    self.pos += 1;
                    Token::Delim('<')
                }
            }
            '@' => {
                let (x, y, z) = self.three_at(self.pos + 1);
                self.pos += 1;
                if starts_ident(x, y, z) {
                    Token::AtKeyword(self.consume_name())
                } else {
                    Token::Delim('@')
                }
            }
            '[' => {
                self.pos += 1;
                Token::LeftSquare
            }
            ']' => {
                self.pos += 1;
                Token::RightSquare
            }
            '{' => {
                self.pos += 1;
                Token::LeftCurly
            }
            '}' => {
                self.pos += 1;
                Token::RightCurly
            }
            '\\' => {
                if valid_escape(a, b) {
                    self.consume_ident_like()
                } else {
                    self.pos += 1;
                    Token::Delim('\\')
                }
            }
            c if c.is_ascii_digit() => self.consume_numeric(),
            c if is_name_start(c) => self.consume_ident_like(),
            c => {
                self.pos += c.len_utf8();
                Token::Delim(c)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Token<'_>> {
        Tokenizer::new(src).collect()
    }

    fn ident(s: &str) -> Token<'_> {
        Token::Ident(s.into())
    }

    fn string(s: &str) -> Token<'_> {
        Token::String(s.into())
    }

    fn int(value: f32) -> Token<'static> {
        Token::Number {
            value,
            is_integer: true,
        }
    }

    fn num(value: f32) -> Token<'static> {
        Token::Number {
            value,
            is_integer: false,
        }
    }

    #[test]
    fn rules_and_whitespace() {
        assert_eq!(
            tokens("a { color : red ; }"),
            vec![
                ident("a"),
                Token::Whitespace,
                Token::LeftCurly,
                Token::Whitespace,
                ident("color"),
                Token::Whitespace,
                Token::Colon,
                Token::Whitespace,
                ident("red"),
                Token::Whitespace,
                Token::Semicolon,
                Token::Whitespace,
                Token::RightCurly,
            ]
        );
        assert_eq!(
            tokens("a \n\t b\r\n c"),
            vec![
                ident("a"),
                Token::Whitespace,
                ident("b"),
                Token::Whitespace,
                ident("c")
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(tokens("a/* x */b"), vec![ident("a"), ident("b")]);
        assert_eq!(tokens("a/* never ends"), vec![ident("a")]);
        assert!(tokens("/**/").is_empty());
        assert_eq!(tokens("/* a *//* b */c"), vec![ident("c")]);
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(tokens(r#""a\"b""#), vec![string("a\"b")]);
        assert_eq!(tokens("'x y'"), vec![string("x y")]);
        assert_eq!(tokens("\"ab\\\ncd\""), vec![string("abcd")]);
        assert_eq!(tokens("\"abc"), vec![string("abc")]);
        assert_eq!(tokens("\"\""), vec![string("")]);
    }

    #[test]
    fn bad_string_stops_at_an_unescaped_newline() {
        assert_eq!(
            tokens("\"ab\ncd"),
            vec![Token::BadString, Token::Whitespace, ident("cd")]
        );
    }

    #[test]
    fn hex_escapes() {
        assert_eq!(tokens("\\41 b"), vec![ident("Ab")]);
        assert_eq!(tokens("\\000041"), vec![ident("A")]);
        assert_eq!(tokens("\\0 x"), vec![ident("\u{FFFD}x")]);
        assert_eq!(tokens("\\110000"), vec![ident("\u{FFFD}")]);
        assert_eq!(tokens("a\\:b"), vec![ident("a:b")]);
    }

    #[test]
    fn numbers_and_units() {
        assert_eq!(
            tokens("12 -3.5 +.5 1e3 1.5E-2 10px 50% 2em"),
            vec![
                int(12.0),
                Token::Whitespace,
                num(-3.5),
                Token::Whitespace,
                num(0.5),
                Token::Whitespace,
                num(1000.0),
                Token::Whitespace,
                num(0.015),
                Token::Whitespace,
                Token::Dimension {
                    value: 10.0,
                    is_integer: true,
                    unit: "px".into()
                },
                Token::Whitespace,
                Token::Percentage(50.0),
                Token::Whitespace,
                Token::Dimension {
                    value: 2.0,
                    is_integer: true,
                    unit: "em".into()
                },
            ]
        );
        assert_eq!(tokens("1."), vec![int(1.0), Token::Delim('.')]);
    }

    #[test]
    fn hash_tokens() {
        assert_eq!(
            tokens("#abc #1 #-x # a"),
            vec![
                Token::Hash {
                    value: "abc".into(),
                    is_id: true
                },
                Token::Whitespace,
                Token::Hash {
                    value: "1".into(),
                    is_id: false
                },
                Token::Whitespace,
                Token::Hash {
                    value: "-x".into(),
                    is_id: true
                },
                Token::Whitespace,
                Token::Delim('#'),
                Token::Whitespace,
                ident("a"),
            ]
        );
    }

    #[test]
    fn at_keywords() {
        assert_eq!(
            tokens("@media @-x @ x"),
            vec![
                Token::AtKeyword("media".into()),
                Token::Whitespace,
                Token::AtKeyword("-x".into()),
                Token::Whitespace,
                Token::Delim('@'),
                Token::Whitespace,
                ident("x"),
            ]
        );
    }

    #[test]
    fn functions_and_urls() {
        assert_eq!(
            tokens("rgb(1,2)"),
            vec![
                Token::Function("rgb".into()),
                int(1.0),
                Token::Comma,
                int(2.0),
                Token::RightParen
            ]
        );
        assert_eq!(
            tokens("url( \"a\" )"),
            vec![
                Token::Function("url".into()),
                Token::Whitespace,
                string("a"),
                Token::Whitespace,
                Token::RightParen
            ]
        );
        assert_eq!(tokens("url(a.png)"), vec![Token::Url("a.png".into())]);
        assert_eq!(tokens("URL(  x  )"), vec![Token::Url("x".into())]);
        assert_eq!(tokens("url(a b)"), vec![Token::BadUrl]);
        assert_eq!(
            tokens("url(data:image/png;base64,AA==)"),
            vec![Token::Url("data:image/png;base64,AA==".into())]
        );
        assert_eq!(tokens("url(a\\)b)"), vec![Token::Url("a)b".into())]);
        assert_eq!(
            tokens("url(a\"b) x"),
            vec![Token::BadUrl, Token::Whitespace, ident("x")]
        );
    }

    #[test]
    fn cdo_and_cdc() {
        assert_eq!(
            tokens("<!-- -->"),
            vec![Token::Cdo, Token::Whitespace, Token::Cdc]
        );
        assert_eq!(
            tokens("<!-"),
            vec![Token::Delim('<'), Token::Delim('!'), Token::Delim('-')]
        );
    }

    #[test]
    fn punctuation_and_delims() {
        assert_eq!(
            tokens("a>b"),
            vec![ident("a"), Token::Delim('>'), ident("b")]
        );
        assert_eq!(
            tokens("{};:,()[]"),
            vec![
                Token::LeftCurly,
                Token::RightCurly,
                Token::Semicolon,
                Token::Colon,
                Token::Comma,
                Token::LeftParen,
                Token::RightParen,
                Token::LeftSquare,
                Token::RightSquare
            ]
        );
        assert_eq!(tokens("--x"), vec![ident("--x")]);
        assert_eq!(tokens("-->"), vec![Token::Cdc]);
        assert_eq!(tokens("-"), vec![Token::Delim('-')]);
        assert_eq!(tokens("+"), vec![Token::Delim('+')]);
    }

    #[test]
    fn non_ascii_names() {
        assert_eq!(tokens("é-\u{1F600}"), vec![ident("é-\u{1F600}")]);
    }

    #[test]
    fn text_is_borrowed_unless_an_escape_was_decoded() {
        assert!(matches!(
            tokens("abc").first(),
            Some(Token::Ident(Cow::Borrowed("abc")))
        ));
        assert!(matches!(
            tokens("a\\62 c").first(),
            Some(Token::Ident(Cow::Owned(_)))
        ));
        assert!(matches!(
            tokens("url(x)").first(),
            Some(Token::Url(Cow::Borrowed("x")))
        ));
        assert!(matches!(
            tokens("\"s\"").first(),
            Some(Token::String(Cow::Borrowed("s")))
        ));
    }

    #[test]
    fn preprocess_replaces_nul() {
        assert_eq!(preprocess("a\0b"), "a\u{FFFD}b");
        assert!(matches!(preprocess("ab"), Cow::Borrowed(_)));
        assert!(matches!(preprocess("a\0b"), Cow::Owned(_)));
    }
}