//! 从进程环境装配 [`FeishuBridgeConfig`]，按关切点拆分子函数与中间结构体以降低单函数圈复杂度。

use std::env;
use std::sync::Arc;
use std::time::Duration;

use crabmate_im_bridge::{CrabmateClient, FeishuBridgeConfig, FeishuToolApprovalMode};

pub fn feishu_bridge_config_from_env() -> Result<FeishuBridgeConfig, String> {
    let ids = read_identity_bundle()?;
    let signing = read_signing_and_bot_env()?;
    let tools = read_tool_approval_env()?;
    let sqlite = read_sqlite_queue_env()?;
    let ui = read_runtime_and_ui_env()?;

    let crabmate = Arc::new(
        CrabmateClient::new(ids.crabmate_base, ids.crabmate_bearer).map_err(|e| e.to_string())?,
    );

    Ok(FeishuBridgeConfig {
        app_id: ids.app_id,
        app_secret: ids.app_secret,
        encrypt_key: signing.encrypt_key,
        verify_signature_when_possible: signing.verify_signature_when_possible,
        verification_token: signing.verification_token,
        replay_timestamp_max_skew_secs: signing.replay_timestamp_max_skew_secs,
        nonce_dedup_ttl: signing.nonce_dedup_ttl,
        group_require_bot_mention: signing.group_require_bot_mention,
        bot_open_id: signing.bot_open_id,
        crabmate,
        dedup_ttl: Duration::from_secs(600),
        max_message_content_json_chars: ui.max_message_content_json_chars,
        async_worker: ui.async_worker,
        event_queue_capacity: ui.event_queue_capacity,
        workspace_root_template: ui.workspace_root_template,
        tool_approval_mode: tools.mode,
        tool_decision_secret: tools.secret,
        tool_decision_timeout_secs: tools.timeout_secs,
        quiet_sse_status: ui.quiet_sse_status,
        result_card_max_body_chars: ui.result_card_max_body_chars,
        in_place_progress_card: ui.in_place_progress_card,
        event_queue_sqlite_path: sqlite.path,
        sqlite_queue_max_retries: sqlite.max_retries,
        sqlite_queue_poll_ms: sqlite.poll_ms,
        sqlite_queue_lease_secs: sqlite.lease_secs.max(30),
    })
}

struct IdentityBundle {
    crabmate_base: String,
    crabmate_bearer: String,
    app_id: String,
    app_secret: String,
}

fn read_identity_bundle() -> Result<IdentityBundle, String> {
    Ok(IdentityBundle {
        crabmate_base: env_req("CM_BASE_URL")?,
        crabmate_bearer: crabmate_bearer_from_env()?,
        app_id: env_req("FEISHU_APP_ID")?,
        app_secret: env_req("FEISHU_APP_SECRET")?,
    })
}

struct SigningAndBotEnv {
    encrypt_key: Option<String>,
    verification_token: Option<String>,
    verify_signature_when_possible: bool,
    replay_timestamp_max_skew_secs: i64,
    nonce_dedup_ttl: Duration,
    group_require_bot_mention: bool,
    bot_open_id: Option<String>,
}

fn read_signing_and_bot_env() -> Result<SigningAndBotEnv, String> {
    Ok(SigningAndBotEnv {
        encrypt_key: optional_trimmed_env("FEISHU_ENCRYPT_KEY"),
        verification_token: optional_trimmed_env("FEISHU_VERIFICATION_TOKEN"),
        verify_signature_when_possible: env_verify_signature_default_on(),
        replay_timestamp_max_skew_secs: env_u64("FEISHU_REPLAY_MAX_SKEW_SECS", 600)? as i64,
        nonce_dedup_ttl: Duration::from_secs(env_u64("FEISHU_NONCE_DEDUP_SECS", 900)?),
        group_require_bot_mention: env_group_require_bot_mention(),
        bot_open_id: optional_trimmed_env("FEISHU_BOT_OPEN_ID"),
    })
}

struct ToolApprovalEnv {
    mode: FeishuToolApprovalMode,
    secret: Option<String>,
    timeout_secs: u64,
}

fn read_tool_approval_env() -> Result<ToolApprovalEnv, String> {
    let secret = optional_trimmed_env("FEISHU_TOOL_DECISION_SECRET");
    let mode = parse_tool_approval_mode(
        env::var("FEISHU_TOOL_APPROVAL_MODE")
            .unwrap_or_else(|_| "wait_message".into())
            .as_str(),
    )?;
    validate_wait_http_has_secret(mode, secret.as_deref())?;
    let timeout_secs = env_u64("FEISHU_TOOL_DECISION_TIMEOUT_SECS", 600)?.max(5);
    Ok(ToolApprovalEnv {
        mode,
        secret,
        timeout_secs,
    })
}

struct SqliteQueueEnv {
    path: Option<String>,
    max_retries: u32,
    poll_ms: u64,
    lease_secs: i64,
}

fn read_sqlite_queue_env() -> Result<SqliteQueueEnv, String> {
    Ok(SqliteQueueEnv {
        path: optional_trimmed_env("FEISHU_EVENT_QUEUE_SQLITE"),
        max_retries: env_u64("FEISHU_SQLITE_QUEUE_MAX_RETRIES", 5)?.max(1) as u32,
        poll_ms: env_u64("FEISHU_SQLITE_QUEUE_POLL_MS", 200)?.max(50),
        lease_secs: env_u64("FEISHU_SQLITE_QUEUE_LEASE_SECS", 600)? as i64,
    })
}

struct RuntimeAndUiEnv {
    max_message_content_json_chars: usize,
    async_worker: bool,
    event_queue_capacity: usize,
    workspace_root_template: Option<String>,
    quiet_sse_status: bool,
    result_card_max_body_chars: usize,
    in_place_progress_card: bool,
}

fn read_runtime_and_ui_env() -> Result<RuntimeAndUiEnv, String> {
    Ok(RuntimeAndUiEnv {
        max_message_content_json_chars: env_u64("FEISHU_MAX_MESSAGE_JSON_CHARS", 12000)?.max(256)
            as usize,
        async_worker: env_bool("FEISHU_ASYNC_WORKER", true)?,
        event_queue_capacity: env_u64("FEISHU_EVENT_QUEUE_CAPACITY", 100)?.max(1) as usize,
        workspace_root_template: optional_trimmed_env("FEISHU_WORKSPACE_ROOT_TEMPLATE"),
        quiet_sse_status: env_bool("FEISHU_QUIET_SSE_STATUS", false)?,
        result_card_max_body_chars: env_u64("FEISHU_RESULT_CARD_MAX_CHARS", 3500)?.max(200)
            as usize,
        in_place_progress_card: env_bool("FEISHU_IN_PLACE_PROGRESS_CARD", false)?,
    })
}

fn crabmate_bearer_from_env() -> Result<String, String> {
    env::var("CM_WEB_API_BEARER_TOKEN")
        .or_else(|_| env::var("CM_WEB_API_BEARER"))
        .map_err(|_| {
            "缺少 CM_WEB_API_BEARER_TOKEN（或与 serve 相同的 Bearer；可选别名 CM_WEB_API_BEARER）"
                .to_string()
        })
}

fn optional_trimmed_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_verify_signature_default_on() -> bool {
    !matches!(
        env::var("FEISHU_VERIFY_SIGNATURE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    )
}

fn env_group_require_bot_mention() -> bool {
    matches!(
        env::var("FEISHU_GROUP_REQUIRE_BOT_MENTION").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn validate_wait_http_has_secret(
    mode: FeishuToolApprovalMode,
    secret: Option<&str>,
) -> Result<(), String> {
    if mode == FeishuToolApprovalMode::WaitHttp && secret.is_none() {
        return Err(
            "FEISHU_TOOL_APPROVAL_MODE=wait_http requires FEISHU_TOOL_DECISION_SECRET".to_string(),
        );
    }
    Ok(())
}

fn env_req(name: &str) -> Result<String, String> {
    let s = env::var(name).map_err(|_| format!("missing environment variable {name}"))?;
    let t = s.trim().to_string();
    if t.is_empty() {
        return Err(format!("environment variable {name} is empty"));
    }
    Ok(t)
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env::var(name) {
        Err(_) => Ok(default),
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(default)
            } else {
                t.parse::<u64>()
                    .map_err(|_| format!("invalid unsigned integer for {name}: {s}"))
            }
        }
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Err(_) => Ok(default),
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            if t.is_empty() {
                return Ok(default);
            }
            match t.as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" => Ok(false),
                _ => Err(format!("invalid boolean for {name}: {s}")),
            }
        }
    }
}

fn parse_tool_approval_mode(raw: &str) -> Result<FeishuToolApprovalMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "wait_message" => Ok(FeishuToolApprovalMode::WaitMessage),
        "deny_all" | "deny" => Ok(FeishuToolApprovalMode::DenyAll),
        "default_allow_once" | "auto_allow_once" | "allow_once_auto" => {
            Ok(FeishuToolApprovalMode::DefaultAllowOnce)
        }
        "wait_http" | "http" => Ok(FeishuToolApprovalMode::WaitHttp),
        other => Err(format!(
            "invalid FEISHU_TOOL_APPROVAL_MODE: {other} (deny_all | default_allow_once | wait_http | wait_message)"
        )),
    }
}
