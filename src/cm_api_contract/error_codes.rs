//! 机器可读 HTTP / SSE 错误码常量（与 `docs/命令行契约.md`、`docs/SSE协议.md` 对齐）。

/// 未授权或 Bearer / X-API-Key 无效。
pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
/// 队列已满。
pub const QUEUE_FULL: &str = "QUEUE_FULL";
/// 流式任务已结束或不可恢复。
pub const STREAM_JOB_GONE: &str = "STREAM_JOB_GONE";
/// 客户端声明的 SSE 协议版本低于服务端（非 0）。
pub const SSE_PROTOCOL_MISMATCH: &str = "SSE_PROTOCOL_MISMATCH";
/// 客户端声明的 SSE 协议版本高于服务端。
pub const SSE_CLIENT_TOO_NEW: &str = "SSE_CLIENT_TOO_NEW";
/// 客户端声明的 SSE 协议版本为 0（非法）。
pub const INVALID_SSE_CLIENT_PROTOCOL: &str = "INVALID_SSE_CLIENT_PROTOCOL";
/// 调用 LLM 前缺少 API 密钥。
pub const LLM_API_KEY_REQUIRED: &str = "LLM_API_KEY_REQUIRED";
/// 工作区未设置。
pub const WORKSPACE_NOT_SET: &str = "WORKSPACE_NOT_SET";
/// 配置热重载失败。
pub const CONFIG_RELOAD_FAILED: &str = "CONFIG_RELOAD_FAILED";
/// 会话存储后端切换失败。
pub const SESSION_STORE_SWITCH_FAILED: &str = "SESSION_STORE_SWITCH_FAILED";
/// 审批会话 id 无效。
pub const INVALID_APPROVAL_SESSION_ID: &str = "INVALID_APPROVAL_SESSION_ID";
/// 审批决策无效。
pub const INVALID_APPROVAL_DECISION: &str = "INVALID_APPROVAL_DECISION";
/// 找不到审批会话。
pub const APPROVAL_SESSION_NOT_FOUND: &str = "APPROVAL_SESSION_NOT_FOUND";
/// 审批会话已关闭。
pub const APPROVAL_SESSION_CLOSED: &str = "APPROVAL_SESSION_CLOSED";
/// 会话 id 无效。
pub const INVALID_CONVERSATION_ID: &str = "INVALID_CONVERSATION_ID";
/// 会话不存在。
pub const CONVERSATION_NOT_FOUND: &str = "CONVERSATION_NOT_FOUND";
/// 会话 revision 未知。
pub const CONVERSATION_REVISION_UNKNOWN: &str = "CONVERSATION_REVISION_UNKNOWN";
/// 乐观锁冲突。
pub const CONVERSATION_CONFLICT: &str = "CONVERSATION_CONFLICT";
/// 用户消息过大。
pub const MESSAGE_TOO_LARGE: &str = "MESSAGE_TOO_LARGE";
/// SSE 编码失败兜底。
pub const SSE_ENCODE: &str = "SSE_ENCODE";
/// 内部错误。
pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
/// 后台任务 id 不存在 / 从未创建（`GET /tools/jobs/{id}`、`POST .../cancel`）。
pub const JOB_NOT_FOUND: &str = "JOB_NOT_FOUND";
/// 后台任务已过 TTL+宽限被清理。
pub const JOB_EXPIRED: &str = "JOB_EXPIRED";
/// 请求头 `X-Workspace-Root` 与任务归属 workspace 不符。
pub const JOB_OWNERSHIP_MISMATCH: &str = "JOB_OWNERSHIP_MISMATCH";
/// `DELETE /memory/{id}` 路径参数不是十进制整数（`i64`）。
pub const INVALID_MEMORY_ID: &str = "INVALID_MEMORY_ID";
/// `GET /memory/list` 的 `kind` 不是受支持的枚举值。
pub const INVALID_MEMORY_KIND: &str = "INVALID_MEMORY_KIND";
/// 长期记忆（LTM）未启用（配置关闭或运行时未装配）。
pub const LONG_TERM_MEMORY_DISABLED: &str = "LONG_TERM_MEMORY_DISABLED";
/// 工具名路径参数格式非法（长度 > 128 或含 `[A-Za-z0-9_.:-]` 之外字符）。
pub const INVALID_TOOL_NAME: &str = "INVALID_TOOL_NAME";
/// 工具名未在任一来源注册（`GET/PUT /tools/{tool_name}*`）。
pub const TOOL_NOT_FOUND: &str = "TOOL_NOT_FOUND";
/// 工作区动态工具定义文件校验失败（`plugins/*.json`）。
pub const INVALID_PLUGIN_DEFINITION: &str = "INVALID_PLUGIN_DEFINITION";
/// 工作区动态工具文件不存在（`GET /tools/plugins/{file}`）。
pub const PLUGIN_NOT_FOUND: &str = "PLUGIN_NOT_FOUND";
/// skills 元工具调用失败。
pub const SKILL_INVOKE_FAILED: &str = "SKILL_INVOKE_FAILED";
/// `GET /tools` 的 `source` / `category` 查询参数不是受支持的枚举值（或 `limit`/`offset` 非法）。
pub const INVALID_TOOL_FILTER: &str = "INVALID_TOOL_FILTER";
