//! 工具管理 HTTP JSON 契约（`GET /tools`、`GET /tools/{tool_name}`、`PUT /tools/{tool_name}/enabled`、`GET /tools/plugins`）。
//!
//! 仅 serde 类型，进程级只读投影 + 启停写入响应；与 `routes` / handler 共享。

use serde::{Deserialize, Serialize};

/// 过滤后、分页前按来源计数（满足 `builtin + dynamic + mcp == total`）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolSourceCounts {
    pub builtin: usize,
    pub dynamic: usize,
    pub mcp: usize,
    /// 在场（可被本回合注册）工具数。
    pub present: usize,
}

/// `GET /tools` 单条工具投影。
#[derive(Debug, Clone, Serialize)]
pub struct ToolListItem {
    pub name: String,
    pub description: String,
    /// `basic` / `development`。
    pub category: String,
    /// `builtin` / `dynamic` / `mcp`。
    pub source: String,
    pub dev_tags: Vec<String>,
    /// 是否在场；`false` 时给出 `absent_reason`。
    pub present: bool,
    /// `feature_not_built` / `config_disabled` / `disabled_by_policy`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absent_reason: Option<String>,
    /// 是否在**当前权限等级**下可用（设计 `permission_levels.md` §16.4）。
    ///
    /// 与 `present` 是两条独立的轴：`present` 说「本回合能不能被注册」（feature /
    /// 配置开关 / `disabled_tools`），本字段说「注册了能不能调」（等级投影后的
    /// `tool_allowed_by_caps`）。两者都为 `true` 才真的调得动。
    ///
    /// 基准是**进程默认等级**（`permission_level` 经 ceiling 钳制），不是某个会话的
    /// 等级——`GET /tools` 是「枚举 + 策略投影」，不引入 `?permission_level=` 过滤（Q7）。
    pub available_at_level: bool,
}

/// `GET /tools` 响应。
#[derive(Debug, Clone, Serialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolListItem>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
    pub counts: ToolSourceCounts,
    /// 本清单的**等级基准**（`chat` / `read` / `workspace` / `approved` / `unrestricted`）。
    ///
    /// 即 `available_at_level` 按哪一档算出来的：进程默认档经 `permission_level_max`
    /// 钳制后的值。必须回传——会话可以持久化自己的等级（S5b），与该档常常不同；
    /// 只给 bool 不给基准，客户端看到 `false` 无从解释，看到 `true` 也不知道能不能当真。
    pub permission_level: String,
    /// 当前工作区根；未设置工作区时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
}

/// `GET /tools/{tool_name}` 策略投影。
#[derive(Debug, Clone, Serialize)]
pub struct ToolPolicyView {
    pub read_only: bool,
    pub write_effect: bool,
    pub parallel_readonly_batch_allowed: bool,
    pub sync_default_inline: bool,
    pub wall_timeout_secs: u64,
    /// 命中的 `sub_agent_*_extra_tools` 集合名（如 `sub_agent_patch_write_extra_tools`）。
    pub sub_agent_extra_allow: Vec<String>,
    pub background_job_capable: bool,
    pub retry_eligible: bool,
}

/// `GET /tools/{tool_name}` 响应。
#[derive(Debug, Clone, Serialize)]
pub struct ToolDetailView {
    pub name: String,
    pub description: String,
    pub category: String,
    pub source: String,
    pub dev_tags: Vec<String>,
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absent_reason: Option<String>,
    /// 是否在**当前权限等级**下可用；与 [`ToolListItem::available_at_level`] 同一条判据、
    /// 同一档基准，否则清单与详情页会给出互相矛盾的答案（设计 §16.4）。
    pub available_at_level: bool,
    /// 工具的 JSON Schema `parameters`（内置走编译期缓存；动态工具读盘）。
    pub parameters: serde_json::Value,
    pub policy: ToolPolicyView,
}

/// `PUT /tools/{tool_name}/enabled` 请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct ToolEnabledBody {
    pub enabled: bool,
}

/// `PUT /tools/{tool_name}/enabled` 响应。
#[derive(Debug, Clone, Serialize)]
pub struct ToolEnabledResponse {
    pub name: String,
    pub enabled: bool,
    pub source: String,
    /// 已写入 `tool_overrides.json` 并就地生效，无需重启。
    pub effective_after_reload: bool,
    /// 落盘并就地生效后，按 D1 口径重算的真实在场态（供调用方察觉被前缀策略拦截等）。
    pub present: bool,
    /// 仍不在场时的原因（`feature_not_built` / `config_disabled` / `disabled_by_policy`）；在场时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absent_reason: Option<String>,
}

/// `GET /tools/plugins` 单条工作区动态工具文件投影。
#[derive(Debug, Clone, Serialize)]
pub struct DynamicToolFileView {
    /// 文件名（相对 `<workspace>/plugins/`）。
    pub file: String,
    pub name: String,
    pub description: String,
    /// 是否通过 `plugins/*.json` 校验。
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 其 `command` 程序名是否在 `allowed_commands` 白名单中。
    pub command_allowed: bool,
}

/// `GET /tools/plugins` 响应。
#[derive(Debug, Clone, Serialize)]
pub struct PluginsListResponse {
    pub plugins: Vec<DynamicToolFileView>,
    pub total: usize,
}

/// `GET /tools/plugins/{file}` 单文件详情投影（只读）。
#[derive(Debug, Clone, Serialize)]
pub struct DynamicToolFileDetail {
    /// 文件名（相对 `<workspace>/plugins/`）。
    pub file: String,
    pub name: String,
    pub description: String,
    /// 是否通过 `plugins/*.json` 校验。
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 其 `command` 程序名是否在 `allowed_commands` 白名单中。
    pub command_allowed: bool,
    /// 原始 `parameters` JSON Schema（解析失败时为 `null`）。
    pub parameters: serde_json::Value,
    /// 原始 `args` 字段。
    pub args: Vec<String>,
    /// 原始 `pass_args_json` 字段。
    pub pass_args_json: bool,
}
