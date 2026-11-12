//! Measures Rust-heap usage of parsing (NOT process RSS) with a counting
//! allocator, and checks that everything is released after the result is dropped.
//! The `unsafe` below is confined to this test binary; the crates forbid unsafe.
//! The counter is global, so everything runs inside one #[test].
//! Realloc is counted as alloc + free, so peaks are an upper bound.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustium_html::{ParseLimits, parse};

#[path = "common/fixtures.rs"]
mod fixtures;

struct CountingAlloc;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged and only adds bookkeeping on atomics, so System's contract holds.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: caller upholds GlobalAlloc::alloc's contract; forwarded as-is.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: caller upholds GlobalAlloc::dealloc's contract; forwarded as-is.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

/// Tolerance for unrelated allocations by the test harness.
const SLACK: usize = 64 * 1024;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn measure(label: &str, input: &str) {
    let limits = ParseLimits::default();
    let baseline = live();
    PEAK.store(baseline, Ordering::Relaxed);
    let result = parse(input.as_bytes(), &limits);
    let peak_extra = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    let held = live().saturating_sub(baseline);
    let outcome = match &result {
        Ok(parsed) => format!("ok, {} nodes", parsed.document.node_count()),
        Err(err) => format!("err ({err})"),
    };
    drop(result);
    let after = live().saturating_sub(baseline);
    println!(
        "{label:<18} input {:>9} B | peak parse heap +{peak_extra:>10} B | result holds {held:>10} B | after drop +{after:>6} B | {outcome}",
        input.len()
    );
    assert!(
        after < SLACK,
        "{label}: {after} bytes still allocated after the result was dropped"
    );
    assert!(
        peak_extra < 256 * 1024 * 1024,
        "{label}: peak heap blow-up: {peak_extra} bytes"
    );
}

#[test]
fn parsing_releases_everything_it_allocates() {
    // Warm-up: one-time lazy statics (atom tables and the like) must not count as leaks.
    let warmup = parse(b"<p>x</p><x-warm>y</x-warm>", &ParseLimits::default()).unwrap();
    drop(warmup);

    let cases = [
        ("article-10k", fixtures::article(38)),
        ("article-600k", fixtures::article(2300)),
        ("text-1mib", fixtures::text_blob(1 << 20)),
        ("entities-100k", fixtures::entities(100_000)),
        ("flat-siblings", fixtures::flat_siblings(24_000)),
        ("attributes", fixtures::attributes_heavy(100, 200)),
        ("custom-tags-5000", fixtures::custom_tags(5_000)),
        ("nested-div-200", fixtures::nested("<div>", 200)),
        ("nested-div-300", fixtures::nested("<div>", 300)),
    ];
    for (label, input) in &cases {
        measure(label, input);
    }
}
