use std::fmt::Write as _;

use rustium_dom::{Document, DomError, Limits, Namespace, NodeId, NodeKind, QuirksMode};
use rustium_html::{ParseError, ParseLimits, Parsed, parse};

fn parse_default(input: &str) -> Parsed {
    parse(input.as_bytes(), &ParseLimits::default()).unwrap()
}

fn depth(doc: &Document, id: NodeId) -> usize {
    let mut depth = 0;
    let mut cursor = doc.node(id).unwrap().parent();
    while let Some(parent) = cursor {
        depth += 1;
        cursor = doc.node(parent).unwrap().parent();
    }
    depth
}

/// One line per node, indented by depth. Non-HTML elements get a namespace prefix.
fn dump(doc: &Document) -> String {
    let mut out = String::new();
    for id in doc.descendants(doc.root()).unwrap() {
        let indent = "  ".repeat(depth(doc, id) - 1);
        match doc.node(id).unwrap().kind() {
            NodeKind::Document => {}
            NodeKind::Element(el) => {
                let prefix = match el.namespace() {
                    Namespace::Html => "",
                    Namespace::Svg => "svg:",
                    Namespace::MathMl => "math:",
                };
                write!(out, "{indent}<{prefix}{}", el.tag()).unwrap();
                for attr in el.attributes() {
                    write!(out, " {}=\"{}\"", attr.name(), attr.value()).unwrap();
                }
                out.push_str(">\n");
            }
            NodeKind::Text(text) => writeln!(out, "{indent}\"{text}\"").unwrap(),
        }
    }
    out
}

fn text_content(doc: &Document) -> String {
    doc.descendants(doc.root())
        .unwrap()
        .filter_map(|id| match doc.node(id).unwrap().kind() {
            NodeKind::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn find_element(doc: &Document, tag: &str) -> NodeId {
    doc.descendants(doc.root())
        .unwrap()
        .find(
            |id| matches!(doc.node(*id).unwrap().kind(), NodeKind::Element(el) if el.tag() == tag),
        )
        .unwrap_or_else(|| panic!("no <{tag}> element"))
}

#[test]
fn empty_input_builds_the_skeleton() {
    let parsed = parse_default("");
    assert_eq!(dump(&parsed.document), "<html>\n  <head>\n  <body>\n");
    assert_eq!(parsed.document.node_count(), 4);
}

#[test]
fn simple_document() {
    let parsed = parse_default("<p>Hello</p>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    <p>\n      \"Hello\"\n"
    );
}

#[test]
fn quirks_mode_follows_the_doctype() {
    let with = parse_default("<!DOCTYPE html><p>x</p>");
    assert_eq!(with.document.quirks_mode(), QuirksMode::NoQuirks);
    let without = parse_default("<p>x</p>");
    assert_eq!(without.document.quirks_mode(), QuirksMode::Quirks);
}

#[test]
fn comments_doctypes_and_pis_are_not_represented() {
    let parsed = parse_default("<!DOCTYPE html><!-- c --><p>x</p><?pi data>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    <p>\n      \"x\"\n"
    );
    assert_eq!(parsed.document.node_count(), 6);
}

#[test]
fn adjacent_text_is_coalesced_into_one_node() {
    let parsed = parse_default("<p>a&amp;b&lt;c</p>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    <p>\n      \"a&b<c\"\n"
    );
}

const FIXTURE: &str = "<!DOCTYPE html><html lang=\"en\"><head><title>T &amp; é☃😀</title></head>\
<body class=\"a b\"><p id=x>one<b>two</b>&copy; three</p><!-- c --><table><tr><td>cell</td></tr></table>\
<svg width=\"10\"><circle r=\"1\"/></svg></body></html>";

#[test]
fn chunk_boundaries_do_not_change_the_tree() {
    let expected = dump(&parse_default(FIXTURE).document);
    assert!(expected.contains("T & é☃😀"));
    for chunk_bytes in [1, 2, 3, 5, 7, 64, 4096] {
        let limits = ParseLimits {
            chunk_bytes,
            ..ParseLimits::default()
        };
        let parsed = parse(FIXTURE.as_bytes(), &limits).unwrap();
        assert_eq!(
            dump(&parsed.document),
            expected,
            "chunk_bytes = {chunk_bytes}"
        );
    }
}

#[test]
fn invalid_utf8_becomes_a_replacement_character() {
    let parsed = parse(b"<p>a\xFFb</p>", &ParseLimits::default()).unwrap();
    assert!(dump(&parsed.document).contains("\"a\u{FFFD}b\""));
}

#[test]
fn attributes_are_kept_in_order_and_duplicates_dropped() {
    let parsed = parse_default("<a href=\"/x\" class=\"c d\" id=1 id=2 title=\"a&amp;b\">t</a>");
    let doc = &parsed.document;
    let a = find_element(doc, "a");
    let NodeKind::Element(el) = doc.node(a).unwrap().kind() else {
        panic!("expected an element");
    };
    let names: Vec<&str> = el.attributes().iter().map(|attr| attr.name()).collect();
    assert_eq!(names, ["href", "class", "id", "title"]);
    assert_eq!(el.attribute("id"), Some("1"));
    assert_eq!(el.attribute("class"), Some("c d"));
    assert_eq!(el.attribute("title"), Some("a&b"));
}

#[test]
fn foreign_content_keeps_namespaces() {
    let parsed = parse_default("<svg width=\"10\"><circle r=\"1\"/></svg><math><mi>x</mi></math>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    <svg:svg width=\"10\">\n      <svg:circle r=\"1\">\n    <math:math>\n      <math:mi>\n        \"x\"\n"
    );
}

#[test]
fn template_contents_become_children_of_the_template() {
    let parsed = parse_default("<template><p>x</p></template>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n    <template>\n      <p>\n        \"x\"\n  <body>\n"
    );
}

#[test]
fn foster_parenting_moves_text_before_the_table() {
    let parsed = parse_default("<table>x<tr><td>y</td></tr></table>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    \"x\"\n    <table>\n      <tbody>\n        <tr>\n          <td>\n            \"y\"\n"
    );
}

#[test]
fn misnested_formatting_keeps_all_text_in_order() {
    let parsed = parse_default("<b><p>x</b>y</p><b><i>a</b>b</i>c");
    assert_eq!(text_content(&parsed.document), "xyabc");
    assert!(parsed.document.validate_depth().is_ok());
}

#[test]
fn script_text_is_kept_raw() {
    let parsed = parse_default("<script>if (a < b && c) {}</script>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n    <script>\n      \"if (a < b && c) {}\"\n  <body>\n"
    );
}

#[test]
fn noscript_is_markup_because_scripting_is_disabled() {
    let parsed = parse_default("<body><noscript><p>x</p></noscript></body>");
    assert_eq!(
        dump(&parsed.document),
        "<html>\n  <head>\n  <body>\n    <noscript>\n      <p>\n        \"x\"\n"
    );
}

#[test]
fn repeated_html_and_body_tags_merge_missing_attributes() {
    let parsed = parse_default("<html a=1><body b=2><html c=3 a=9><body d=4>x");
    assert_eq!(
        dump(&parsed.document),
        "<html a=\"1\" c=\"3\">\n  <head>\n  <body b=\"2\" d=\"4\">\n    \"x\"\n"
    );
}

#[test]
fn parse_errors_are_counted() {
    let parsed = parse_default("</b></b></b>");
    assert!(parsed.parse_errors > 0);
}

#[test]
fn invalid_limits_are_rejected() {
    let zero_chunk = ParseLimits {
        chunk_bytes: 0,
        ..ParseLimits::default()
    };
    assert_eq!(
        parse(b"x", &zero_chunk).unwrap_err(),
        ParseError::InvalidLimits
    );
    let zero_nodes = ParseLimits {
        dom: Limits {
            max_nodes: 0,
            ..Limits::default()
        },
        ..ParseLimits::default()
    };
    assert_eq!(
        parse(b"x", &zero_nodes).unwrap_err(),
        ParseError::Dom(DomError::InvalidLimits)
    );
}

#[test]
fn oversize_input_is_rejected_before_parsing() {
    let limits = ParseLimits {
        max_input_bytes: 10,
        ..ParseLimits::default()
    };
    assert_eq!(
        parse(&[b'a'; 11], &limits).unwrap_err(),
        ParseError::InputTooLarge {
            limit: 10,
            actual: 11
        }
    );
    assert!(parse(&[b'a'; 10], &limits).is_ok());
}

#[test]
fn node_limit_fails_closed() {
    let limits = ParseLimits {
        dom: Limits {
            max_nodes: 10,
            ..Limits::default()
        },
        ..ParseLimits::default()
    };
    let input = "<p>".repeat(20);
    assert_eq!(
        parse(input.as_bytes(), &limits).unwrap_err(),
        ParseError::Dom(DomError::NodeLimitExceeded { limit: 10 })
    );
}

#[test]
fn depth_limit_fails_closed() {
    let deep = "<div>".repeat(300);
    assert_eq!(
        parse(deep.as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Dom(DomError::DepthLimitExceeded { limit: 256 })
    );
    let ok = "<div>".repeat(200);
    assert!(parse(ok.as_bytes(), &ParseLimits::default()).is_ok());
}

#[test]
fn attribute_flood_fails_closed() {
    let mut input = String::from("<p ");
    for i in 0..300 {
        write!(input, "a{i}=1 ").unwrap();
    }
    input.push('>');
    assert_eq!(
        parse(input.as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Dom(DomError::AttributeLimitExceeded { limit: 256 })
    );
}

#[test]
fn string_budget_fails_closed() {
    let limits = ParseLimits {
        dom: Limits {
            max_string_bytes: 100,
            ..Limits::default()
        },
        chunk_bytes: 16,
        ..ParseLimits::default()
    };
    let input = format!("<p>{}", "x".repeat(200));
    assert_eq!(
        parse(input.as_bytes(), &limits).unwrap_err(),
        ParseError::Dom(DomError::StringBudgetExceeded { limit: 100 })
    );
}

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
    "<",
    ">",
    "/",
    "!",
    "-",
    "=",
    "\"",
    "'",
    " ",
    "&",
    ";",
    "a",
    "b",
    "i",
    "p",
    "x",
    "div",
    "table",
    "tr",
    "td",
    "tbody",
    "svg",
    "math",
    "template",
    "select",
    "option",
    "script",
    "style",
    "title",
    "<!--",
    "-->",
    "<![CDATA[",
    "]]>",
    "&amp;",
    "&#x",
    "é",
    "\u{FFFD}",
    "\n",
    "<b>",
    "</b>",
    "<p>",
    "</p>",
    "<table>",
    "</table>",
    "<td>",
    "<tr>",
    "<svg>",
    "<template>",
    "</template>",
    "<a href=x>",
    "</a>",
    "<html a=1>",
    "<body c=2>",
];

#[test]
fn pseudo_random_inputs_never_panic_and_respect_limits() {
    let dom = Limits {
        max_nodes: 400,
        max_depth: 32,
        max_string_bytes: 8192,
        max_attributes_per_element: 8,
    };
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for case in 0..400 {
        let mut input = Vec::new();
        for _ in 0..rng.below(120) {
            if rng.below(10) == 0 {
                input.push((rng.next() & 0xFF) as u8); // raw byte, may be invalid UTF-8
            } else {
                input.extend_from_slice(PIECES[rng.below(PIECES.len())].as_bytes());
            }
        }
        for chunk_bytes in [64, 3] {
            let limits = ParseLimits {
                dom,
                chunk_bytes,
                ..ParseLimits::default()
            };
            match parse(&input, &limits) {
                Ok(parsed) => {
                    assert!(parsed.document.node_count() <= 400, "case {case}");
                    assert!(parsed.document.validate_depth().is_ok(), "case {case}");
                }
                Err(err) => assert!(
                    matches!(
                        err,
                        ParseError::Dom(
                            DomError::NodeLimitExceeded { .. }
                                | DomError::DepthLimitExceeded { .. }
                                | DomError::StringBudgetExceeded { .. }
                                | DomError::AttributeLimitExceeded { .. }
                        )
                    ),
                    "case {case}: unexpected error {err:?} for input {input:?}"
                ),
            }
        }
    }
}
