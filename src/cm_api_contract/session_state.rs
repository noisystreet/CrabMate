//! `POST /conversation/state` 的请求/响应契约（设计 `docs/design/permission_levels.md` §16.1）。
//!
//! 会话级运行态意图（`agent_role` / `session_mode` / `permission_level`）的**空闲态写入口**：
//! Client 切换后立刻 `POST`，服务端下一回合与 `GET /permission`、`GET /conversation/messages`
//! 即可读到新值，不必等下一轮 `/chat` 顺带写入。
//!
//! 三者存的都是**意图**：每回合仍按 §6.1 的 min 链重算（ceiling / 角色上限 / 会话模式），
//! 所以这条写路径不构成提权通道（MB-1）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cm_api_contract::permission::PermissionView;

/// `POST /conversation/state` 请求体：三个字段均可选，但**至少要给一个**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationStatePatch {
    /// 目标会话（须已持久化；不存在 → 404，不做 upsert：空行会污染会话列表与 TTL 淘汰）。
    ///
    /// 用 `Option` 而不是必填 `String`：`String` 缺字段会在 axum 的 `Json` 提取阶段变成 **422**
    /// （axum 自己的错误体），与本端点「400 + 机器可读 code」的契约不一致。缺失 / 空白一律
    /// 400 `INVALID_CONVERSATION_ID`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// 会话级权限等级**意图**，写法与 `POST /chat` 的 `permission_level` 一致。
    ///
    /// 允许高于生效上限：越界由回合内 min 链钳制，`PermissionView::clamped_by` 负责解释。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_level: Option<String>,
    /// 会话模式（`ask` / `plan` / `act`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    /// 命名角色 id（`agent_roles` 里已存在的那个；未知角色 → 400 `INVALID_AGENT_ROLE`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
}

/// `POST /conversation/state` 响应：写入后的会话状态 + 同一份权限投影。
///
/// 回带 `permission` 是为了让 Client 一次请求就刷新选择器（`available` / `clamped_by`），
/// 不必再打一次 `GET /permission`；两者的 `level` 都取自同一条 min 链，不会互相矛盾。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationStateView {
    pub conversation_id: String,
    /// 落库的等级**意图**（原样回显，可能高于上限——钳制结果见 `permission.level`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_permission_level: Option<String>,
    /// 落库的会话模式意图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_mode: Option<String>,
    /// 落库的命名角色意图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent_role: Option<String>,
    /// 写入后按当前配置算出的权限投影（与 `GET /permission` 同一份实现）。
    pub permission: PermissionView,
}
