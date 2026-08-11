//! HTTP / SSE 握手失败路径金样（`client_sse_protocol`）：同 `fixtures/http_sse_failure_path_golden.jsonl`。

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use super::turn_build::reject_if_client_sse_protocol_invalid;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    kind: String,
    case: Value,
    expect: Value,
}

fn resolve_client_sse_protocol(v: &Value) -> Option<u8> {
    if v.is_null() {
        return None;
    }
    if let Some(n) = v.as_u64() {
        return Some(n as u8);
    }
    let s = v.as_str().expect("client_sse_protocol string or number");
    let supported = crate::sse::protocol::SSE_PROTOCOL_VERSION;
    match s {
        "supported" => Some(supported),
        "too_new" => Some(supported.saturating_add(1)),
        "too_old" => {
            assert!(
                supported > 1,
                "SSE_PROTOCOL_VERSION must be >1 to test mismatch"
            );
            Some(supported - 1)
        }
        other => panic!("unknown client_sse_protocol token {other}"),
    }
}

#[test]
fn golden_http_sse_failure_path_sse_protocol() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/http_sse_failure_path_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut seen = 0usize;
    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        if row.kind != "sse_protocol" {
            continue;
        }
        let ctx = format!("{}:{} ({})", path.display(), line_no + 1, row.id);
        let proto = resolve_client_sse_protocol(&row.case["client_sse_protocol"]);
        let result = reject_if_client_sse_protocol_invalid(proto);
        let ok = row.expect["ok"].as_bool().expect("expect.ok");
        if ok {
            assert!(
                result.is_ok(),
                "{ctx}: expected Ok, got Err(status={})",
                result.as_ref().err().map(|e| e.0.as_u16()).unwrap_or(0)
            );
        } else {
            let err = result.expect_err(&format!("{ctx}: expected Err"));
            let status = row.expect["http_status"].as_u64().expect("http_status") as u16;
            let code = row.expect["code"].as_str().expect("code");
            assert_eq!(err.0.as_u16(), status, "{ctx}: http_status");
            assert_eq!(err.1.0.code, code, "{ctx}: code");
        }
        seen += 1;
    }
    assert!(seen >= 4, "expected sse_protocol cases, got {seen}");
}
