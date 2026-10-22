//! Round-2 adversarial probe — unit level: numeric/parse paths of the D28
//! chain. Every test documents the traced behavior with file:line evidence
//! in its assertion messages.

use serde::Deserialize;
use tributary_control::api::PutClassifierRequest;
use tributary_control::classifier::{Dialect, parse_alignment, parse_systemone};
use tributary_gateway::classifier::{MAX_PROMPT_CHARS, extract_prompt};

fn base_put() -> String {
    serde_json::json!({
        "base_url": "http://127.0.0.1:9",
        "model": "jev-align-1",
        "key": "sk-very-secret-key-0123456789",
        "purpose_statement": "coding only",
    })
    .to_string()
}

/// Thresholds as NUMERIC STRINGS must be rejected at deserialization —
/// the bp comparison must never see a string. (attack idea 1)
#[test]
fn t01_string_thresholds_rejected() {
    let raw = format!(
        "{{\"base_url\":\"http://x\",\"model\":\"m\",\"key\":\"sk-12345678\",\"purpose_statement\":\"p\",\"alert_threshold\":\"0.5\"}}"
    );
    let parsed: Result<PutClassifierRequest, _> = serde_json::from_str(&raw);
    assert!(parsed.is_err(), "string threshold must not deserialize");
    // `mode`/`dialect` ARE strings on the wire (validated in the handler).
    // Null vs missing are identical (Option) — both fall to defaults.
    let with_null: PutClassifierRequest = serde_json::from_str(
        r#"{"base_url":"http://x","model":"m","key":"sk-12345678","purpose_statement":"p","alert_threshold":null,"mode":null}"#,
    )
    .unwrap();
    assert!(with_null.alert_threshold.is_none() && with_null.mode.is_none());
}

/// JSON numbers at float extremes: over/underflow behavior through the
/// REAL request struct (what serde feeds threshold_bp).
#[test]
fn t02_float_extremes_trace() {
    let mk = |v: &str| {
        format!(
            "{{\"base_url\":\"http://x\",\"model\":\"m\",\"key\":\"sk-12345678\",\"purpose_statement\":\"p\",\"alert_threshold\":{v}}}"
        )
    };
    // 1e-324 underflows to 0.0 (finite, in range) → bp 0 = "never alert".
    let p: PutClassifierRequest = serde_json::from_str(&mk("1e-324")).unwrap();
    assert_eq!(p.alert_threshold, Some(0.0));
    // 1e309: serde_json behavior — trace it. Either Err (422) or Ok(in)
    // (threshold_bp rejects: !(0.0..=1.0).contains(inf)). Both safe; record.
    let big: Result<PutClassifierRequest, _> = serde_json::from_str(&mk("1e309"));
    match big {
        Err(e) => println!("1e309 → deserialization error: {e}"),
        Ok(p) => {
            let v = p.alert_threshold.unwrap();
            println!("1e309 → Ok({v}) finite={}", v.is_finite());
            assert!(v.is_infinite(), "expected overflow to inf if parsed");
        }
    }
    // -0.0 parses and sits in range (== 0.0) → bp 0.
    let p: PutClassifierRequest = serde_json::from_str(&mk("-0.0")).unwrap();
    assert_eq!(p.alert_threshold, Some(-0.0));
    // Duplicate keys: serde REJECTS the whole body (better than last-wins).
    let dup = r#"{"base_url":"http://x","model":"m","key":"sk-12345678","purpose_statement":"p","mode":"alert","mode":"auto_block"}"#;
    assert!(serde_json::from_str::<PutClassifierRequest>(dup).is_err());
}

/// Vendor-response garbage battery through the public parsers
/// (attack idea 3: malformed vendor JSON shapes).
#[test]
fn t03_vendor_shapes_fail_open_except_finite_data() {
    // noul/alignment as non-numbers → None (fail open).
    for body in [
        br#"{"answers":{"alignment":{"noul":true}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":null}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":[0.5]}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":{"x":0.5}}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":"0.5abc"}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":"0x10"}}}"#.as_slice(),
        br#"{"answers":{"alignment":{"noul":"Infinity"}}}"#.as_slice(),
        br#"{"alignment": true}"#.as_slice(),
        br#"{"alignment": null}"#.as_slice(),
        br#"{"alignment": "high"}"#.as_slice(),
    ] {
        assert_eq!(parse_systemone(body), None, "systemone body {body:?}");
    }
    assert_eq!(parse_alignment(br#"{"alignment": "0.5abc"}"#), None);
    assert_eq!(parse_alignment(br#"{"alignment": true}"#), None);
    // Whitespace-trimmed numeric strings DO parse (documented tolerance).
    assert_eq!(parse_alignment(br#"{"alignment": " 0.90 "}"#), Some(0.9));
    assert_eq!(parse_systemone(br#"{"answers":{"alignment":{"noul":" 0.42 "}}}"#), Some(0.42));
    // 1e-400 as a STRING: Rust's f64 FromStr UNDERFLOWS to Ok(0.0) —
    // finite, so it is accepted as alignment 0.0 (vendor DATA, not an
    // error) → bp 0 at the gateway → worst score. Confirmed by a direct
    // labeled probe (parse_check bin): raw parse Ok(0.0), both parsers
    // Some(0.0). The ERROR path (fail open) is only for non-finite or
    // unparseable values.
    assert_eq!(parse_systemone(br#"{"answers":{"alignment":{"noul":"1e-400"}}}"#), Some(0.0));
    assert_eq!(parse_alignment(br#"{"alignment":"1e-400"}"#), Some(0.0));
    // Bare JSON number form: serde_json accepts it as 0.0 too.
    assert_eq!(parse_alignment(br#"{"alignment":1e-400}"#), Some(0.0));
    // -0.0 finite → clamp keeps -0.0 == 0 numerically → bp 0 at gateway.
    assert_eq!(parse_alignment(br#"{"alignment":-0.0}"#), Some(-0.0));
    // "1e400" overflows to inf → REJECTED (A-014 fix verified).
    assert_eq!(parse_systemone(br#"{"answers":{"alignment":{"noul":"1e400"}}}"#), None);
    assert_eq!(parse_alignment(br#"{"alignment":"1e400"}"#), None);
    // "NaN"/"inf" strings → None (fail open).
    for b in [
        br#"{"alignment":"NaN"}"#.as_slice(),
        br#"{"alignment":"inf"}"#.as_slice(),
        br#"{"alignment":"-Infinity"}"#.as_slice(),
    ] {
        assert_eq!(parse_alignment(b), None);
    }
}

/// Dialect parsing is trim+lowercase at the GATEWAY (unknown → custom),
/// but the PUT validates strictly — unknown dialects never reach the DB.
#[test]
fn t04_dialect_gateway_tolerant() {
    assert_eq!(Dialect::parse("Jev "), Dialect::Jev);
    assert_eq!(Dialect::parse("CUSTOM"), Dialect::Custom);
    assert_eq!(Dialect::parse("garbage"), Dialect::Custom);
}

/// extract_prompt battery (attack idea 3/hot-path panic; A-013 regression).
#[test]
fn t05_extract_prompt_never_panics_or_overshoots() {
    // 3-byte char straddling byte 8192.
    let straddle = serde_json::json!({"messages":[{"role":"user","content":
        format!("{}中", "x".repeat(MAX_PROMPT_CHARS - 2))}]});
    let s = extract_prompt(&straddle);
    assert!(s.chars().all(|c| c == 'x' || c == '中'));
    assert!(s.chars().count() <= MAX_PROMPT_CHARS);
    assert!(s.is_char_boundary(s.len()));
    // Two messages where the cap lands mid-SECOND message: the cap bites
    // inside the second message's 'y' run.
    let two = serde_json::json!({"messages":[
        {"role":"system","content":"x".repeat(MAX_PROMPT_CHARS - 10)},
        {"role":"user","content": format!("{}🎉", "y".repeat(50))}]});
    let s = extract_prompt(&two);
    assert_eq!(s.len(), MAX_PROMPT_CHARS);
    assert!(s.ends_with('y'), "cap lands mid-second-message");
    // 100KB single message → hard cap.
    let big = serde_json::json!({"messages":[{"role":"user","content":"z".repeat(100_000)}]});
    assert_eq!(extract_prompt(&big).len(), MAX_PROMPT_CHARS);
    // Degrading shapes: arrays/null/missing/non-object.
    assert_eq!(extract_prompt(&serde_json::json!({"messages":[{"content":["part"]}]})), "");
    assert_eq!(extract_prompt(&serde_json::json!({"messages":[{"content":null}]})), "");
    assert_eq!(extract_prompt(&serde_json::json!({"messages":"nope"})), "");
    assert_eq!(extract_prompt(&serde_json::json!([1,2,3])), "");
    // base_put string used above proves the JSON helper compiles.
    let _ = base_put();
}
