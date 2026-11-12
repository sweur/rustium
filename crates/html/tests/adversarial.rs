use std::fmt::Write as _;

use rustium_dom::{Document, DomError, Limits, NodeKind};
use rustium_html::{ParseError, ParseLimits, parse};

#[path = "common/fixtures.rs"]
mod fixtures;

fn text_lengths(doc: &Document) -> Vec<usize> {
    doc.descendants(doc.root())
        .unwrap()
        .filter_map(|id| match doc.node(id).unwrap().kind() {
            NodeKind::Text(text) => Some(text.len()),
            _ => None,
        })
        .collect()
}

#[test]
fn parse_error_flood_is_counted_and_builds_only_the_skeleton() {
    let input = fixtures::end_tag_flood(200_000);
    let parsed = parse(input.as_bytes(), &ParseLimits::default()).unwrap();
    assert!(parsed.parse_errors > 100_000);
    assert_eq!(parsed.document.node_count(), 4);
}

#[test]
fn one_megabyte_of_text_coalesces_into_a_single_node() {
    let input = fixtures::text_blob(1_000_000);
    let parsed = parse(input.as_bytes(), &ParseLimits::default()).unwrap();
    assert_eq!(text_lengths(&parsed.document), [1_000_000]);
}

#[test]
fn character_reference_flood_coalesces_into_a_single_node() {
    let input = fixtures::entities(100_000);
    let parsed = parse(input.as_bytes(), &ParseLimits::default()).unwrap();
    assert_eq!(text_lengths(&parsed.document), [100_000]);
}

#[test]
fn depth_boundary_is_exact() {
    let limits = ParseLimits::default();
    // html is depth 1 and body depth 2, so 254 nested divs reach depth 256.
    assert!(parse(fixtures::nested("<div>", 254).as_bytes(), &limits).is_ok());
    assert_eq!(
        parse(fixtures::nested("<div>", 255).as_bytes(), &limits).unwrap_err(),
        ParseError::Dom(DomError::DepthLimitExceeded { limit: 256 })
    );
}

#[test]
fn node_boundary_is_exact() {
    let max = Limits::default().max_nodes;
    // root, html, head and body take 4 nodes; each `<i></i>` adds exactly one.
    let fits = "<i></i>".repeat(max - 4);
    let parsed = parse(fits.as_bytes(), &ParseLimits::default()).unwrap();
    assert_eq!(parsed.document.node_count(), max);
    let one_too_many = "<i></i>".repeat(max - 3);
    assert_eq!(
        parse(one_too_many.as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Dom(DomError::NodeLimitExceeded { limit: max })
    );
}

#[test]
fn attribute_cap_boundary_is_exact() {
    let build = |count: usize| {
        let mut input = String::from("<p");
        for i in 0..count {
            write!(input, " a{i}=1").unwrap();
        }
        input.push('>');
        input
    };
    assert!(parse(build(256).as_bytes(), &ParseLimits::default()).is_ok());
    assert_eq!(
        parse(build(257).as_bytes(), &ParseLimits::default()).unwrap_err(),
        ParseError::Dom(DomError::AttributeLimitExceeded { limit: 256 })
    );
}
