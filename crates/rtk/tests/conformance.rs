//! Conformance suite — port of OmniRoute's `verify.ts`.
//!
//! Runs every vendored built-in filter's embedded `tests[]` through
//! `apply_line_filter` and asserts the output matches `expected` byte-for-byte
//! (after trimming trailing newlines, exactly as `verify.ts::trimComparable`).
//! This is the ground truth that the Rust pipeline matches OmniRoute.

use anyllm_rtk::{apply_line_filter, filters};

/// Mirror `verify.ts::trimComparable`: strip trailing `\n` runs.
fn trim_comparable(s: &str) -> &str {
    s.trim_end_matches('\n')
}

#[test]
fn every_filter_test_passes() {
    let mut failures: Vec<String> = Vec::new();
    let mut total = 0usize;

    for filter in filters() {
        for t in &filter.tests {
            total += 1;
            let actual = apply_line_filter(&t.input, filter, None);
            if trim_comparable(&actual) != trim_comparable(&t.expected) {
                failures.push(format!(
                    "\n[{}] {}\n  expected: {:?}\n  actual:   {:?}",
                    filter.id,
                    t.name,
                    trim_comparable(&t.expected),
                    trim_comparable(&actual),
                ));
            }
        }
    }

    assert!(total > 0, "no inline tests found — filters not vendored?");
    assert!(
        failures.is_empty(),
        "{} of {} filter conformance tests failed:{}",
        failures.len(),
        total,
        failures.join("")
    );
}

#[test]
fn compression_never_grows_output() {
    // verify.ts invariant #5: compressed output is never larger than input.
    for filter in filters() {
        for t in &filter.tests {
            let actual = apply_line_filter(&t.input, filter, None);
            assert!(
                actual.chars().count() <= t.input.chars().count(),
                "filter {} test {} grew output ({} -> {} chars)",
                filter.id,
                t.name,
                t.input.chars().count(),
                actual.chars().count(),
            );
        }
    }
}
