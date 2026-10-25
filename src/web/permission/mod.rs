//! `GET /permission`：权限等级与六轴投影的**只读**出口
//! （设计 `docs/design/permission_levels.md` §16.2）。
//!
//! 与 `GET /status` 的分工：`/status` 报运行态（模型、工具数…），本端点报**策略** ——
//! 客户端拿 `available` 直接渲染等级选择器，拿 `clamped_by` + `ceiling_source` 解释
//! 「为什么本机不能选 L4」。契约形状见 [`crate::cm_api_contract::permission`]，
//! 路由表见 [`crate::web::routes::permission`]。
//!
//! **不提供写 `ceiling` 的接口**（§16.3）：上限只能由启动期配置 / 绑定地址推导，
//! 或经 `POST /config/reload` 降下来。

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::cm_agent::permission::{
    PermissionTurnInput, project_caps, resolve_permission_level_for_turn, selectable_levels,
};
use crate::cm_api_contract::ApiError;
use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::permission::{PermissionLevelSource, PermissionView};
use crate::cm_config::AgentConfig;
use crate::cm_internal::agent_role_turn::turn_role_max_permission_level_for_web_or_cli_job;
use crate::cm_internal::session_mode_turn::{
    resolve_session_mode_for_turn, role_default_session_mode_for_turn,
};
use crate::cm_types::{PermissionLevel, SessionMode};
use crate::web::app_state_facets::WebChatAppFacet;
use crate::web::chat_handlers::normalize_agent_role;

/// `GET /permission` 的查询参数。
#[derive(Debug, Deserialize)]
pub(crate) struct PermissionQuery {
    /// 即将使用的命名角色 id，决定角色上限 `max_permission_level`。
    ///
    /// 缺省时按 `roles_prompts.default_agent_role_id` 算——但客户端**应当**带上一轮真正要用的
    /// `agent_role`：不带只会拿到按默认角色算的 `available`，再被服务端钳制（§16.2）。
    pub agent_role: Option<String>,
    /// 可选会话 id：取该会话持久化的 `agent_role` / `session_mode` / `permission_level`。
    ///
    /// 会话级等级由 `POST /chat` 的 `permission_level` 写入（S5b）；会话从未指定时
    /// `persisted_level` 为 `None`，`level_source` 即为 `config`。
    pub conversation_id: Option<String>,
}

/// 视图组装的输入：把 IO 结果摘出来，让视图本身可以纯函数单测。
pub(crate) struct PermissionViewInput<'a> {
    pub cfg: &'a AgentConfig,
    /// 请求里显式指定的角色（`?agent_role=`）。
    pub requested_agent_role: Option<&'a str>,
    /// 会话持久化的角色（`?conversation_id=`）。
    pub persisted_agent_role: Option<&'a str>,
    /// 会话持久化的会话模式。
    pub persisted_session_mode: Option<&'a str>,
    /// 会话持久化的等级意图（S5b 起可能非空）。
    pub persisted_level: Option<PermissionLevel>,
}

/// 组装 `GET /permission` 的响应（纯函数：无 IO、无全局状态）。
pub(crate) fn build_permission_view(input: PermissionViewInput<'_>) -> PermissionView {
    let cfg = input.cfg;
    let role_max = turn_role_max_permission_level_for_web_or_cli_job(
        cfg,
        input.persisted_agent_role,
        input.requested_agent_role,
    );
    let session_mode = session_mode_for_projection(
        cfg,
        input.persisted_agent_role,
        input.requested_agent_role,
        input.persisted_session_mode,
    );
    let resolved = resolve_permission_level_for_turn(PermissionTurnInput {
        cfg,
        requested: None,
        persisted: input.persisted_level,
        role_max,
        session_mode,
    });
    // 投影取 `projection_level` 而不是 `effective`：`ask` / `plan` 的只读由既有
    // `ReviewReadonly` 通道执行，不能回灌成 L1 的正列表（见 `ResolvedPermission`）。
    let caps = project_caps(resolved.projection_level, cfg);
    PermissionView {
        level: resolved.effective.as_str().to_string(),
        level_source: if input.persisted_level.is_some() {
            PermissionLevelSource::Persisted
        } else {
            PermissionLevelSource::Config
        },
        ceiling: cfg.permission.effective_ceiling().as_str().to_string(),
        ceiling_source: cfg
            .permission
            .permission_level_max_source
            .as_str()
            .to_string(),
        available: selectable_levels(cfg, role_max, session_mode)
            .iter()
            .map(|level| level.as_str().to_string())
            .collect(),
        clamped_by: resolved
            .clamped_by
            .iter()
            .map(|clamp| clamp.as_str().to_string())
            .collect(),
        caps: caps.view(),
    }
}

/// 会话模式：与回合解析同一条优先级（持久化 → 角色默认 → 配置默认）。
///
/// 持久化串损坏时按 `Act` 投影并留日志：一个坏的历史字段不该让只读端点整页 500。
fn session_mode_for_projection(
    cfg: &AgentConfig,
    persisted_agent_role: Option<&str>,
    requested_agent_role: Option<&str>,
    persisted_session_mode: Option<&str>,
) -> SessionMode {
    let role_default =
        role_default_session_mode_for_turn(cfg, persisted_agent_role, requested_agent_role);
    match resolve_session_mode_for_turn(
        None,
        persisted_session_mode,
        role_default,
        cfg.roles_prompts.default_session_mode,
    ) {
        Ok(mode) => mode,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "GET /permission：持久化的 session_mode 非法，投影按 act 处理：{e}"
            );
            SessionMode::Act
        }
    }
}
/// `GET /conversation/messages` 的等级回显：`(active, effective)`。
///
/// - `active` = 会话持久化的**意图**（原样回传，非法串不瞎修）；
/// - `effective` = 走与回合同一条 min 链后的**生效**值。
///
/// 两者都必须给：等级会被 ceiling / 角色上限 / `ask`·`plan` 三重钳制，
/// 只回一个值就会出现「我明明切了 L4，怎么还是只读」且无从排查（设计 §16.1）。
/// 复用 [`resolve_permission_level_for_turn`] 而非重写比较，避免长出第二套判据。
pub(crate) fn permission_levels_for_snapshot(
    cfg: &AgentConfig,
    persisted_agent_role: Option<&str>,
    persisted_session_mode: Option<&str>,
    persisted_level: Option<&str>,
) -> (Option<String>, Option<String>) {
    let active = persisted_level
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let persisted_parsed = match crate::cm_types::parse_optional_permission_level(persisted_level) {
        Ok(level) => level,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "会话持久化的 permission_level 非法，回显按未指定处理：{e}"
            );
            None
        }
    };
    let role_max =
        turn_role_max_permission_level_for_web_or_cli_job(cfg, persisted_agent_role, None);
    let session_mode =
        session_mode_for_projection(cfg, persisted_agent_role, None, persisted_session_mode);
    let resolved = resolve_permission_level_for_turn(PermissionTurnInput {
        cfg,
        requested: None,
        persisted: persisted_parsed,
        role_max,
        session_mode,
    });
    (active, Some(resolved.effective.as_str().to_string()))
}

fn bad_request(code: &'static str, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (StatusCode::BAD_REQUEST, Json(ApiError::new(code, message)))
}

fn not_found(code: &'static str, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (StatusCode::NOT_FOUND, Json(ApiError::new(code, message)))
}

/// `GET /permission`：等级 + 上限 + 可选项 + 六轴投影。
pub(crate) async fn permission_handler(
    State(state): State<WebChatAppFacet>,
    Query(query): Query<PermissionQuery>,
) -> Result<Json<PermissionView>, (StatusCode, Json<ApiError>)> {
    let requested_agent_role = normalize_agent_role(query.agent_role.as_deref())
        .map_err(|msg| bad_request("INVALID_AGENT_ROLE", msg))?;
    let conversation_id = crate::web::normalize_client_conversation_id(query.conversation_id.as_deref())
        .map_err(|msg| bad_request(error_codes::INVALID_CONVERSATION_ID, msg))?;
    // 会话不存在就明确 404，而不是悄悄退回「无持久化」的静态投影：
    // 后者会让客户端把一份按默认配出来的等级当成该会话的等级。
    let seed = match conversation_id.as_deref() {
        None => None,
        Some(cid) => Some(state.load_conversation_seed(cid).await.ok_or_else(|| {
            not_found(
                error_codes::CONVERSATION_NOT_FOUND,
                "会话不存在或已过期",
            )
        })?),
    };
    let cfg = state.cfg.read().await;
    // 未知角色必须报错：静默回落默认角色会返回一份偏松的 `available`，
    // 客户端「先选错再纠正」正是 §16.2 要避免的体验。
    if let Some(role) = requested_agent_role.as_deref()
        && !cfg.roles_prompts.agent_roles.contains_key(role)
    {
        return Err(bad_request(
            "INVALID_AGENT_ROLE",
            format!("未知的 agent_role：{role}"),
        ));
    }
    let view = build_permission_view(PermissionViewInput {
        cfg: &cfg,
        requested_agent_role: requested_agent_role.as_deref(),
        persisted_agent_role: seed
            .as_ref()
            .and_then(|seed| seed.persisted_active_agent_role.as_deref()),
        persisted_session_mode: seed
            .as_ref()
            .and_then(|seed| seed.persisted_active_session_mode.as_deref()),
        persisted_level: match crate::cm_types::parse_optional_permission_level(
            seed.as_ref()
                .and_then(|seed| seed.persisted_active_permission_level.as_deref()),
        ) {
            Ok(level) => level,
            Err(e) => {
                // 坏的历史串按「未指定」处理：只读端点不该因一行脏数据整页 500。
                log::warn!(
                    target: "crabmate",
                    "GET /permission：持久化的 permission_level 非法，按未指定处理：{e}"
                );
                None
            }
        },
    });
    Ok(Json(view))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::CeilingSource;

    fn base_cfg() -> AgentConfig {
        let mut cfg = crate::config::load_config(None).expect("embed default config");
        // 启动期冻结后的典型 loopback 形态：默认 approved，上限由绑定地址推导成 unrestricted。
        cfg.permission.permission_level = PermissionLevel::Approved;
        cfg.permission.permission_level_max_effective = PermissionLevel::Unrestricted;
        cfg.permission.permission_level_max_source = CeilingSource::DerivedLoopback;
        cfg
    }

    fn view_for(cfg: &AgentConfig, persisted_session_mode: Option<&str>) -> PermissionView {
        build_permission_view(PermissionViewInput {
            cfg,
            requested_agent_role: None,
            persisted_agent_role: None,
            persisted_session_mode,
            persisted_level: None,
        })
    }

    #[test]
    fn act_projection_lists_every_level_and_reports_ceiling() {
        let cfg = base_cfg();
        let view = view_for(&cfg, None);
        assert_eq!(view.level, "approved");
        assert_eq!(view.level_source, PermissionLevelSource::Config);
        assert_eq!(view.ceiling, "unrestricted");
        assert_eq!(view.ceiling_source, "derived_loopback");
        assert_eq!(
            view.available,
            ["chat", "read", "workspace", "approved", "unrestricted"]
        );
        assert!(view.clamped_by.is_empty(), "{:?}", view.clamped_by);
        // 投影与 `self_config_info` 同源：同一个 `caps_view()`。
        assert_eq!(view.caps, PermissionLevel::Approved.caps_view());
    }

    /// `ask` / `plan`：可选项收缩到 L1，但 `caps` **不**回灌成 L1 —— 只读由既有的
    /// `ReviewReadonly` 通道执行（否则默认档的 Plan 会话会连带丢掉 MCP 工具）。
    #[test]
    fn ask_mode_narrows_available_without_degrading_caps() {
        let cfg = base_cfg();
        let view = view_for(&cfg, Some("ask"));
        assert_eq!(view.level, "read");
        assert_eq!(view.available, ["chat", "read"]);
        assert_eq!(view.clamped_by, ["session_mode"]);
        assert_eq!(view.caps, PermissionLevel::Approved.caps_view());
    }
    /// 会话快照回显必须同时给出「意图」与「生效」：`ask` 会话即便持久化了 L4 也只读到 L1。
    #[test]
    fn snapshot_reports_intent_and_effective_separately() {
        let cfg = base_cfg();
        let (active, effective) = permission_levels_for_snapshot(&cfg, None, Some("ask"), Some("unrestricted"));
        assert_eq!(active.as_deref(), Some("unrestricted"));
        assert_eq!(effective.as_deref(), Some("read"), "ask 钳到 ≤ read");

        let (active, effective) = permission_levels_for_snapshot(&cfg, None, Some("act"), Some("workspace"));
        assert_eq!(active.as_deref(), Some("workspace"));
        assert_eq!(effective.as_deref(), Some("workspace"));
    }

    /// 会话从未指定 + 坏串都按「未指定」回显，且 `effective` 回落配置默认而不是整页报错。
    #[test]
    fn snapshot_tolerates_missing_and_corrupt_persisted_level() {
        let cfg = base_cfg();
        let (active, effective) = permission_levels_for_snapshot(&cfg, None, None, None);
        assert_eq!(active, None);
        assert_eq!(effective.as_deref(), Some(cfg.permission.permission_level.as_str()));

        let (active, effective) = permission_levels_for_snapshot(&cfg, None, None, Some("root"));
        assert_eq!(active.as_deref(), Some("root"), "原样回传，便于用户看到脏值");
        assert_eq!(effective.as_deref(), Some(cfg.permission.permission_level.as_str()));
    }


    /// 上限被显式降到 `workspace`（启动期 `reconcile_default_level_with_ceiling` 会把默认等级
    /// 一并降到上限）：`available` 里不再出现更高的档。`clamped_by` 此时为空是**正确**的 ——
    /// 它是「本回合的请求被钳制」的记录，而这里默认等级在配置层就已经是上限。
    #[test]
    fn ceiling_clamps_available_set_and_config_level_follows_it() {
        let mut cfg = base_cfg();
        cfg.permission.permission_level = PermissionLevel::Workspace;
        cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
        cfg.permission.permission_level_max_source = CeilingSource::Config;
        let view = view_for(&cfg, None);
        assert_eq!(view.ceiling, "workspace");
        assert_eq!(view.ceiling_source, "config");
        assert_eq!(view.level, "workspace");
        assert_eq!(view.available, ["chat", "read", "workspace"]);
        assert!(view.clamped_by.is_empty(), "{:?}", view.clamped_by);
        assert_eq!(view.caps, PermissionLevel::Workspace.caps_view());
    }

    /// 会话里选过 L4、后来上限被降（§6.2 的重算路径）：`clamped_by` 必须报出 `ceiling`，
    /// 否则客户端会以为「切换没生效」而反复重试。
    #[test]
    fn persisted_level_above_ceiling_reports_ceiling_clamp() {
        let mut cfg = base_cfg();
        cfg.permission.permission_level = PermissionLevel::Workspace;
        cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
        cfg.permission.permission_level_max_source = CeilingSource::Config;
        let view = build_permission_view(PermissionViewInput {
            cfg: &cfg,
            requested_agent_role: None,
            persisted_agent_role: None,
            persisted_session_mode: None,
            persisted_level: Some(PermissionLevel::Unrestricted),
        });
        assert_eq!(view.level, "workspace");
        assert_eq!(view.level_source, PermissionLevelSource::Persisted);
        assert_eq!(view.clamped_by, ["ceiling"]);
        assert_eq!(view.available, ["chat", "read", "workspace"]);
    }

    /// 未知角色：视图构建层不校验（那是 handler 的 400），但指定了不存在的角色时
    /// 角色上限解析出 `None`，不应把 `available` 意外收紧或放宽。
    #[test]
    fn unknown_role_falls_back_to_no_role_cap_at_view_layer() {
        let cfg = base_cfg();
        let with_role = build_permission_view(PermissionViewInput {
            cfg: &cfg,
            requested_agent_role: Some("no-such-role"),
            persisted_agent_role: None,
            persisted_session_mode: None,
            persisted_level: None,
        });
        assert_eq!(with_role.available, view_for(&cfg, None).available);
        assert_eq!(with_role.level, "approved");
    }

    /// 持久化等级（S5b 起会出现）：会被 min 链接受并反映在 `level_source` 上。
    #[test]
    fn persisted_level_is_reported_with_persisted_source() {
        let cfg = base_cfg();
        let view = build_permission_view(PermissionViewInput {
            cfg: &cfg,
            requested_agent_role: None,
            persisted_agent_role: None,
            persisted_session_mode: None,
            persisted_level: Some(PermissionLevel::Workspace),
        });
        assert_eq!(view.level, "workspace");
        assert_eq!(view.level_source, PermissionLevelSource::Persisted);
        assert_eq!(view.caps, PermissionLevel::Workspace.caps_view());
    }
}

/// 真实 axum 路由的轻量 HTTP 冒烟（`start_test_serve`，跳过 Bearer 与 LLM）。
#[cfg(all(test, feature = "web"))]
#[path = "http_tests.rs"]
mod http_tests;
