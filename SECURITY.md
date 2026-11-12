# Security Policy

## Reporting a Vulnerability

To report a vulnerability, simply open a PR or issue with a detailed explanation about what the vulnerability actually is and what changes you made or are needed to patch it. AI-generated code or PRs are accepted, although, please take the time to actually read through the changes you have made and your comments. Thank you

# Security notes

Rustium is a research project. It is not a hardened browser, and nothing here
claims it is secure. This file records the threat model and the known gaps.

## Threat model
All HTML is treated as hostile. Today the only untrusted input is HTML bytes
passed to `rustium_html::parse` (the CLI reads a local file named by its user).

## What exists
- Input size is checked before any parsing work
- Document limits (nodes, depth, string bytes, attributes) are enforced before
  allocation; a breach fails the whole parse
- No recursion on tree data; traversal and drop are iterative
- `#![forbid(unsafe_code)]` in every crate; the only `unsafe` is in test-only
  counting allocators
- The CLI escapes document text before printing it to the terminal
- Dependencies: html5ever 0.40.1 and its transitive crates (26 external crates).
  `cargo audit` scanned the lock file in November 2026 and printed no advisories.
  Re-run it before each release.

## Known gaps
- Logical separation only: everything runs in one process. There is no sandbox.
- No wall-clock limit on parsing; some HTML algorithms may be slow on
  adversarial input. See the benchmark example for measurements.
- html5ever's own transient memory is not covered by the document limits; it is
  bounded only by the input size limit.
- `forbid(unsafe_code)` does not cover dependencies, which may use `unsafe`.
- No fuzzing yet. The tests include a seeded random-input loop on stable Rust.
  Coverage-guided fuzzing (`cargo-fuzz`) needs a nightly toolchain, which is
  outside the current plan.

## Reporting
Open an issue on the project's repository.