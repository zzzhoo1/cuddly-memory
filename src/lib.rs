//! Fidelity port of the TS/JS semantics used by the outbound-filter plugin.
//! Hand-rolled: no derive helpers, deterministic JSON (BTreeMap), byte-exact digest.
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub mod digest;
pub mod usage;

/// Result of digesting a match/limit value expression against the message map.
pub struct DigestOutcome {
    pub value: Option<String>,
    pub digest_hex: String,
    pub blank_key_marker: String,
}

/// Digest a match/limit value: serialize to canonical JSON, then SHA-256.
pub fn digest_message_value(expr: &str, message: &Value) -> DigestOutcome {
    let raw = eval_value_expr(expr, message);
    let marker = match &raw {
        None => "value is null".to_string(),
        Some(v) if v.is_null() => "value is null".to_string(),
        Some(_) => String::new(),
    };
    let value = raw.filter(|v| !v.is_null()).map(|v| to_canonical_json(&v));
    let payload = value.clone().unwrap_or_default();
    let digest_hex = digest::sha256_hex(payload.as_bytes());
    DigestOutcome {
        value,
        digest_hex,
        blank_key_marker: marker,
    }
}

/// Resolve a match/limit value expression against the message map.
/// Supports `$.a.b` dotted paths (a bare `$` returns the whole map), literal
/// fallback otherwise.
pub fn eval_value_expr(expr: &str, message: &Value) -> Option<Value> {
    if expr == "$" {
        return Some(message.clone());
    }
    let key = expr.strip_prefix("$.")?;
    let mut cur = message;
    for part in key.split('.') {
        let obj = cur.as_object()?;
        cur = obj.get(part)?;
    }
    Some(cur.clone())
}

/// Reproduce the plugin's canonical message-map JSON: keys sorted (BTreeMap),
/// only non-null values kept, scalars preserved, arrays/objects re-serialized
/// through the same canonicalizer so nested keys land sorted too.
pub fn canonical_message_map(message: &Map<String, Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (k, v) in message {
        if v.is_null() {
            continue;
        }
        let cleaned = strip_nulls(v);
        out.insert(k.clone(), cleaned);
    }
    out
}

fn strip_nulls(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                if val.is_null() {
                    continue;
                }
                out.insert(k.clone(), strip_nulls(val));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(strip_nulls).collect()),
        other => other.clone(),
    }
}

pub fn to_canonical_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => json_quote(s),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(to_canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", json_quote(k), to_canonical_json(&m[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build the request body the plugin sends to the rate-limit service.
///
/// Shape (optional members appear only when their input is present):
/// `{"now":N,"match":"<digest>","limit":"<digest>"[,"usage":{...}][,"rate":"..."][,"payload":"..."]}`.
/// Usage members are emitted only when parsing yields a usable number; headers
/// that do not parse are dropped (see `usage::parse_number`).
pub fn build_rate_limit_body(
    now: f64,
    usage_headers: &[(String, Value)],
    decision_rate: Option<&str>,
    match_digest: &str,
    limit_digest: &str,
    first_payload: Option<&str>,
) -> String {
    let mut usage = String::new();
    if !usage_headers.is_empty() {
        usage.push_str(",\"usage\":{");
        let parts: Vec<String> = usage_headers
            .iter()
            .filter_map(|(name, raw)| {
                usage::parse_number(raw).map(|n| format!("{}:{}", json_quote(name), fmt_num(n)))
            })
            .collect();
        usage.push_str(&parts.join(","));
        usage.push('}');
    }
    let rate_part = decision_rate
        .map(|r| format!(",\"rate\":{}", json_quote(r)))
        .unwrap_or_default();
    let payload_part = first_payload
        .map(|p| format!(",\"payload\":{}", json_quote(p)))
        .unwrap_or_default();
    format!(
        "{{\"now\":{},\"match\":\"{}\",\"limit\":\"{}\"{}{}{}}}",
        fmt_num(now),
        match_digest,
        limit_digest,
        usage,
        rate_part,
        payload_part
    )
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let mut s = format!("{}", n);
        if s.ends_with(".0") {
            s.truncate(s.len() - 2);
        }
        s
    }
}

/// Merge plugin-level and route-level config maps.
pub fn merge_configs(
    plugin: &Map<String, Value>,
    route: &Map<String, Value>,
) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (k, v) in plugin {
        out.insert(k.clone(), v.clone());
    }
    for (k, v) in route {
        out.insert(k.clone(), v.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn digest_skips_null_and_sorts_keys() {
        let msg = json!({"env": "prod", "user": {"b": 2, "a": 1}, "drop": null});
        let out = digest_message_value("$.user", &msg);
        assert_eq!(out.value.as_deref(), Some("{\"a\":1,\"b\":2}"));
        // sha256('{"a":1,"b":2}')
        assert_eq!(
            out.digest_hex,
            "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[test]
    fn body_minimal_shape() {
        let body = build_rate_limit_body(1000.0, &[], None, "match-d", "limit-d", None);
        let v: Value = serde_json::from_str(&body).expect("body must be valid JSON");
        assert_eq!(
            v,
            json!({"now": 1000, "match": "match-d", "limit": "limit-d"})
        );
    }

    #[test]
    fn body_with_usage_rate_payload() {
        let headers = vec![
            ("tokens".to_string(), json!(5)),
            ("unparsable".to_string(), json!("abc")),
        ];
        let body = build_rate_limit_body(1000.5, &headers, Some("qpm"), "md", "ld", Some("hello"));
        let v: Value = serde_json::from_str(&body).expect("body must be valid JSON");
        assert_eq!(
            v,
            json!({
                "now": 1000.5,
                "match": "md",
                "limit": "ld",
                "usage": {"tokens": 5},
                "rate": "qpm",
                "payload": "hello"
            })
        );
    }

    #[test]
    fn body_usage_empty_when_no_header_parses() {
        let headers = vec![("bad".to_string(), json!("abc"))];
        let body = build_rate_limit_body(1.0, &headers, None, "m", "l", None);
        let v: Value = serde_json::from_str(&body).expect("body must be valid JSON");
        assert_eq!(v["usage"], json!({}), "usage object present but empty");
    }

    #[test]
    fn body_escapes_quoted_strings() {
        let body = build_rate_limit_body(0.0, &[], Some("r\"x"), "m", "l", Some("p\nq"));
        let v: Value = serde_json::from_str(&body).expect("body must be valid JSON");
        assert_eq!(v["rate"], json!("r\"x"));
        assert_eq!(v["payload"], json!("p\nq"));

        // Usage header names go through json_quote as well; a quote inside the
        // name must be escaped by that one call, not quoted a second time —
        // a double quote pair reads as an empty key and then demands a colon.
        let headers = vec![("a\"b".to_string(), json!(7))];
        let body = build_rate_limit_body(0.0, &headers, None, "m", "l", None);
        let v: Value = serde_json::from_str(&body).expect("body must be valid JSON");
        assert_eq!(v["usage"], json!({"a\"b": 7}));
    }
}
