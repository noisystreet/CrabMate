//! 回合级 replay 事件流（JSONL）：
//! - 当设置 `CM_REPLAY_DUMP_DIR` 时，
//!   将关键动作追加写入 `turn-replay-events.jsonl`。
//! - 不再生成 `turn-replay-{wall_ms}.json` 快照文件。

use std::io::Write;

use crate::config::AgentConfig;
use crate::redact;
use crate::types::{
    LlmSeedOverride, Message, Tool, message_content_as_str,
    user_message_counts_for_branch_truncation,
};

const EVENT_STREAM_FILE: &str = "turn-replay-events.jsonl";
static TURN_REPLAY_DUMP_DIR_LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static TURN_REPLAY_EVENT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static TURN_REPLAY_DECISION_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static TURN_REPLAY_EVENT_CONTEXT: std::sync::LazyLock<
    std::sync::RwLock<Option<TurnReplayEventContext>>,
> = std::sync::LazyLock::new(|| std::sync::RwLock::new(None));

#[derive(Debug, Clone)]
struct TurnReplayEventContext {
    wall_start_ms: u64,
    conversation_scope_id: Option<String>,
    job_id: Option<u64>,
}

fn turn_replay_dir_from_env() -> Option<std::path::PathBuf> {
    let s = std::env::var_os("CM_REPLAY_DUMP_DIR")?;
    let t = s.to_string_lossy();
    let trimmed = t.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trimmed))
    }
}

fn now_unix_ms() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as u64,
        Err(_) => 0,
    }
}

pub(crate) fn set_turn_replay_event_context(
    wall_start_ms: u64,
    conversation_scope_id: Option<&str>,
    job_id: Option<u64>,
) {
    if let Ok(mut guard) = TURN_REPLAY_EVENT_CONTEXT.write() {
        *guard = Some(TurnReplayEventContext {
            wall_start_ms,
            conversation_scope_id: conversation_scope_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            job_id,
        });
    }
}

pub(crate) fn clear_turn_replay_event_context() {
    if let Ok(mut guard) = TURN_REPLAY_EVENT_CONTEXT.write() {
        *guard = None;
    }
}

fn build_latest_user_input_detail(messages: &[Message]) -> Option<serde_json::Value> {
    let (idx, m) = messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| user_message_counts_for_branch_truncation(m))?;
    let text = message_content_as_str(&m.content)
        .unwrap_or_default()
        .to_string();
    let text_truncated = text.chars().count() > 1200;
    let text_preview = redact::single_line_preview(&text, 1200);
    let content_json = serde_json::to_value(&m.content).unwrap_or(serde_json::Value::Null);
    let detail = serde_json::json!({
        "phase": "turn_input",
        "message_index": idx,
        "user_name": m.name,
        "user_text": text,
        "user_text_preview": text_preview,
        "user_text_truncated": text_truncated,
        "user_content": content_json
    });
    let detail_s = serde_json::to_string(&detail).ok()?;
    serde_json::from_str(&redact::redact_secrets_in_json_str(&detail_s)).ok()
}

pub(crate) fn append_latest_user_input_event_if_configured(messages: &[Message]) {
    if let Some(detail) = build_latest_user_input_detail(messages) {
        append_turn_replay_event_json_if_configured(
            "turn_user_input",
            "latest_user_message",
            Some(&detail),
        );
    }
}

pub(crate) fn append_turn_replay_event_if_configured(
    event: &str,
    title: &str,
    detail: Option<&str>,
) {
    let detail_json = detail.map(|d| serde_json::json!({ "text": d, "phase": "general" }));
    append_turn_replay_event_json_if_configured(event, title, detail_json.as_ref());
}

pub(crate) fn append_turn_replay_event_json_if_configured(
    event: &str,
    title: &str,
    detail: Option<&serde_json::Value>,
) {
    let Some(dir) = turn_replay_dir_from_env() else {
        return;
    };
    if TURN_REPLAY_DUMP_DIR_LOGGED.get().is_none() {
        log::info!(
            target: "crabmate",
            "turn replay events enabled: {}",
            dir.display()
        );
        let _ = TURN_REPLAY_DUMP_DIR_LOGGED.set(());
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(
            target: "crabmate",
            "turn replay event: create_dir_all {:?} failed: {e}",
            dir
        );
        return;
    }

    let path = dir.join(EVENT_STREAM_FILE);
    let mut payload = serde_json::json!({
        "seq": TURN_REPLAY_EVENT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        "ts_ms": now_unix_ms(),
        "event": event,
        "title": title,
    });
    if let Ok(guard) = TURN_REPLAY_EVENT_CONTEXT.read()
        && let Some(ctx) = guard.as_ref()
        && let serde_json::Value::Object(map) = &mut payload
    {
        map.insert(
            "wall_start_ms".to_string(),
            serde_json::Value::Number(ctx.wall_start_ms.into()),
        );
        map.insert(
            "job_id".to_string(),
            match ctx.job_id {
                Some(v) => serde_json::Value::Number(v.into()),
                None => serde_json::Value::Null,
            },
        );
        map.insert(
            "conversation_scope_id".to_string(),
            match ctx.conversation_scope_id.clone() {
                Some(v) => serde_json::Value::String(v),
                None => serde_json::Value::Null,
            },
        );
    }
    if let Some(d) = detail
        && let serde_json::Value::Object(map) = &mut payload
    {
        map.insert("detail".to_string(), d.clone());
    }

    let mut line = payload.to_string();
    line.push('\n');
    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(line.as_bytes())?;
        f.flush()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        log::warn!(
            target: "crabmate",
            "turn replay event: append {} failed: {e}",
            path.display()
        );
    }
}

pub(crate) fn append_decision_point_event_if_configured(
    phase: &str,
    decision_type: &str,
    chosen: &str,
    reason: &str,
    evidence: serde_json::Value,
    impact_scope: &str,
    related_ids: Option<serde_json::Value>,
) {
    let decision_id = format!(
        "dec-{}",
        TURN_REPLAY_DECISION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    append_turn_replay_event_json_if_configured(
        "decision_point",
        decision_type,
        Some(&serde_json::json!({
            "decision_id": decision_id,
            "phase": phase,
            "decision_type": decision_type,
            "chosen": chosen,
            "reason": reason,
            "evidence": evidence,
            "impact_scope": impact_scope,
            "related_ids": related_ids.unwrap_or(serde_json::Value::Null),
        })),
    );
}

/// 兼容旧调用点：不再生成 `turn-replay-*.json`，仅写一条回合摘要事件。
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_turn_replay_dump_if_configured(
    wall_ms: u64,
    long_term_memory_scope_id: Option<&str>,
    tracing_job_id: Option<u64>,
    result: &Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    _messages: &[Message],
    _tools: &[Tool],
    _cfg: &AgentConfig,
    _no_stream: bool,
    _render_to_terminal: bool,
    _plain_terminal_stream: bool,
    _effective_working_dir: &std::path::Path,
    _workspace_is_set: bool,
    _temperature_override: Option<f32>,
    _model_override: Option<String>,
    _use_executor_model: bool,
    _executor_model_override: Option<String>,
    _seed_override: LlmSeedOverride,
) {
    // 旧函数保持存在，避免改动调用链；仅追加事件，不再写 turn-replay-*.json。
    append_turn_replay_event_json_if_configured(
        "turn_snapshot_skipped",
        "json_snapshot_removed",
        Some(&serde_json::json!({
            "phase": "turn",
            "wall_start_ms": wall_ms,
            "job_id": tracing_job_id,
            "conversation_scope_id": long_term_memory_scope_id,
            "ok": result.is_ok(),
            "reason": "turn-replay-json-disabled",
        })),
    );
}
