//! D28 fixture contract tests: the tolerant parser must keep accepting
//! every response dialect we have on record (files under
//! `tests/fixtures/jev/`). When the vendor drifts, the weekly live smoke
//! workflow catches it; these fixtures pin OUR parsing behavior so a fix
//! can be developed against the recorded shape, not against production.

use tributary_control::classifier::{parse_alignment, parse_systemone};

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

// ---- Jev systemone dialect (director research 2026-10-21) ------------------
//
// Fixtures are built from the documented example shapes: the score lives
// at `answers.alignment.noul`. These pin OUR parsing so vendor drift is
// caught by the weekly smoke, not by silence.

#[test]
fn systemone_canonical_parses() {
    assert_eq!(parse_systemone(&fixture("systemone.json")), Some(0.87));
}

#[test]
fn systemone_noul_as_string_with_siblings_parses() {
    // Numeric-string noul + extra sibling keys/answers: still 0.31, and
    // the decoy `other.noul` must NOT win (the documented path wins).
    assert_eq!(
        parse_systemone(&fixture("systemone-noul-string.json")),
        Some(0.31)
    );
}

#[test]
fn systemone_out_of_range_clamps() {
    assert_eq!(
        parse_systemone(&fixture("systemone-clamped.json")),
        Some(1.0)
    );
}

#[test]
fn systemone_wrong_path_fails_open() {
    // `alignment.noul` at top level or `answers.noul` are NOT the
    // documented path — no score (fail-open), never a guessed one.
    assert_eq!(parse_systemone(&fixture("systemone-wrong-path.json")), None);
}
