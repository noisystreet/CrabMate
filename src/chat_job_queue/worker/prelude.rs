//! 队列任务的**回合前置**：JSON 与 SSE 两条 driver 共用的同一份取数与解析（#1380 PR-1）。
//!
//! 这里只做「取数与解析」，不碰任何输出接线。sink 侧的差异——SSE 帧、审批会话、`cancel`、
//! `llm_backend` 注入、oneshot 回执——分别留在两个 driver 里。
//!
//! 抽这一层的理由：这些解析此前被逐行复制两份，任何「回合前置」改动都要改两遍，
//! 而 `json_job.rs` ↔ `stream_job_setup.rs` 是队列子系统里骨架重复度最高的文件对。

use std::collections::HashSet;
use std::sync::Arc;

use crate::agent_role_turn::{filter_tools_for_agent_role, turn_allow_for_web_or_cli_job};
use crate::cm_agent::permission::TurnPermissionStages;
use crate::config::AgentConfig;
use crate::types::{SessionMode, Tool};

use super::super::{
    PerTurnFlight, WebChatJobEnvelope, WebChatLlmOverride, WebChatQueueDeps,
    resolve_executor_llm_for_job, resolve_web_llm_for_job,
};
use super::{resolve_job_permission_persisted, resolve_job_permission_requested};

/// [`prepare_queued_turn`] 的入参（结构体入参以守住形参棘轮）。
pub(super) struct PreparedTurnParams<'a> {
    pub(super) envelope: &'a WebChatJobEnvelope,
    pub(super) queue_deps: &'a WebChatQueueDeps,
}

/// 一个排队回合在**执行之前**就已确定的全部解析结果。
///
/// 两条 driver 都从这里取 `TurnRequest` 的入参；除输出 sink 之外不再各自解析。
pub(super) struct PreparedQueuedTurn {
    /// 本回合的飞行状态。**持有这个 `Arc` 即在册**（见 `ChatJobQueue` 的 `active_per_flights`）：
    /// driver 必须让它活到收尾结束，否则 `GET /status` 的 `per_active_jobs` 会提前失去该任务。
    pub(super) flight: Arc<PerTurnFlight>,
    /// 服务端配置快照（未叠加 `client_llm` 覆盖）。收尾落盘用这一份。
    pub(super) cfg_snap: Arc<AgentConfig>,
    /// 叠加 `client_llm` 覆盖与实际 `api_key` 后的回合配置。
    pub(super) cfg_turn: Arc<AgentConfig>,
    pub(super) api_key_turn: String,
    /// 角色过滤后的工具表；与 `turn_allow` 同源，一起进出。
    pub(super) tools_for_job: Arc<Vec<Tool>>,
    pub(super) turn_allow: Option<Arc<HashSet<String>>>,
    /// 权限 `min` 链的四档输入（设计 §6.1）。
    pub(super) permission: TurnPermissionStages,
    pub(super) executor_api_base: Option<String>,
    pub(super) executor_api_key: Option<String>,
    pub(super) executor_model_override: Option<String>,
    /// 客户端 `client_llm.model`：单独传递，不写进 `cfg.llm.model`（避免影响 L2 分类器等内部组件）。
    pub(super) client_model_override: Option<String>,
}

/// 解析一个排队回合的前置阶段，并登记它的 PER 观测镜像。
pub(super) async fn prepare_queued_turn(p: PreparedTurnParams<'_>) -> PreparedQueuedTurn {
    let PreparedTurnParams { envelope, queue_deps } = p;

    // 在册 ⟺ 两个 driver 之一仍持有 `flight` 的强引用（见 `ChatJobQueue` 的
    // `active_per_flights`）。先建先登记、再做解析：否则「回合已开始」与「出现在
    // `GET /status`」之间会夹一段等配置锁的窗口。
    let flight = Arc::new(PerTurnFlight::default());
    queue_deps
        .chat_queue
        .attach_per_flight(envelope.job_id, &flight);

    let cfg_snap = {
        let g = queue_deps.cfg.read().await;
        Arc::new(g.clone())
    };
    let (mut cfg_turn, api_key_turn) =
        resolve_web_llm_for_job(queue_deps, cfg_snap.clone(), envelope.llm_override.as_ref());
    if let Some(secs) = envelope.readonly_tool_ttl_cache_secs {
        let mut c = (*cfg_turn).clone();
        c.chat_queues_cache.readonly_tool_ttl_cache_secs = secs;
        cfg_turn = Arc::new(c);
    }

    let turn_allow = turn_allow_for_web_or_cli_job(
        &cfg_turn,
        envelope.persisted_active_agent_role.as_deref(),
        envelope.request_agent_role.as_deref(),
    );
    let tools_for_job = Arc::new(filter_tools_for_agent_role(
        &queue_deps.tools,
        turn_allow.as_ref().map(|a| a.as_ref()),
    ));

    // 角色级权限上限：与工具白名单同一角色解析口径（设计 §6.1 的 min 链）。
    let permission = TurnPermissionStages {
        role_max:
            crate::context::agent_role_turn::turn_role_max_permission_level_for_web_or_cli_job(
                &cfg_turn,
                envelope.persisted_active_agent_role.as_deref(),
                envelope.request_agent_role.as_deref(),
            ),
        session_mode: resolve_turn_session_mode(
            cfg_snap.as_ref(),
            envelope.request_session_mode.as_deref(),
            envelope.persisted_active_session_mode.as_deref(),
            envelope.request_agent_role.as_deref(),
            envelope.persisted_active_agent_role.as_deref(),
        ),
        requested: resolve_job_permission_requested(envelope.request_permission_level.as_deref()),
        persisted: resolve_job_permission_persisted(
            envelope.persisted_active_permission_level.as_deref(),
        ),
    };

    let (executor_api_base, executor_api_key, executor_model_override) =
        resolve_executor_llm_triple(queue_deps, &cfg_turn, envelope.executor_llm_override.as_ref());
    let client_model_override = envelope.llm_override.as_ref().and_then(|o| o.model.clone());

    PreparedQueuedTurn {
        flight,
        cfg_snap,
        cfg_turn,
        api_key_turn,
        tools_for_job,
        turn_allow,
        permission,
        executor_api_base,
        executor_api_key,
        executor_model_override,
        client_model_override,
    }
}

/// 本回合会话模式：请求 > 会话持久值 > 角色默认 > 配置默认；坏串回落 `act` 并留一条 warn。
///
/// 回落而不是报错：`POST /chat` 已在解析请求体时返回 400，这里只防御历史库脏数据与内部入口——
/// 一行坏串不该让整个回合 500（与 `permission_level` 解析同取向）。
fn resolve_turn_session_mode(
    cfg: &AgentConfig,
    request: Option<&str>,
    persisted: Option<&str>,
    request_agent_role: Option<&str>,
    persisted_agent_role: Option<&str>,
) -> SessionMode {
    let role_default = crate::session_mode_turn::role_default_session_mode_for_turn(
        cfg,
        persisted_agent_role,
        request_agent_role,
    );
    match crate::session_mode_turn::resolve_session_mode_for_turn(
        request,
        persisted,
        role_default,
        cfg.roles_prompts.default_session_mode,
    ) {
        Ok(m) => m,
        Err(e) => {
            log::warn!(target: "crabmate", "session_mode resolve failed: {e}; using act");
            SessionMode::Act
        }
    }
}

/// executor 阶段的 `(api_base, api_key, model)` 覆盖三元组。
///
/// `base` / `model` 只在**与主模型不同**时才下发，避免把相同值再抄一遍；`api_key` 恒随
/// override 下发（`resolve_executor_llm_for_job` 返回 `Some` 即代表确实存在 override）。
fn resolve_executor_llm_triple(
    queue_deps: &WebChatQueueDeps,
    cfg_turn: &Arc<AgentConfig>,
    executor_llm_override: Option<&WebChatLlmOverride>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some((executor_cfg, executor_key)) =
        resolve_executor_llm_for_job(queue_deps, Arc::clone(cfg_turn), executor_llm_override)
    else {
        return (None, None, None);
    };
    let base =
        (executor_cfg.llm.api_base != cfg_turn.llm.api_base).then(|| executor_cfg.llm.api_base.clone());
    let model =
        (executor_cfg.llm.model != cfg_turn.llm.model).then(|| executor_cfg.llm.model.clone());
    (base, Some(executor_key), model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AgentConfig {
        crate::cm_config::load_config(None).expect("嵌入默认配置应可加载")
    }

    /// 请求 > 会话持久值：两档都在时请求胜出，只有持久值时用持久值。
    ///
    /// 这两条断言都与配置内容无关（`request` 命中即返回），锁住的是搬移时最容易写反的
    /// 一处分支——它此前被复制在两个文件里，写反了不会有编译错误。
    #[test]
    fn session_mode_request_wins_over_persisted() {
        let c = cfg();
        assert_eq!(
            resolve_turn_session_mode(&c, Some("ask"), Some("plan"), None, None),
            SessionMode::Ask
        );
        assert_eq!(
            resolve_turn_session_mode(&c, None, Some("plan"), None, None),
            SessionMode::Plan
        );
    }

    /// 坏串一律回落 `act`（并留一条 warn），不让一行脏值把整个回合打成 500。
    #[test]
    fn session_mode_bad_value_falls_back_to_act() {
        let c = cfg();
        assert_eq!(
            resolve_turn_session_mode(&c, Some("sudo"), None, None, None),
            SessionMode::Act
        );
        assert_eq!(
            resolve_turn_session_mode(&c, None, Some("root"), None, None),
            SessionMode::Act
        );
    }
}
