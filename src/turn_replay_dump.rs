//! 每轮 [`crate::run_agent_turn`] 结束后，将**会话消息快照**与**排障用配置摘要**写入 JSON（供离线对照与工具重放，**不**含完整 LLM 请求体或密钥）。
//!
//! | 环境变量 | 说明 |
//! |----------|------|
//! | **`CRABMATE_TURN_REPLAY_DUMP_DIR`** | 非空目录时，每轮结束追加 **`turn-replay-{wall_ms}.json`**（与 `CRABMATE_REQUEST_CHROME_TRACE_DIR` 的 `turn-*.json` 不同名，可同目录并存）。 |
//! | **`AGENT_TURN_REPLAY_DUMP_DIR`** | 同上别名。优先读取 **`CRABMATE_*`**。 |
//!
//! 正文字段经 [`crate::redact::redact_secrets_in_json_str`] 做启发式脱敏；**仍可能含长段用户/助手正文**——仅用于**可信**本机调试，勿共享或进版本库。

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::config::AgentConfig;
use crate::redact;
use crate::types::{LlmSeedOverride, Message, Tool, resolved_llm_seed};

const DUMP_VERSION: u32 = 1;
const FILE_PREFIX: &str = "turn-replay-";

/// 与 Chrome trace 一致：从环境读取目录，空则关闭。
fn turn_replay_dir_from_env() -> Option<std::path::PathBuf> {
    let a = std::env::var_os("CRABMATE_TURN_REPLAY_DUMP_DIR");
    let b = std::env::var_os("AGENT_TURN_REPLAY_DUMP_DIR");
    let s = a.or(b)?;
    let t = s.to_string_lossy();
    let trimmed = t.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trimmed))
    }
}

#[derive(Debug, Serialize)]
struct ConfigSnapshotV1 {
    version: u32,
    api_base: String,
    model: String,
    planner_model: Option<String>,
    executor_model: Option<String>,
    llm_http_auth_mode: String,
    temperature: f32,
    max_tokens: u32,
    max_message_history: usize,
    no_stream: bool,
    render_to_terminal: bool,
    plain_terminal_stream: bool,
    planner_executor_mode: String,
    staged_plan_execution: bool,
    api_timeout_secs: u64,
    run_command_working_dir: String,
    effective_working_dir: String,
    workspace_is_set: bool,
}

fn build_config_snapshot(
    cfg: &AgentConfig,
    no_stream: bool,
    render_to_terminal: bool,
    plain_terminal_stream: bool,
    effective_working_dir: &Path,
    workspace_is_set: bool,
) -> ConfigSnapshotV1 {
    ConfigSnapshotV1 {
        version: 1,
        api_base: cfg.api_base.clone(),
        model: cfg.model.clone(),
        planner_model: cfg.planner_model.clone(),
        executor_model: cfg.executor_model.clone(),
        llm_http_auth_mode: cfg.llm_http_auth_mode.as_str().to_string(),
        temperature: cfg.temperature,
        max_tokens: cfg.max_tokens,
        max_message_history: cfg.max_message_history,
        no_stream,
        render_to_terminal,
        plain_terminal_stream,
        planner_executor_mode: cfg.planner_executor_mode.as_str().to_string(),
        staged_plan_execution: cfg.staged_plan_execution,
        api_timeout_secs: cfg.api_timeout_secs,
        run_command_working_dir: cfg.run_command_working_dir.clone(),
        effective_working_dir: effective_working_dir.display().to_string(),
        workspace_is_set,
    }
}

#[derive(Debug, Serialize)]
struct TurnReplayDumpV1 {
    version: u32,
    /// 与 Chrome **`turn-*.json`** 文件名中的 **`wall_ms`** 对齐（Unix 毫秒）。
    wall_start_ms: u64,
    conversation_scope_id: Option<String>,
    job_id: Option<u64>,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    temperature_override: Option<f32>,
    model_override: Option<String>,
    use_executor_model: bool,
    executor_model_override: Option<String>,
    llm_seed_effective: Option<i64>,
    tool_names: Vec<String>,
    /// 脱敏后 JSON；键名经 serde，正文类值经 [`redact::redact_secrets_in_json_str`]。
    messages: serde_json::Value,
    config: ConfigSnapshotV1,
}

/// `run_agent_turn` 无论成功/失败在返回前调用：写入**回合结束**时的消息列表与配置（失败时追加短错误说明）。
// 与 `run_agent_turn` 入参一一对应，拆结构体无收益。
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_turn_replay_dump_if_configured(
    wall_ms: u64,
    long_term_memory_scope_id: Option<&str>,
    tracing_job_id: Option<u64>,
    result: &Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    messages: &[Message],
    tools: &[Tool],
    cfg: &AgentConfig,
    no_stream: bool,
    render_to_terminal: bool,
    plain_terminal_stream: bool,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    temperature_override: Option<f32>,
    model_override: Option<String>,
    use_executor_model: bool,
    executor_model_override: Option<String>,
    seed_override: LlmSeedOverride,
) {
    let Some(dir) = turn_replay_dir_from_env() else {
        return;
    };
    let (ok, err_opt) = match result {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{e}"))),
    };
    let config_snap = build_config_snapshot(
        cfg,
        no_stream,
        render_to_terminal,
        plain_terminal_stream,
        effective_working_dir,
        workspace_is_set,
    );
    let tool_names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();
    let msg_json = match serde_json::to_value(messages) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "turn replay dump: serialize messages failed: {e}"
            );
            return;
        }
    };
    let msg_str = match serde_json::to_string(&msg_json) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "turn replay dump: messages to string failed: {e}"
            );
            return;
        }
    };
    let msg_redacted = redact::redact_secrets_in_json_str(&msg_str);
    let messages_out: serde_json::Value = serde_json::from_str(&msg_redacted).unwrap_or(msg_json);
    let dump = TurnReplayDumpV1 {
        version: DUMP_VERSION,
        wall_start_ms: wall_ms,
        conversation_scope_id: long_term_memory_scope_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        job_id: tracing_job_id,
        ok,
        error: err_opt,
        temperature_override,
        model_override,
        use_executor_model,
        executor_model_override,
        llm_seed_effective: resolved_llm_seed(cfg.llm_seed, seed_override),
        tool_names,
        messages: messages_out,
        config: config_snap,
    };
    let pretty = match serde_json::to_string_pretty(&dump) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "turn replay dump: to_string_pretty failed: {e}"
            );
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(
            target: "crabmate",
            "turn replay dump: create_dir_all {:?} failed: {e}",
            dir
        );
        return;
    }
    let path = dir.join(format!("{FILE_PREFIX}{wall_ms}.json"));
    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(pretty.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        log::warn!(
            target: "crabmate",
            "turn replay dump: write {} failed: {e}",
            path.display()
        );
    } else {
        log::info!(
            target: "crabmate",
            "turn replay dump: wrote {}",
            path.display()
        );
    }
}
