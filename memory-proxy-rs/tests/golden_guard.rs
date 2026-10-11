//! Golden diff tests — `buildRateLimitResponse` 双协议（10 案例）。
//!
//! golden.json `responses[]` 为 gen 脚本对同一批 5 个 decision 依次跑
//! openai/anthropic 协议的真实 TS 输出（偶数下标 openai，奇数下标
//! anthropic）。body 断言为结构相等（canon 键排序，见 lib.rs 方法论 2），
//! wire 字节另由 `expected_wire_*` 独立断言。

use memory_proxy_rs::{build_rate_limit_response, to_f64, Decision, Protocol};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenResponse {
    status: u16,
    headers: std::collections::BTreeMap<String, String>,
    body: Value,
}

/// golden responses[] 条目自带 decision 与 protocol 字段（gen 脚本同一
/// decision 对象跑两协议，天然配对），case() 直接按下标抽取。
fn case(idx: usize, protocol: Protocol) {
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let entry = &v["responses"][idx];
    assert_eq!(entry["protocol"], json!(if protocol == Protocol::Openai { "openai" } else { "anthropic" }));
    let decision: Decision = serde_json::from_value(entry["decision"].clone()).unwrap();
    let golden: GoldenResponse = serde_json::from_value(entry.clone()).unwrap();

    let built = build_rate_limit_response(protocol, &decision);

    assert_eq!(built.status, golden.status, "case{idx}: status");
    assert_eq!(built.status, 429, "case{idx}: TS 恒 429");
    assert_eq!(
        built.headers, golden.headers,
        "case{idx}: headers\nbuilt={:?}\ngolden={:?}",
        built.headers, golden.headers
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&built.body_bytes).unwrap(),
        golden.body,
        "case{idx}: body 结构（built bytes: {:?}）",
        String::from_utf8_lossy(&built.body_bytes)
    );
}

// ---- openai 半边（本 crate 新覆盖，此前 POC 未移植）----
// golden responses[] 序列：逐 decision 交替 (openai, anthropic)，
// openai = 偶数下标 0/2/4/6/8，anthropic = 奇数下标 1/3/5/7/9。

#[test]
fn openai_case1_qpm_retry_37() {
    case(0, Protocol::Openai);
    // openai 独有 code 字段的 wire 顺序：message, type, code, param（guard.ts:104-110）
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let d: Decision = serde_json::from_value(v["responses"][0]["decision"].clone()).unwrap();
    let built = build_rate_limit_response(Protocol::Openai, &d);
    assert_eq!(
        String::from_utf8(built.body_bytes).unwrap(),
        "{\"error\":{\"message\":\"该模型请求频率已达上限，请稍后重试\",\"type\":\"rate_limit_error\",\"code\":\"qpm_exceeded\",\"param\":null}}"
    );
}

#[test]
fn openai_case2_tpm_retry_0_clamped_to_1() {
    case(2, Protocol::Openai);
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let d: Decision = serde_json::from_value(v["responses"][2]["decision"].clone()).unwrap();
    assert_eq!(to_f64(&d.retry_after_seconds), 0.0);
    assert_eq!(d.reason, "tpm");
}

#[test]
fn openai_case3_tpm_retry_86400_verbatim() {
    case(4, Protocol::Openai);
}

#[test]
fn openai_case4_qpm_zero_limits() {
    case(6, Protocol::Openai);
}

#[test]
fn openai_case9_negative_retry_clamped_degraded_ignored() {
    case(8, Protocol::Openai);
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let d: Decision = serde_json::from_value(v["responses"][8]["decision"].clone()).unwrap();
    let built = build_rate_limit_response(Protocol::Openai, &d);
    assert_eq!(built.headers.get("retry-after").unwrap(), "1");
    assert_eq!(built.headers.len(), 7, "degraded 不产生头");
}

// ---- anthropic 半边（与 POC rust-port 同源用例，双 crate 交叉验证）----

#[test]
fn anthropic_case1() {
    case(1, Protocol::Anthropic);
}

#[test]
fn anthropic_case2() {
    case(3, Protocol::Anthropic);
}

#[test]
fn anthropic_case3() {
    case(5, Protocol::Anthropic);
}

#[test]
fn anthropic_case4() {
    case(7, Protocol::Anthropic);
}

#[test]
fn anthropic_case9() {
    case(9, Protocol::Anthropic);
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let d: Decision = serde_json::from_value(v["responses"][9]["decision"].clone()).unwrap();
    let built = build_rate_limit_response(Protocol::Anthropic, &d);
    assert_eq!(
        String::from_utf8(built.body_bytes).unwrap(),
        "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"该模型请求频率已达上限，请稍后重试\"}}"
    );
}

/// 双协议同 decision：header 全等，仅 body 不同（结构断言一次）。
#[test]
fn same_decision_same_headers_across_protocols() {
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).unwrap();
    let d: Decision = serde_json::from_value(v["responses"][0]["decision"].clone()).unwrap();
    let o = build_rate_limit_response(Protocol::Openai, &d);
    let a = build_rate_limit_response(Protocol::Anthropic, &d);
    assert_eq!(o.headers, a.headers);
    assert_ne!(o.body_bytes, a.body_bytes);
}
