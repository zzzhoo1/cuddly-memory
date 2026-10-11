//! Golden diff tests — `dimensionField`（6 案例）。
//!
//! `redis-store.ts:324`：`JSON.stringify([instanceId, modelId])`。
//! 覆盖普通串、引号/反斜杠转义、Unicode 原样、控制字符短转义、超长串。

use memory_proxy_rs::dimension_field;
use serde::Deserialize;

#[derive(Deserialize)]
struct DimCase {
    #[serde(rename = "instanceId")]
    instance_id: String,
    #[serde(rename = "modelId")]
    model_id: String,
    expected: String,
}

fn golden() -> Vec<DimCase> {
    let raw = include_str!("../../golden.json");
    let v: serde_json::Value = serde_json::from_str(raw).expect("golden.json parses");
    serde_json::from_value(v["dimension"].clone()).expect("dimension cases shape")
}

#[test]
fn golden_dimension_matches_ts() {
    let cases = golden();
    assert_eq!(cases.len(), 6, "golden dimension case count");
    for (i, c) in cases.iter().enumerate() {
        let got = dimension_field(&c.instance_id, &c.model_id);
        assert_eq!(
            got,
            c.expected,
            "dimension case{}: inst={:?} model={:?}",
            i + 1,
            c.instance_id,
            c.model_id
        );
    }
}

/// golden 外的转义边界：控制字符 → `\u00XX`（JSON.stringify 用小写十六进制）。
#[test]
fn control_chars_escaped_lowercase_hex() {
    assert_eq!(dimension_field("a\u{01}b", "c"), "[\"a\\u0001b\",\"c\"]");
    // 单引号与斜杠不转义（JSON.stringify 语义）
    assert_eq!(dimension_field("a'b/c", "d"), "[\"a'b/c\",\"d\"]");
}
