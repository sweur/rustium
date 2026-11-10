//! Checks that dropping a Document returns its Rust-heap allocations.
//! This counts requested heap bytes, NOT process RSS (allocator behavior differs).
//! The `unsafe` below is confined to this test binary; rustium-dom forbids unsafe.
//! One single #[test] on purpose: the counter is global, so parallel tests
//! would pollute each other's measurements.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustium_dom::{Document, Limits};

struct CountingAlloc;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged and only adds bookkeeping on atomics, so System's contract holds.
// Default `realloc`/`alloc_zeroed` route through `alloc`/`dealloc` below.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: caller upholds GlobalAlloc::alloc's contract; forwarded as-is.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: caller upholds GlobalAlloc::dealloc's contract; forwarded as-is.
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

/// Tolerance for unrelated allocations by the test harness's own threads.
const SLACK: usize = 64 * 1024;

fn build(elements: usize) -> Document {
    let mut doc = Document::new(Limits::default()).unwrap();
    let root = doc.root();
    for _ in 0..elements {
        let el = doc.create_element("div").unwrap();
        doc.append_child(root, el).unwrap();
        let text = doc.create_text("hello").unwrap();
        doc.append_child(el, text).unwrap();
    }
    doc
}

#[test]
fn dropping_a_document_releases_its_heap_allocations() {
    // Phase 1: one near-limit document (1 + 2 * 24_999 = 49_999 nodes).
    let baseline = live_bytes();
    let doc = build(24_999);
    assert_eq!(doc.node_count(), 49_999);
    let peak = live_bytes();
    assert!(
        peak.saturating_sub(baseline) > 1_000_000,
        "tree should hold > 1 MB of heap, held {}",
        peak.saturating_sub(baseline)
    );
    drop(doc);
    let after = live_bytes();
    assert!(
        after.saturating_sub(baseline) < SLACK,
        "heap not released: {} bytes above baseline",
        after.saturating_sub(baseline)
    );

    // Phase 2: repeated construction and destruction must not accumulate.
    let baseline = live_bytes();
    for _ in 0..1_000 {
        let doc = build(50);
        drop(doc);
    }
    let after = live_bytes();
    assert!(
        after.saturating_sub(baseline) < SLACK,
        "heap grew across cycles: {} bytes above baseline",
        after.saturating_sub(baseline)
    );
}
