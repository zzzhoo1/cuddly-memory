//! JS `String(number)` 与 decision 数值共用的显示层语义。
//!
//! golden 语料全部为整数（含 0、负数、86400），整数路径 `f64 → i64` 无损。
//! JS 的 `String(n)`：整数值不带 `.0`；小数保留最短往返表示；|n| ≥ 1e21
//! 切换指数记法——语料外边界，保持简单实现并在此记录。

/// JS `String(n)` 的整数/普通小数路径。
pub fn js_number_to_string(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}
