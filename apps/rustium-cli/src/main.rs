#![forbid(unsafe_code)]

use rustium_dom::{Document, DomError, Limits, NodeId, NodeKind};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

/// Depth of a node (root = 0), by walking parent links. Demo helper only.
fn depth(doc: &Document, id: NodeId) -> Result<usize, DomError> {
    let mut depth = 0;
    let mut cursor = doc.node(id)?.parent();
    while let Some(parent) = cursor {
        depth += 1;
        cursor = doc.node(parent)?.parent();
    }
    Ok(depth)
}

fn run() -> Result<(), DomError> {
    let mut doc = Document::new(Limits::default())?;
    let root = doc.root();

    let html = doc.create_element("html")?;
    doc.append_child(root, html)?;
    let body = doc.create_element("body")?;
    doc.append_child(html, body)?;
    let h1 = doc.create_element("h1")?;
    doc.append_child(body, h1)?;
    let title = doc.create_text("Hello, Rustium")?;
    doc.append_child(h1, title)?;
    let p = doc.create_element("p")?;
    doc.append_child(body, p)?;
    let text = doc.create_text("Phase 1 DOM demo")?;
    doc.append_child(p, text)?;

    println!("#document");
    for id in doc.descendants(root)? {
        let indent = "  ".repeat(depth(&doc, id)?);
        match doc.node(id)?.kind() {
            NodeKind::Document => println!("{indent}#document"),
            NodeKind::Element(el) => println!("{indent}<{}>", el.tag()),
            NodeKind::Text(text) => println!("{indent}\"{text}\""),
        }
    }
    println!("nodes: {}", doc.node_count());
    Ok(())
}
