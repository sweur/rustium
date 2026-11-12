//! Informational parse benchmark. Run in release mode:
//!   cargo run --release -p rustium-html --example bench
//! Timings from shared cloud VMs are noisy; compare only runs on the same machine.

#![forbid(unsafe_code)]

#[path = "../tests/common/fixtures.rs"]
mod fixtures;

use std::time::{Duration, Instant};

use rustium_html::{ParseLimits, parse};

struct Case {
    name: &'static str,
    input: String,
    runs: usize,
}

fn cases() -> Vec<Case> {
    let case = |name, input, runs| Case { name, input, runs };
    vec![
        case("article-10k", fixtures::article(38), 20),
        case("article-600k", fixtures::article(2300), 5),
        case("text-4mib", fixtures::text_blob(4_194_104), 3),
        case("entities-1mb", fixtures::entities(200_000), 3),
        case("flat-siblings-48k", fixtures::flat_siblings(24_000), 5),
        case("attrs-100x200", fixtures::attributes_heavy(100, 200), 5),
        case("custom-tags-5000", fixtures::custom_tags(5_000), 5),
        case(
            "nested-tables-50",
            fixtures::nested("<table><tr><td>", 50),
            5,
        ),
        case("deep-div-254", fixtures::nested("<div>", 254), 5),
        case("deep-div-300", fixtures::nested("<div>", 300), 5),
        case("unclosed-p-200k", fixtures::unclosed_p(200_000), 3),
        // Hostile fixtures with unknown cost run once, last. If one runs for
        // more than about 30 seconds, press Ctrl+C and paste what printed.
        case("end-tag-flood-250k", fixtures::end_tag_flood(250_000), 1),
        case(
            "repeated-anchors-15k",
            fixtures::repeated_anchors(15_000),
            1,
        ),
        case("foster-heavy-5k", fixtures::foster_heavy(5_000), 1),
        case("adoption-heavy-3k", fixtures::adoption_heavy(3_000), 1),
    ]
}

fn main() {
    println!("rustium-html parse benchmark (informational; no claim about any other browser)");
    if cfg!(debug_assertions) {
        println!("WARNING: debug build, timings are not meaningful. Use --release.");
    }
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    println!("available_parallelism: {threads}");
    let limits = ParseLimits::default();
    println!(
        "{:<22} {:>10} {:>4} {:>10} {:>10}  result",
        "fixture", "input KiB", "runs", "min ms", "median ms"
    );
    for case in cases() {
        let mut times: Vec<Duration> = Vec::with_capacity(case.runs);
        let mut outcome = String::new();
        for _ in 0..case.runs {
            let start = Instant::now();
            let result = parse(case.input.as_bytes(), &limits);
            times.push(start.elapsed());
            outcome = match &result {
                Ok(parsed) => format!(
                    "ok nodes={} parse_errors={}",
                    parsed.document.node_count(),
                    parsed.parse_errors
                ),
                Err(err) => format!("err: {err}"),
            };
        }
        times.sort();
        let min = times.first().copied().unwrap_or_default();
        let median = times.get(times.len() / 2).copied().unwrap_or_default();
        println!(
            "{:<22} {:>10.1} {:>4} {:>10.3} {:>10.3}  {outcome}",
            case.name,
            case.input.len() as f64 / 1024.0,
            case.runs,
            min.as_secs_f64() * 1000.0,
            median.as_secs_f64() * 1000.0
        );
    }
}
