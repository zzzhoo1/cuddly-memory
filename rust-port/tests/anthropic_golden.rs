//! Golden diff tests for the 5 anthropic rate-limit responses.
//!
//! Each test pairs `GOLDEN_DECISIONS_ANTHROPIC[i]` with `GOLDEN_RESPONSES_ANTHROPIC[i]`
//! (golden.json emits the two protocols interleaved per decision, so the anthropic
//! entries are the even-then-odd filtering of `responses`).

use anthropic_ratelimit_port::{
    build_rate_limit_response, expected_wire_body, to_f64, Decision, GoldenResponse,
    GOLDEN_DECISIONS_ANTHROPIC, GOLDEN_RESPONSES_ANTHROPIC,
};
use serde_json::Value;

fn load_decisions() -> Vec<Decision> {
    serde_json::from_str(GOLDEN_DECISIONS_ANTHROPIC).expect("embedded decisions must parse")
}

fn load_responses() -> Vec<GoldenResponse> {
    serde_json::from_str(GOLDEN_RESPONSES_ANTHROPIC).expect("embedded responses must parse")
}

fn assert_case(idx: usize) {
    let decisions = load_decisions();
    let responses = load_responses();
    let decision = &decisions[idx];
    let golden = &responses[idx];
    let built = build_rate_limit_response(decision);

    // ---- status ----
    assert_eq!(built.status, golden.status, "case{}: status", idx + 1);
    assert_eq!(built.status, 429, "case{}: TS builder hardcodes 429", idx + 1);

    // ---- headers, key by key (BTreeMap eq is an exact key-set + value match) ----
    assert_eq!(
        built.headers, golden.headers,
        "case{}: headers mismatch\n built={:?}\n golden={:?}",
        idx + 1,
        built.headers,
        golden.headers
    );

    // ---- body ----
    // golden body is a key-sorted canon() rendering, so compare as JSON values.
    let golden_body: Value = golden.body.clone();
    assert_eq!(
        Value::from(serde_json::from_slice::<Value>(&built.body_bytes).unwrap()),
        golden_body,
        "case{}: body mismatch (built bytes as UTF-8: {:?})",
        idx + 1,
        String::from_utf8_lossy(&built.body_bytes)
    );

    // The true wire bytes reconstructed from guard.ts (rule 5).
    assert_eq!(
        built.body_bytes,
        expected_wire_body(decision),
        "case{}: wire body bytes", idx + 1
    );

    // ---- decision field fidelity ----
    // Decision re-serialized must equal the embedded golden decision exactly.
    assert_eq!(
        serde_json::to_value(decision).unwrap(),
        serde_json::to_value(&decisions[idx]).unwrap(),
        "case{}: decision round-trip", idx + 1
    );
}

#[test]
fn case1_qpm_retry_37() {
    // allowed=false, reason=qpm, retryAfterSeconds=37 -> retry-after: "37" verbatim.
    assert_case(0);
}

#[test]
fn case2_tpm_retry_0_clamped_to_1() {
    // retryAfterSeconds=0 -> Math.max(1, 0) === 1 -> "1" (floor is 1, not 0).
    let d = &load_decisions()[1];
    assert_eq!(to_f64(&d.retry_after_seconds), 0.0);
    assert_eq!(d.reason, "tpm");
    assert_case(1);
}

#[test]
fn case3_tpm_retry_86400_verbatim() {
    // retryAfterSeconds=86400 -> emitted verbatim, no upper clamp.
    let d = &load_decisions()[2];
    assert_eq!(to_f64(&d.retry_after_seconds), 86400.0);
    assert_case(2);
}

#[test]
fn case4_qpm_retry_1_zero_limits() {
    // tpm=0, qpm=5, both remaining = 0.
    let d = &load_decisions()[3];
    assert_eq!(to_f64(&d.tpm), 0.0);
    assert_eq!(to_f64(&d.remaining_tokens), 0.0);
    assert_eq!(to_f64(&d.remaining_requests), 0.0);
    assert_case(3);
}

#[test]
fn case9_negative_retry_clamped_to_1() {
    // retryAfterSeconds=-3, degraded={} (canonicalized new Error()).
    // Both retry headers clamp to "1"; degraded adds no header (rule 3).
    let d = &load_decisions()[4];
    assert_eq!(to_f64(&d.retry_after_seconds), -3.0);
    assert_eq!(d.degraded, serde_json::json!({}));
    let built = build_rate_limit_response(d);
    assert_eq!(built.headers.get("retry-after").unwrap(), "1");
    assert_eq!(built.headers.get("x-ratelimit-reset-seconds").unwrap(), "1");
    // No header carries "degraded".
    assert!(!built.headers.keys().any(|k| k.contains("degraded")));
    assert_eq!(built.headers.len(), 7, "exactly 7 headers, degraded adds none");
    assert_case(4);
}