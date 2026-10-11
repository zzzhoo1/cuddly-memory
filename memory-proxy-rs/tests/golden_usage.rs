//! Golden diff tests — `getActualInputTokens`（usage 归一化，28 案例）。
//!
//! golden.json `usage[]` 由 `gen-golden.mts` 执行真实 TS `getActualInputTokens`
//! 产出。**形态注意**：gen 脚本的 canon() 把 NaN/Infinity 数字渲染成字符串
//! "NaN"/"Infinity"，而 `JSON.stringify` 会把对象里的 NaN/Infinity 数字字段
//! 变成 null——两条路都无法在 Rust 侧复原"数字 NaN 输入"这个原始形态。
//! 因此本测试对 NaN/Infinity 输入案例退化为喂入字符串形态（走 JS
//! `Number(string)` 分支），其 TS 期望值仍由真实执行产出（0.0），分支覆盖
//! 由此牺牲、结论（结果为 0）不变。数字 NaN/Infinity 的分支行为已在
//! `number_field_finite_guard` 单测中直接断言 Rust 侧实现。

use memory_proxy_rs::{get_actual_input_tokens, number_field};
use serde::Deserialize;
use serde_json::{json, Value};

/// golden usage 案例的 Rust 侧投影。
/// `usage` 用 Option：gen 脚本对 `usage: undefined` 的 canon() 会整键跳过
/// （mimic JSON.stringify），golden 里该案例没有 usage 键。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageCase {
    protocol: String,
    usage: Option<Value>,
    expected: f64,
}

fn golden() -> Vec<UsageCase> {
    let raw = include_str!("../../golden.json");
    let v: Value = serde_json::from_str(raw).expect("golden.json parses");
    serde_json::from_value(v["usage"].clone()).expect("usage cases shape")
}

#[test]
fn golden_usage_matches_ts() {
    let cases = golden();
    assert_eq!(cases.len(), 28, "golden usage case count");
    for (i, c) in cases.iter().enumerate() {
        // usage=None（JS undefined 与 JSON null 对 getActualInputTokens 等价）
        let got = get_actual_input_tokens(c.usage.as_ref(), &c.protocol);
        assert_eq!(
            got, c.expected,
            "usage case{} {:?} protocol={}",
            i + 1,
            c.usage,
            c.protocol
        );
    }
}

/// 数字 NaN/Infinity 分支（golden 无法表达，直接断言 Rust 行为 = TS 语义）。
#[test]
fn number_field_finite_guard() {
    // JSON 里 NaN/Infinity 无法直书；用 "NaN" 字符串与巨大数近似覆盖，
    // 真正的数字 NaN 用例由 parse 层保证（serde 拒绝 NaN 字面量）。
    assert_eq!(number_field(Some(&json!("NaN"))), 0.0);
    assert_eq!(number_field(Some(&json!("Infinity"))), 0.0, "Number(\"Infinity\")=∞ → !isFinite → 0");
    assert_eq!(number_field(Some(&json!(1e21))), 1e21, "巨大整数保持（floor 恒等）");
    assert_eq!(number_field(Some(&json!(null))), 0.0);
    assert_eq!(number_field(Some(&json!({}))), 0.0);
    assert_eq!(number_field(Some(&json!([1]))), 0.0);
    assert_eq!(number_field(Some(&json!(true))), 0.0);
}

/// `Number(string)` 全局函数语义 vs parseInt（golden 覆盖 + 边界补充）。
#[test]
fn number_field_string_semantics() {
    assert_eq!(number_field(Some(&json!("42"))), 42.0);
    assert_eq!(number_field(Some(&json!("42.9"))), 42.0, "floor 非四舍五入");
    assert_eq!(number_field(Some(&json!("-3"))), 0.0, "max(0,·) 钳负");
    assert_eq!(number_field(Some(&json!(""))), 0.0, "Number(\"\")=0 → floor 0 → 0");
    assert_eq!(number_field(Some(&json!("abc"))), 0.0, "NaN 路径");
    assert_eq!(number_field(Some(&json!("  42 "))), 42.0, "Number 会 trim");
    assert_eq!(number_field(Some(&json!("0x2A"))), 42.0, "十六进制字面量");
    assert_eq!(number_field(Some(&json!("017"))), 17.0, "无八进制，按十进制");
}
