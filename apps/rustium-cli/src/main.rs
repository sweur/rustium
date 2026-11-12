#![forbid(unsafe_code)]

use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use rustium_dom::{Document, DomError, Limits, Namespace, NodeId, NodeKind};
use rustium_html::{ParseLimits, parse};

const USAGE: &str = "usage:
  rustium-cli                          print a small demo tree
  rustium-cli parse <file> [--quiet]   parse an HTML file; print the tree and statistics";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        None | Some("demo") => demo().map_err(|err| err.to_string()),
        Some("parse") => parse_command(args.get(1..).unwrap_or_default()),
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n{USAGE}")),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_command(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut quiet = false;
    for arg in args {
        match arg.as_str() {
            "--quiet" => quiet = true,
            other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
            other => {
                if path.replace(other).is_some() {
                    return Err("only one input file is allowed".to_string());
                }
            }
        }
    }
    let path = path.ok_or("usage: rustium-cli parse <file> [--quiet]")?;

    let limits = ParseLimits::default();
    let bytes = read_bounded(path, limits.max_input_bytes)?;
    if bytes.len() > limits.max_input_bytes {
        return Err(format!(
            "{path} is larger than the {}-byte input limit",
            limits.max_input_bytes
        ));
    }

    let start = Instant::now();
    let parsed = parse(&bytes, &limits).map_err(|err| err.to_string())?;
    let elapsed = start.elapsed();

    if !quiet {
        print_tree(&parsed.document).map_err(|err| err.to_string())?;
    }
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("input bytes: {}", bytes.len());
    println!("nodes: {}", parsed.document.node_count());
    println!("parse errors reported: {}", parsed.parse_errors);
    println!("quirks mode: {:?}", parsed.document.quirks_mode());
    println!(
        "parse time: {:.3} ms ({build} build, informational)",
        elapsed.as_secs_f64() * 1000.0
    );
    Ok(())
}

/// Reads at most `limit + 1` bytes, so an oversized or endless file cannot
/// exhaust memory; the caller rejects anything longer than `limit`.
fn read_bounded(path: &str, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|err| format!("cannot open {path}: {err}"))?;
    let max_read = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(max_read)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("cannot read {path}: {err}"))?;
    Ok(bytes)
}

/// Depth of a node (root = 0), by walking parent links.
fn depth(doc: &Document, id: NodeId) -> Result<usize, DomError> {
    let mut depth = 0;
    let mut cursor = doc.node(id)?.parent();
    while let Some(parent) = cursor {
        depth += 1;
        cursor = doc.node(parent)?.parent();
    }
    Ok(depth)
}

/// Prints one line per node. Everything that came from the document is escaped,
/// so hostile input cannot inject terminal control sequences.
fn print_tree(doc: &Document) -> Result<(), Box<dyn Error>> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for id in doc.descendants(doc.root())? {
        let indent = "  ".repeat(depth(doc, id)?.saturating_sub(1));
        match doc.node(id)?.kind() {
            NodeKind::Document => {}
            NodeKind::Element(el) => {
                let prefix = match el.namespace() {
                    Namespace::Html => "",
                    Namespace::Svg => "svg:",
                    Namespace::MathMl => "math:",
                };
                write!(out, "{indent}<{prefix}{}", el.tag().escape_debug())?;
                for attr in el.attributes() {
                    write!(
                        out,
                        " {}=\"{}\"",
                        attr.name().escape_debug(),
                        attr.value().escape_debug()
                    )?;
                }
                writeln!(out, ">")?;
            }
            NodeKind::Text(text) => writeln!(out, "{indent}\"{}\"", text.escape_debug())?,
        }
    }
    out.flush()?;
    Ok(())
}

fn demo() -> Result<(), DomError> {
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
