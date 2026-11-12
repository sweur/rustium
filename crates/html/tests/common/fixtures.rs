//! Deterministic input generators shared by the tests, the heap test and the
//! bench example (included with `#[path]`, so not every includer uses every item).
#![allow(dead_code)]

use std::fmt::Write as _;

/// A realistic page: `blocks` repeated blocks of about 19 nodes and 270 bytes each.
pub fn article(blocks: usize) -> String {
    let mut out = String::from(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Fixture</title></head><body>\n",
    );
    for i in 0..blocks {
        writeln!(
            out,
            "<div class=\"post\"><h2 id=\"p{i}\">Title &amp; subtitle</h2><p>Lorem ipsum dolor sit amet, <a href=\"/link\">consectetur</a> adipiscing <b>elit</b>, sed do eiusmod &copy; tempor.</p><ul><li>one</li><li>two</li><li>three</li></ul></div>\n"
        )
        .unwrap();
    }
    out.push_str("</body></html>\n");
    out
}

/// `<p>` followed by `bytes` bytes of plain text (one huge text node).
pub fn text_blob(bytes: usize) -> String {
    let mut out = String::from("<p>");
    out.push_str(&"lorem ipsum ".repeat(bytes / 12 + 1));
    out.truncate(3 + bytes);
    out
}

/// `<p>` followed by `count` character references (`&amp;`).
pub fn entities(count: usize) -> String {
    format!("<p>{}", "&amp;".repeat(count))
}

/// `pairs` sibling elements with one text child each (2 nodes per pair).
pub fn flat_siblings(pairs: usize) -> String {
    "<i>x</i>".repeat(pairs)
}

/// `elements` paragraphs with `attrs` attributes each.
pub fn attributes_heavy(elements: usize, attrs: usize) -> String {
    let mut out = String::new();
    for _ in 0..elements {
        out.push_str("<p");
        for a in 0..attrs {
            write!(out, " a{a}=v").unwrap();
        }
        out.push('>');
    }
    out
}

/// Misnested formatting elements that exercise the adoption agency.
pub fn adoption_heavy(reps: usize) -> String {
    "<b><i><p></b></i>x".repeat(reps)
}

/// Text inside tables, which the parser must foster-parent.
pub fn foster_heavy(reps: usize) -> String {
    "<table>x<tr><td>y</td></tr></table>".repeat(reps)
}

pub fn repeated_anchors(reps: usize) -> String {
    "<a>x".repeat(reps)
}

/// `count` elements with distinct custom tag names.
pub fn custom_tags(count: usize) -> String {
    let mut out = String::new();
    for i in 0..count {
        write!(out, "<x-{i}>a</x-{i}>").unwrap();
    }
    out
}

pub fn nested(open: &str, count: usize) -> String {
    open.repeat(count)
}

pub fn unclosed_p(count: usize) -> String {
    "<p>".repeat(count)
}

pub fn end_tag_flood(count: usize) -> String {
    "</b>".repeat(count)
}
