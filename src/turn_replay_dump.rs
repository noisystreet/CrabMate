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
/// 回合 replay JSONL 行模式版本（与 `replay_schema_version` 字段一致）。
const REPLAY_SCHEMA_VERSION: u32 = 1;
static TURN_REPLAY_DUMP_DIR_LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static TURN_REPLAY_EVENT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static TURN_REPLAY_DECISION_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
/// 每次 `set_turn_replay_event_context`（通常每轮 `run_agent_turn`）单调 +1，写入同轮所有事件行。
static TURN_REPLAY_TURN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TURN_REPLAY_EVENT_CONTEXT: std::sync::LazyLock<
    std::sync::RwLock<Option<TurnReplayEventContext>>,
> = std::sync::LazyLock::new(|| std::sync::RwLock::new(None));

#[derive(Debug, Clone)]
struct TurnReplayEventContext {
    wall_start_ms: u64,
    conversation_scope_id: Option<String>,
    job_id: Option<u64>,
    replay_turn_seq: u64,
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
    let replay_turn_seq =
        TURN_REPLAY_TURN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if let Ok(mut guard) = TURN_REPLAY_EVENT_CONTEXT.write() {
        *guard = Some(TurnReplayEventContext {
            wall_start_ms,
            conversation_scope_id: conversation_scope_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            job_id,
            replay_turn_seq,
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
    if let serde_json::Value::Object(map) = &mut payload {
        map.insert(
            "replay_schema_version".to_string(),
            serde_json::Value::Number(REPLAY_SCHEMA_VERSION.into()),
        );
        let ctx = TURN_REPLAY_EVENT_CONTEXT
            .read()
            .ok()
            .and_then(|g| g.as_ref().cloned());
        map.insert(
            "replay_turn_seq".to_string(),
            match ctx.as_ref().map(|c| c.replay_turn_seq) {
                Some(v) => serde_json::Value::Number(v.into()),
                None => serde_json::Value::Null,
            },
        );
        if let Some(ctx) = ctx.as_ref() {
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
        } else {
            map.insert("wall_start_ms".to_string(), serde_json::Value::Null);
            map.insert("job_id".to_string(), serde_json::Value::Null);
            map.insert("conversation_scope_id".to_string(), serde_json::Value::Null);
        }
    }
    if let Some(d) = detail
        && let serde_json::Value::Object(map) = &mut payload
    {
        map.insert("detail".to_string(), d.clone());
    }

    let mut line = redact::redact_secrets_in_json_str(&payload.to_string());
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
///
/// 其余字段保留在结构体上，便于调用方不必删参数即可迁移到「单结构体」调用。
pub(crate) struct TurnReplayDumpParams<'a> {
    pub wall_ms: u64,
    pub long_term_memory_scope_id: Option<&'a str>,
    pub tracing_job_id: Option<u64>,
    pub result: &'a Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    pub messages: &'a [Message],
    pub tools: &'a [Tool],
    pub cfg: &'a AgentConfig,
    pub no_stream: bool,
    pub render_to_terminal: bool,
    pub plain_terminal_stream: bool,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub temperature_override: Option<f32>,
    pub model_override: Option<String>,
    pub use_executor_model: bool,
    pub executor_model_override: Option<String>,
    pub seed_override: LlmSeedOverride,
}

pub(crate) fn write_turn_replay_dump_if_configured(p: TurnReplayDumpParams<'_>) {
    let TurnReplayDumpParams {
        wall_ms,
        long_term_memory_scope_id,
        tracing_job_id,
        result,
        messages: _messages,
        tools: _tools,
        cfg: _cfg,
        no_stream: _no_stream,
        render_to_terminal: _render_to_terminal,
        plain_terminal_stream: _plain_terminal_stream,
        effective_working_dir: _effective_working_dir,
        workspace_is_set: _workspace_is_set,
        temperature_override: _temperature_override,
        model_override: _model_override,
        use_executor_model: _use_executor_model,
        executor_model_override: _executor_model_override,
        seed_override: _seed_override,
    } = p;
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

/// 仅 **`cargo test`**：重置回合 replay 的进程级全局状态，避免测试间互相污染。
#[cfg(test)]
pub(crate) fn reset_turn_replay_globals_for_tests() {
    TURN_REPLAY_EVENT_SEQ.store(1, std::sync::atomic::Ordering::Relaxed);
    TURN_REPLAY_DECISION_SEQ.store(1, std::sync::atomic::Ordering::Relaxed);
    TURN_REPLAY_TURN_SEQ.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut guard) = TURN_REPLAY_EVENT_CONTEXT.write() {
        *guard = None;
    }
}

#[cfg(test)]
mod turn_replay_line_tests {
    use super::*;
    use std::sync::Mutex;

    static REPLAY_TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn jsonl_line_includes_schema_turn_seq_and_redacts() {
        let _lock = REPLAY_TEST_ENV_LOCK.lock().unwrap();
        reset_turn_replay_globals_for_tests();
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: `REPLAY_TEST_ENV_LOCK` serializes these tests; no other thread reads this env var concurrently.
        unsafe {
            std::env::set_var("CM_REPLAY_DUMP_DIR", dir.path().as_os_str());
        }
        set_turn_replay_event_context(9_001, Some("scope-a"), Some(42));
        append_turn_replay_event_json_if_configured(
            "test_event",
            "fixture",
            Some(&serde_json::json!({
                "note": "Authorization: Bearer sk-test-replace-me"
            })),
        );
        let path = dir.path().join(EVENT_STREAM_FILE);
        let raw = std::fs::read_to_string(&path).expect("read jsonl");
        unsafe {
            std::env::remove_var("CM_REPLAY_DUMP_DIR");
        }
        assert!(!raw.contains("sk-test"), "line should be redacted: {raw}");
        let v: serde_json::Value =
            serde_json::from_str(raw.lines().next().expect("one line")).expect("json");
        assert_eq!(
            v.get("replay_schema_version").and_then(|x| x.as_u64()),
            Some(1)
        );
        assert_eq!(v.get("replay_turn_seq").and_then(|x| x.as_u64()), Some(1));
        assert_eq!(v.get("event").and_then(|x| x.as_str()), Some("test_event"));
        assert_eq!(v.get("title").and_then(|x| x.as_str()), Some("fixture"));
        assert_eq!(v.get("wall_start_ms").and_then(|x| x.as_u64()), Some(9_001));
    }

    #[test]
    fn turn_seq_increments_per_set_context() {
        let _lock = REPLAY_TEST_ENV_LOCK.lock().unwrap();
        reset_turn_replay_globals_for_tests();
        let dir = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var("CM_REPLAY_DUMP_DIR", dir.path().as_os_str());
        }
        set_turn_replay_event_context(1, None, None);
        append_turn_replay_event_json_if_configured("a", "t1", None);
        set_turn_replay_event_context(2, None, None);
        append_turn_replay_event_json_if_configured("b", "t2", None);
        let raw = std::fs::read_to_string(dir.path().join(EVENT_STREAM_FILE)).expect("read");
        unsafe {
            std::env::remove_var("CM_REPLAY_DUMP_DIR");
        }
        let mut lines = raw.lines();
        let v1: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let v2: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(v1.get("replay_turn_seq").and_then(|x| x.as_u64()), Some(1));
        assert_eq!(v2.get("replay_turn_seq").and_then(|x| x.as_u64()), Some(2));
    }
}
