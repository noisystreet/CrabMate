//! `[[scheduled_agent_task]]` → 运行态 [`ScheduledAgentTask`]（`serve` 内 `tokio-cron-scheduler` 使用）。

use std::collections::HashSet;

use super::source::ScheduledAgentTaskRow;
use super::types::ScheduledAgentTask;

const CONVERSATION_ID_MAX_LEN: usize = 128;

fn validate_conversation_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("conversation_id 不能为空".to_string());
    }
    if id.len() > CONVERSATION_ID_MAX_LEN {
        return Err(format!(
            "conversation_id 过长（最多 {CONVERSATION_ID_MAX_LEN} 个字符）"
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("conversation_id 仅允许字母、数字、- _ . :".to_string());
    }
    Ok(())
}

fn validate_scheduled_task_conversation_policy(
    row: &ScheduledAgentTaskRow,
    id: &str,
) -> Result<(), String> {
    if row.new_conversation && row.conversation_id.is_some() {
        return Err(format!(
            "scheduled_agent_task id={id}：不能同时设置 new_conversation 与 conversation_id"
        ));
    }
    if !row.new_conversation {
        let cid = row
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                format!(
                    "scheduled_agent_task id={id}：未设置 new_conversation 时须提供非空 conversation_id"
                )
            })?;
        validate_conversation_id(cid)?;
    }
    Ok(())
}

fn validate_scheduled_task_agent_role(
    row: &ScheduledAgentTaskRow,
    id: &str,
    agent_roles: &std::collections::HashMap<String, super::agent_role_spec::AgentRoleSpec>,
) -> Result<(), String> {
    if let Some(ref role) = row.agent_role {
        let t = role.trim();
        if !t.is_empty() && !agent_roles.contains_key(t) {
            return Err(format!(
                "scheduled_agent_task id={id}：未知的 agent_role {t}（请在角色表中定义）"
            ));
        }
    }
    Ok(())
}

fn ensure_schedule_cron_parses(schedule: &str, id: &str) -> Result<(), String> {
    // 与库内 croner 解析一致：非法 cron 在启动加 job 时也会失败，此处先失败可提前提示用户。
    let _job =
        tokio_cron_scheduler::Job::new_async(schedule, |_uuid, _lock| Box::pin(async move {}))
            .map_err(|e| {
                format!("scheduled_agent_task id={id}：无效 schedule {schedule:?}（{e}）")
            })?;
    drop(_job);
    Ok(())
}

fn scheduled_task_row_into_runtime(
    row: ScheduledAgentTaskRow,
    id: String,
    schedule: String,
    message: String,
) -> ScheduledAgentTask {
    ScheduledAgentTask {
        id,
        schedule,
        message,
        conversation_id: row
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        new_conversation: row.new_conversation,
        agent_role: row
            .agent_role
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

/// 将 TOML 行合并、校验为 `AgentConfig` 字段；**须**在 `agent_roles` 定稿后调用。
pub(super) fn finalize_scheduled_agent_tasks(
    rows: Vec<ScheduledAgentTaskRow>,
    agent_roles: &std::collections::HashMap<String, super::agent_role_spec::AgentRoleSpec>,
) -> Result<Vec<ScheduledAgentTask>, String> {
    let mut seen_ids = HashSet::<String>::new();
    let mut out = Vec::new();
    for row in rows {
        if !row.enabled {
            continue;
        }
        let id = row.id.trim().to_string();
        if id.is_empty() {
            return Err("scheduled_agent_task：id 不能为空".to_string());
        }
        if !seen_ids.insert(id.clone()) {
            return Err(format!("scheduled_agent_task：重复的 id {id}"));
        }
        let schedule = row.schedule.trim().to_string();
        if schedule.is_empty() {
            return Err(format!("scheduled_agent_task id={id}：schedule 不能为空"));
        }
        let message = row.message.trim().to_string();
        if message.is_empty() {
            return Err(format!("scheduled_agent_task id={id}：message 不能为空"));
        }
        validate_scheduled_task_conversation_policy(&row, &id)?;
        validate_scheduled_task_agent_role(&row, &id, agent_roles)?;
        ensure_schedule_cron_parses(schedule.as_str(), &id)?;
        out.push(scheduled_task_row_into_runtime(row, id, schedule, message));
    }
    Ok(out)
}
