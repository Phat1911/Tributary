//! D28 fixture contract tests: the tolerant parser must keep accepting
//! every response dialect we have on record (files under
//! `tests/fixtures/jev/`). When the vendor drifts, the weekly live smoke
//! workflow catches it; these fixtures pin OUR parsing behavior so a fix
//! can be developed against the recorded shape, not against production.

use tributary_control::classifier::parse_alignment;

/// Loads one fixture (fails loudly if the file is missing — a silent skip
/// would quietly weaken the contract).
fn fixture(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/jev/{name}");
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

#[test]
fn canonical_dialect_parses() {
    assert_eq!(parse_alignment(&fixture("canonical.json")), Some(0.87));
}

#[test]
fn score_as_string_parses() {
    assert_eq!(
        parse_alignment(&fixture("score-as-string.json")),
        Some(0.31)
    );
}

#[test]
fn nested_result_parses() {
    assert_eq!(parse_alignment(&fixture("nested-result.json")), Some(0.12));
}

#[test]
fn out_of_range_clamps() {
    assert_eq!(parse_alignment(&fixture("clamped.json")), Some(1.0));
}

#[test]
fn vendor_error_body_fails_open() {
    // No alignment anywhere → None (the caller logs and moves on).
    assert_eq!(parse_alignment(&fixture("vendor-error.json")), None);
}

#[test]
fn html_garbage_fails_open() {
    assert_eq!(parse_alignment(&fixture("html-garbage.json")), None);
}
