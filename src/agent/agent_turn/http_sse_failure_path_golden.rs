//! HTTP / SSE 失败路径契约金样：`fixtures/http_sse_failure_path_golden.jsonl`。
//!
//! 覆盖 `RunAgentTurnError` → 公共码 / HTTP 状态 / SSE 与 `ApiError` 分流，以及
//! `http_api_constant` 文档对照（队列满、流任务消失等握手类码）。

use std::fs;
use std::path::PathBuf;

use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::agent::agent_turn::host::errors::{
    AgentTurnJobOutcomeKind, AgentTurnSubPhase, RunAgentTurnError, TurnAbortReason,
};
use crate::llm::{LlmCallError, LlmCompleteError};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    kind: String,
    case: Value,
    expect: Value,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_phase(s: &str) -> AgentTurnSubPhase {
    match s {
        "planner" => AgentTurnSubPhase::Planner,
        "executor" => AgentTurnSubPhase::Executor,
        "reflect" => AgentTurnSubPhase::Reflect,
        other => panic!("unknown phase {other}"),
    }
}

fn build_turn_error(case: &Value) -> RunAgentTurnError {
    let variant = case["variant"].as_str().expect("case.variant");
    let phase = parse_phase(case["phase"].as_str().unwrap_or("planner"));
    match variant {
        "llm_cancelled" => RunAgentTurnError::Llm {
            phase,
            kind: LlmCompleteError::Cancelled,
        },
        "llm_transport" => {
            let status = case["http_status"].as_u64().expect("http_status") as u16;
            let msg = case["user_message"]
                .as_str()
                .unwrap_or("upstream")
                .to_string();
            RunAgentTurnError::Llm {
                phase,
                kind: LlmCompleteError::Transport(LlmCallError::from_http_api(status, msg)),
            }
        }
        "llm_other" => {
            let msg = case["message"].as_str().unwrap_or("other").to_string();
            RunAgentTurnError::Llm {
                phase,
                kind: LlmCompleteError::Other(Box::new(std::io::Error::other(msg))),
            }
        }
        "turn_aborted_sse" => RunAgentTurnError::TurnAborted {
            phase,
            reason: TurnAbortReason::SseDisconnected,
        },
        "turn_aborted_user" => RunAgentTurnError::TurnAborted {
            phase,
            reason: TurnAbortReason::UserCancelled,
        },
        "other" => RunAgentTurnError::Other {
            phase,
            message: case["message"].as_str().unwrap_or("x").to_string(),
        },
        "step_retry" => RunAgentTurnError::StepRetryExhausted {
            phase,
            message: case["message"].as_str().unwrap_or("step").to_string(),
        },
        "replan" => RunAgentTurnError::ReplanExhausted {
            phase,
            message: case["message"].as_str().unwrap_or("replan").to_string(),
        },
        "time_limit" => RunAgentTurnError::TimeLimitExhausted {
            phase,
            message: case["message"].as_str().unwrap_or("time").to_string(),
        },
        "token_limit" => RunAgentTurnError::TokenLimitExhausted {
            phase,
            message: case["message"].as_str().unwrap_or("token").to_string(),
        },
        other => panic!("unknown turn variant {other}"),
    }
}

fn assert_turn_case(row: &GoldenLine, ctx: &str) {
    let err = build_turn_error(&row.case);
    let expect = &row.expect;
    let public_code = expect["public_code"].as_str().expect("public_code");
    let http_status = expect["http_status"].as_u64().expect("http_status") as u16;
    let sse_code = expect["sse_code"].as_str().expect("sse_code");
    let http_has_reason = expect["http_has_reason_code"].as_bool().unwrap_or(false);
    let sse_has_reason = expect["sse_has_reason_code"].as_bool().unwrap_or(false);
    let job_outcome = expect["job_outcome"].as_str().expect("job_outcome");

    assert_eq!(err.public_error_code(), public_code, "{ctx}: public_code");
    assert_eq!(
        err.suggested_http_status(),
        StatusCode::from_u16(http_status).expect("status"),
        "{ctx}: http_status"
    );

    let turn_id = row.case.get("turn_id").and_then(|v| v.as_u64());
    let sse = err.sse_error_payload(turn_id);
    assert_eq!(sse.code.as_deref(), Some(sse_code), "{ctx}: sse_code");
    assert_eq!(
        sse.reason_code.is_some(),
        sse_has_reason,
        "{ctx}: sse_has_reason_code (got {:?})",
        sse.reason_code
    );
    if let Some(tid) = turn_id {
        assert_eq!(sse.turn_id, Some(tid), "{ctx}: turn_id");
    }
    assert_eq!(
        sse.sub_phase.as_deref(),
        Some(err.sub_phase().as_str()),
        "{ctx}: sub_phase"
    );

    let api = err.http_api_error();
    assert_eq!(api.code, public_code, "{ctx}: api.code");
    assert_eq!(
        api.reason_code.is_some(),
        http_has_reason,
        "{ctx}: http_has_reason_code (got {:?})",
        api.reason_code
    );

    let kind = err.job_queue_json_outcome_kind();
    let expected_kind = match job_outcome {
        "user_cancelled" => AgentTurnJobOutcomeKind::UserCancelled,
        "failure_emit_sse" => AgentTurnJobOutcomeKind::FailureEmitSseError,
        other => panic!("{ctx}: unknown job_outcome {other}"),
    };
    assert_eq!(kind, expected_kind, "{ctx}: job_outcome");

    if let Some(sub) = expect
        .get("public_message_contains")
        .and_then(|v| v.as_str())
    {
        assert!(
            err.public_user_message().contains(sub),
            "{ctx}: public_message_contains `{sub}` in {}",
            err.public_user_message()
        );
    }
}

fn assert_http_api_constant(row: &GoldenLine, ctx: &str) {
    use crabmate_api_contract::error_codes;
    let name = row.case["code_const"].as_str().expect("code_const");
    let expect_code = row.expect["code"].as_str().expect("code");
    let expect_status = row.expect["http_status"].as_u64().expect("http_status") as u16;
    let actual = match name {
        "QUEUE_FULL" => error_codes::QUEUE_FULL,
        "STREAM_JOB_GONE" => error_codes::STREAM_JOB_GONE,
        "UNAUTHORIZED" => error_codes::UNAUTHORIZED,
        "LLM_API_KEY_REQUIRED" => error_codes::LLM_API_KEY_REQUIRED,
        "CONVERSATION_CONFLICT" => error_codes::CONVERSATION_CONFLICT,
        other => panic!("{ctx}: unknown code_const {other}"),
    };
    assert_eq!(actual, expect_code, "{ctx}: constant value");
    // 文档约定状态码（handler 字面量与契约表）；常量侧仅锁码字符串。
    let _ = expect_status;
    assert!(
        matches!(
            expect_status,
            401 | 400 | 409 | 410 | 503 | 422 | 429 | 499 | 500 | 502
        ),
        "{ctx}: documented http_status {expect_status}"
    );
}

#[test]
fn golden_http_sse_failure_path_turn_and_constants() {
    let path = repo_root().join("fixtures/http_sse_failure_path_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut seen_turn = 0usize;
    let mut seen_const = 0usize;
    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = format!("{}:{} ({})", path.display(), line_no + 1, row.id);
        match row.kind.as_str() {
            "turn" => {
                assert_turn_case(&row, &ctx);
                seen_turn += 1;
            }
            "http_api_constant" => {
                assert_http_api_constant(&row, &ctx);
                seen_const += 1;
            }
            "sse_protocol" => {
                // 由 `web/chat_handlers` 侧金样消费。
            }
            other => panic!("{ctx}: unknown kind {other}"),
        }
    }
    assert!(seen_turn >= 10, "expected turn cases, got {seen_turn}");
    assert!(
        seen_const >= 3,
        "expected http_api_constant cases, got {seen_const}"
    );
}
