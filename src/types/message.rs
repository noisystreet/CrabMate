//! `Message` / `MessageContent` 及会话变换 helpers。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// 将 `tool_calls[].function.arguments` 规范为上游可解析的 JSON 字符串。
///
/// 部分 OpenAI 兼容网关（如 DeepSeek）在错误响应中报告 **`invalid function arguments json string`**
///（常见内部码 2013）：**空串**、仅空白或非 JSON 片段（流式拼接未完成等）会在**下一轮**把整段历史发回时触发 HTTP 400。
///
/// 修复策略（按序）：合法 JSON 直接紧凑化；否则尝试把 **字符串值内未转义的控制字符**（常见为模型把多行代码直接写进 `"code": "…"`）写成 `\n` 等；再尝试 **截断补全**（流式停在未闭合引号时补上 `"` 与 `}`）。
#[must_use]
pub fn sanitize_tool_call_arguments_for_openai_compat(arguments: &str) -> String {
    let t = arguments.trim();
    if t.is_empty() {
        return "{}".to_string();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        return v.to_string();
    }
    let escaped = escape_raw_controls_inside_json_string_regions(t);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&escaped) {
        return v.to_string();
    }
    if let Some(s) = try_repair_truncated_tool_arguments_json(&escaped) {
        return s;
    }
    if let Some(s) = try_repair_truncated_tool_arguments_json(t) {
        return s;
    }
    "{}".to_string()
}

/// 按 JSON 字符串语义扫描：在 **双引号字符串内** 将未转义的 ASCII 控制字符写成 `\n` / `\uXXXX`，其余字节原样复制。
fn escape_raw_controls_inside_json_string_regions(t: &str) -> String {
    let mut out = String::with_capacity(t.len().saturating_add(16));
    let mut in_string = false;
    let mut escape = false;
    for ch in t.chars() {
        if escape {
            out.push(ch);
            escape = false;
            continue;
        }
        if in_string {
            match ch {
                '\\' => {
                    out.push('\\');
                    escape = true;
                }
                '"' => {
                    out.push('"');
                    in_string = false;
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    use std::fmt::Write;
                    let _ = write!(&mut out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
    }
    out
}

/// 流式或拷贝不完整时，arguments 可能停在 **字符串未闭合**；补上闭合引号并平衡外层 `{`。
fn try_repair_truncated_tool_arguments_json(t: &str) -> Option<String> {
    if !t.starts_with('{') {
        return None;
    }
    let mut out = String::with_capacity(t.len().saturating_add(8));
    let mut brace_depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for ch in t.chars() {
        out.push(ch);
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
    if !in_string || escape {
        return None;
    }
    out.push('"');
    while brace_depth > 0 {
        out.push('}');
        brace_depth -= 1;
    }
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .map(|v| v.to_string())
}

// ---------- 消息与请求 ----------

/// `message.content`：OpenAI 兼容的 **字符串** 或 **多模态片段数组**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    /// 如 `[{"type":"text","text":"…"},{"type":"image_url","image_url":{"url":"…"}}]`。
    Parts(Vec<serde_json::Value>),
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Text(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Text(s.to_string())
    }
}

/// 保证 `content` 为 [`MessageContent::Text`]（`None` / `Parts` 会重置为空串）并返回可变引用。
pub fn message_content_get_or_insert_empty_text(
    content: &mut Option<MessageContent>,
) -> &mut String {
    match content {
        Some(MessageContent::Text(s)) => s,
        _ => {
            *content = Some(MessageContent::Text(String::new()));
            match content {
                Some(MessageContent::Text(s)) => s,
                _ => unreachable!("just assigned Text"),
            }
        }
    }
}

/// 对话消息（OpenAI 兼容格式）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<MessageContent>,
    /// DeepSeek 思考模式等响应中的思维链（与 `content` 同级，见 [思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode)）；出站默认剥离，**仅**在官方 DeepSeek **`api_base`** 且该助手含 **`tool_calls`** 时多轮回传（`preserve_deepseek_thinking_reasoning_roundtrip`）。Moonshot **kimi-k2.5** + thinking 时对含 **`tool_calls`** 的助手由 `preserve_reasoning_on_assistant_tool_calls` 保留。
    /// 部分网关（如 Ollama 对 Qwen3）流式/非流式使用键名 **`reasoning`**，与 **`reasoning_content`** 等价。
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "reasoning")]
    pub reasoning_content: Option<String>,
    /// MiniMax OpenAI 兼容在 **`reasoning_split: true`** 时，非流式响应可能在 `message` 上返回；解析后合并入 [`Self::reasoning_content`] 并清空本字段，**不**回传上游。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 聊天区装饰短分隔线（分阶段规划每步结束等）；`name` 标记，不送入模型。
#[inline]
pub fn is_chat_ui_separator(m: &Message) -> bool {
    m.role == "system" && m.name.as_deref() == Some("crabmate_ui_sep")
}

/// Web UI 时间线占位（审批结果等）；**不**送入模型 API。
#[inline]
pub fn is_chat_timeline_marker(m: &Message) -> bool {
    m.role == "system" && m.name.as_deref() == Some("crabmate_timeline")
}

/// 仅用于聊天 UI / 摘要等「不进上游 chat/completions」的过滤；**不含**长期记忆注入（调用处另按需过滤）。
#[inline]
pub fn is_message_excluded_from_llm_context_except_memory(m: &Message) -> bool {
    is_chat_ui_separator(m) || is_chat_timeline_marker(m)
}

/// 供 **`GET /conversation/messages`** 等客户端只读视图：去掉不应展示的会话内注入（与落盘前 `strip_*` 一致）。
///
/// 另：**不**返回普通 **`system`** 正文（[`Message::system_only`] 等），聊天 UI 与导出 Markdown 均不向用户展示系统提示词；仅保留 **`name == crabmate_timeline`** 的时间线旁注供前端还原步骤条。
/// 亦不返回首轮工作区画像注入（[`is_first_turn_workspace_context_injection`]）：仍落盘并送模型，但不在聊天区展示。
pub fn filter_messages_for_web_client_snapshot(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| is_message_visible_in_chat_transcript(m))
        .cloned()
        .collect()
}

/// Web 聊天区/水合列表是否应省略该条：**`system`** 且**非**时间线占位（[`is_chat_timeline_marker`]）。
#[inline]
fn is_system_role_hidden_from_web_transcript(m: &Message) -> bool {
    m.role == "system" && !is_chat_timeline_marker(m)
}

/// Web 聊天区与 TUI transcript 是否展示该条（与 [`filter_messages_for_web_client_snapshot`] 过滤条件一致）。
///
/// 省略：普通 **`system`**（含系统提示词）、长期记忆 / 工作区变更集 / 首轮工作区画像等 **`user.name`** 注入；保留 **`crabmate_timeline`** 等时间线 **`system`**。
#[inline]
pub fn is_message_visible_in_chat_transcript(m: &Message) -> bool {
    !crate::types::server_injected_user::is_server_injected_user_message(m)
        && !is_system_role_hidden_from_web_transcript(m)
}

/// 长期记忆注入条目的 `user.name`；仅供模型上下文使用，**不得**发往供应商 API。
pub const CRABMATE_LONG_TERM_MEMORY_NAME: &str = "crabmate_long_term_memory";

/// 会话工作区变更集注入（`user.name`）；每次调模型前由运行时刷新，**不应**持久化到会话存储。
pub const CRABMATE_WORKSPACE_CHANGELIST_NAME: &str = "crabmate_workspace_changelist";

/// 新会话首轮「工作区 / 项目画像」等上下文注入（`user.name`）；供模型读取，**不向** Web 快照与聊天水合展示。
pub const CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME: &str =
    "crabmate_first_turn_workspace_context";

/// 分阶段无工具规划轮：模型违规输出 `tool_calls` 后的一次性重写约束（`user.name`）；送模型，**不向** Web 快照与聊天水合展示。
pub const CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME: &str = "crabmate_planner_tool_call_reject";

/// 分阶段单步执行注入（`user.name`）；送模型，聊天区与快照过滤。
pub const CRABMATE_STAGED_STEP_INJECTION_NAME: &str = "crabmate_staged_step_injection";

/// 分阶段规划 coach / ensemble / 优化轮等（`user.name`）。
pub const CRABMATE_STAGED_PLAN_COACH_NAME: &str = "crabmate_staged_plan_coach";

/// 两阶段 NL 展示桥接（`user.name`）。
pub const CRABMATE_STAGED_NL_FOLLOWUP_NAME: &str = "crabmate_staged_nl_followup";

/// 分阶段步失败补丁规划反馈（`user.name`）。
pub const CRABMATE_STAGED_PATCH_FEEDBACK_NAME: &str = "crabmate_staged_patch_feedback";

/// 终答 `plan_rewrite` / 侧向语义反馈（`user.name`）。
pub const CRABMATE_PLAN_REWRITE_NAME: &str = "crabmate_plan_rewrite";

/// 规划轮拒绝重写 user 正文首行；兼容未带 `name` 的历史落盘。
pub const STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX: &str =
    "### 规划轮约束提醒（code=PLANNER_TOOL_CALL_REJECTED）";

/// 意图门控将 canned 改为走主模型时，首轮 P 前临时插入的 `system.name`；调用后须从会话中剔除，避免落盘污染。
pub const CRABMATE_INTENT_GATE_HINT_NAME: &str = "crabmate_intent_gate_hint";

#[inline]
pub fn is_intent_gate_ephemeral_system(m: &Message) -> bool {
    m.role == "system" && m.name.as_deref() == Some(CRABMATE_INTENT_GATE_HINT_NAME)
}

#[inline]
pub fn is_long_term_memory_injection(m: &Message) -> bool {
    m.role == "user" && m.name.as_deref() == Some(CRABMATE_LONG_TERM_MEMORY_NAME)
}

#[inline]
pub fn is_workspace_changelist_injection(m: &Message) -> bool {
    m.role == "user" && m.name.as_deref() == Some(CRABMATE_WORKSPACE_CHANGELIST_NAME)
}

#[inline]
pub fn is_first_turn_workspace_context_injection(m: &Message) -> bool {
    m.role == "user" && m.name.as_deref() == Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME)
}

#[inline]
pub fn is_planner_tool_call_reject_injection(m: &Message) -> bool {
    if m.role != "user" {
        return false;
    }
    if m.name.as_deref() == Some(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME) {
        return true;
    }
    message_content_as_str(&m.content)
        .map(|c| {
            c.trim_start()
                .starts_with(STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX)
        })
        .unwrap_or(false)
}

/// `POST /chat/branch` 等按序截断时计入的「真实用户发言」：排除各类 `user.name` 注入条。
#[inline]
pub fn user_message_counts_for_branch_truncation(m: &Message) -> bool {
    m.role == "user" && !crate::types::server_injected_user::is_server_injected_user_message(m)
}

/// `message.content` 为纯文本时的借用；多模态 [`MessageContent::Parts`] 返回 `None`。
#[inline]
pub fn message_content_as_str(content: &Option<MessageContent>) -> Option<&str> {
    match content {
        Some(MessageContent::Text(s)) => Some(s.as_str()),
        Some(MessageContent::Parts(_)) | None => None,
    }
}

/// 聊天区等 UI 纯文本展示：**`Text`** 全文，或 **`Parts`** 内各 **`text`** 字段（非空）按序拼接。
#[must_use]
pub fn message_content_plain_for_chat_display(content: &Option<MessageContent>) -> String {
    match content {
        None => String::new(),
        Some(MessageContent::Text(s)) => s.clone(),
        Some(MessageContent::Parts(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// 供字符预算等估算：字符串取其 `len()`；多模态数组累加各 `text` 段长度（近似）。
pub fn message_content_byte_len_for_estimate(content: &Option<MessageContent>) -> usize {
    match content {
        None => 0,
        Some(MessageContent::Text(s)) => s.len(),
        Some(MessageContent::Parts(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .map(|t| t.len())
            .sum(),
    }
}

/// 从助手等非流式响应取纯文本正文；多模态 `Parts` 返回空串（摘要/日志路径不展开图片）。
pub fn message_content_into_text_lossy(content: Option<MessageContent>) -> String {
    match content {
        None => String::new(),
        Some(MessageContent::Text(s)) => s,
        Some(MessageContent::Parts(_)) => String::new(),
    }
}

/// 是否视为「无正文」（`None`、空白字符串、空片段数组）。
pub fn message_content_is_effectively_empty(m: &Message) -> bool {
    match &m.content {
        None => true,
        Some(MessageContent::Text(s)) => s.trim().is_empty(),
        Some(MessageContent::Parts(a)) => a.is_empty(),
    }
}

/// 将 `system` 折叠进 `user` 时合并正文：支持纯字符串与多模态数组（前缀写入首段 `text` 或新增 `text` 块）。
pub fn merge_system_text_prefix_into_user_content(msg: &mut Message, prefix: &str) {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return;
    }
    match std::mem::take(&mut msg.content) {
        None => {
            msg.content = Some(MessageContent::Text(prefix.to_string()));
        }
        Some(MessageContent::Text(s)) => {
            let u = s.trim();
            msg.content = Some(MessageContent::Text(if u.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}\n\n{u}")
            }));
        }
        Some(MessageContent::Parts(mut parts)) => {
            if let Some(serde_json::Value::Object(obj)) = parts.first_mut()
                && obj.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(serde_json::Value::String(t)) = obj.get_mut("text")
            {
                let u = t.trim();
                *t = if u.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}\n\n{u}")
                };
                msg.content = Some(MessageContent::Parts(parts));
                return;
            }
            let mut new_parts = Vec::with_capacity(parts.len() + 1);
            new_parts.push(serde_json::json!({"type": "text", "text": prefix}));
            new_parts.extend(parts);
            msg.content = Some(MessageContent::Parts(new_parts));
        }
    }
}

/// 构建带图片的 `user` 消息（OpenAI 兼容 `content` 数组）；`text` 可为空（仅图）。
pub fn message_user_with_images(text: &str, image_urls: &[String]) -> Message {
    let mut parts = Vec::new();
    let t = text.trim();
    if !t.is_empty() {
        parts.push(serde_json::json!({"type": "text", "text": t}));
    }
    for url in image_urls {
        let u = url.trim();
        if u.is_empty() {
            continue;
        }
        parts.push(serde_json::json!({
            "type": "image_url",
            "image_url": {"url": u}
        }));
    }
    let content = if parts.is_empty() {
        None
    } else {
        Some(MessageContent::Parts(parts))
    };
    Message {
        role: "user".to_string(),
        content,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

impl Message {
    /// 分阶段规划：每步完成后的短分隔线。仅用于 UI 与同步，调用模型前须过滤。
    pub fn chat_ui_separator(short: bool) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(
                if short { "short" } else { "long" }.to_string(),
            )),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("crabmate_ui_sep".to_string()),
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `system` 消息（Web 首轮、CLI、TUI 空会话等共用）。
    pub fn system_only(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 意图门控注入的临时 system（[`CRABMATE_INTENT_GATE_HINT_NAME`]）；**不得**长期留在 `messages` 中。
    pub fn system_intent_gate_hint(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_INTENT_GATE_HINT_NAME.to_string()),
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `user` 消息。
    pub fn user_only(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 首轮工作区 / 项目画像等上下文（`user` + [`CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME`]）；送模型，Web 快照过滤。
    pub fn user_first_turn_workspace_context(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME.to_string()),
            tool_call_id: None,
        }
    }

    /// 规划轮 tool_calls 拒绝后的一次性重写约束（[`CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME`]）；Web 快照过滤。
    pub fn user_planner_tool_call_reject_injection(content: impl Into<String>) -> Self {
        Self::user_server_injection(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME, content)
    }

    /// 服务端编排注入 user（`user.name` 须为 [`CRABMATE_*`] 注册名）。
    pub fn user_server_injection(name: &'static str, content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.to_string()),
            tool_call_id: None,
        }
    }

    /// 分阶段单步执行注入。
    pub fn user_staged_step_injection(content: impl Into<String>) -> Self {
        Self::user_server_injection(CRABMATE_STAGED_STEP_INJECTION_NAME, content)
    }

    /// 终答规划重写 / 语义反馈 user。
    pub fn user_plan_rewrite_injection(content: impl Into<String>) -> Self {
        Self::user_server_injection(CRABMATE_PLAN_REWRITE_NAME, content)
    }

    /// 分阶段路径：按正文特征选择 [`CRABMATE_STAGED_*`] `name`。
    pub fn user_staged_orchestration_injection(content: impl Into<String>) -> Self {
        let body = content.into();
        let name =
            crate::types::server_injected_user::staged_injection_user_name_for_content(&body);
        Self::user_server_injection(name, body)
    }

    /// 无 `tool_calls` 的 `assistant` 消息（如分阶段规划补丁合并后的 JSON 快照）。
    pub fn assistant_only(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }
}

/// 单条消息：发往供应商时默认去掉 `reasoning_content` / `reasoning_details`（与 [`messages_stripping_reasoning_for_api_request`] 单元素语义一致）。
///
/// **`preserve_reasoning_on_assistant_tool_calls`**：Moonshot **kimi-k2.5** 在 **thinking 启用** 时要求含 **`tool_calls`** 的 assistant 必须带 **`reasoning_content`**；为真时对该类消息在合并 **`reasoning_details`** 后保留思维链，若仍为空则写入空串以便 JSON 带出该字段。
///
/// **`preserve_deepseek_thinking_reasoning_roundtrip`**：与 [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode) 中「工具调用」一致——**仅**含非空 **`tool_calls`** 的助手须在后续请求回传 **`reasoning_content`**。
#[inline]
pub(crate) fn message_clone_stripping_reasoning_for_api(
    m: &Message,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Message {
    let is_asst = is_assistant_role(m.role.as_str());
    let tc = assistant_has_non_empty_tool_calls(m);
    let keep_kimi = preserve_reasoning_on_assistant_tool_calls && is_asst && tc;
    let keep_deepseek = preserve_deepseek_thinking_reasoning_roundtrip && is_asst && tc;
    let keep = keep_kimi || keep_deepseek;
    if keep {
        let mut x = m.clone();
        merge_reasoning_details_into_reasoning_content(&mut x);
        x.reasoning_details = None;
        if x.reasoning_content.is_none() {
            x.reasoning_content = Some(String::new());
        }
        return x;
    }
    if m.reasoning_content.is_none() && m.reasoning_details.is_none() {
        m.clone()
    } else {
        Message {
            reasoning_content: None,
            reasoning_details: None,
            ..m.clone()
        }
    }
}

/// MiniMax OpenAI 兼容：非流式 `message` 上 **`reasoning_details`**（`[{"text":"…"}]`）合并进 **`reasoning_content`** 并清空 **`reasoning_details`**，避免写入会话后再回传上游。
pub fn merge_reasoning_details_into_reasoning_content(msg: &mut Message) {
    let Some(details) = msg.reasoning_details.take() else {
        return;
    };
    let mut from_details = String::new();
    for d in details {
        let Some(obj) = d.as_object() else {
            continue;
        };
        if let Some(serde_json::Value::String(t)) = obj.get("text") {
            from_details.push_str(t);
        }
    }
    if from_details.is_empty() {
        return;
    }
    let replace = match msg.reasoning_content.as_deref() {
        None | Some("") => true,
        Some(rc) => from_details.starts_with(rc) && from_details.len() >= rc.len(),
    };
    if replace {
        msg.reasoning_content = Some(from_details);
    }
}

/// 构造发往供应商的 `messages`：去掉助手 `reasoning_content`，避免多轮请求回传思维链。
#[allow(dead_code)] // 公共 API；`tool_chat_request` 已用 `messages_for_api_stripping_reasoning_skip_ui_separators` 合并遍历；单测保留等价断言
pub fn messages_stripping_reasoning_for_api_request(
    messages: &[Message],
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    messages
        .iter()
        .map(|m| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect()
}

/// 会话切片 → API 消息：**跳过** [`is_chat_ui_separator`] 与 [`is_long_term_memory_injection`]，并按策略剥离 `reasoning_content`（见 [`message_clone_stripping_reasoning_for_api`]）。
/// 单次遍历，避免先 `filter+clone` 再 [`messages_stripping_reasoning_for_api_request`] 的二次全量拷贝。
pub fn messages_for_api_stripping_reasoning_skip_ui_separators(
    messages: &[Message],
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| {
            !is_message_excluded_from_llm_context_except_memory(m)
                && !is_long_term_memory_injection(m)
                && !is_workspace_changelist_injection(m)
        })
        .map(|m| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect()
}

#[inline]
fn assistant_has_non_empty_tool_calls(m: &Message) -> bool {
    m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
}

/// 与供应商对齐：部分会话/导入里 `role` 可能带空白或大小写不一致，严格 `== "assistant"` 会漏合并。
#[inline]
fn is_assistant_role(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("assistant")
}

fn merge_adjacent_assistant_text(into: &mut Message, from: &Message) {
    let a = message_content_as_str(&into.content)
        .map(str::trim)
        .unwrap_or("");
    let b = message_content_as_str(&from.content)
        .map(str::trim)
        .unwrap_or("");
    into.content = match (a.is_empty(), b.is_empty()) {
        (true, true) => None,
        (false, true) => into.content.clone(),
        (true, false) => from.content.clone(),
        (false, false) => {
            if b.starts_with(a) {
                from.content.clone()
            } else if a.starts_with(b) {
                into.content.clone()
            } else {
                Some(MessageContent::Text(format!("{a}\n\n{b}")))
            }
        }
    };
}

/// 将两条相邻的 `assistant` 压成一条（OpenAI 兼容 API 禁止相邻 assistant）。
///
/// 覆盖此前漏洞：若**后一条**带 `tool_calls`，旧实现会 `push` 第二条，仍触发 400。
fn squash_consecutive_assistant_pair(into: &mut Message, from: Message) {
    let from_has_tc = assistant_has_non_empty_tool_calls(&from);
    let into_has_tc = assistant_has_non_empty_tool_calls(into);
    let from_empty = message_content_is_effectively_empty(&from);

    if into_has_tc && !from_has_tc {
        if from_empty {
            into.tool_calls = None;
            return;
        }
        into.tool_calls = None;
        merge_adjacent_assistant_text(into, &from);
        return;
    }

    if into_has_tc && from_has_tc {
        merge_adjacent_assistant_text(into, &from);
        let mut a = into.tool_calls.take().unwrap_or_default();
        a.extend(from.tool_calls.unwrap_or_default());
        into.tool_calls = if a.is_empty() { None } else { Some(a) };
        return;
    }

    if !into_has_tc && from_has_tc {
        merge_adjacent_assistant_text(into, &from);
        into.tool_calls = from.tool_calls;
        return;
    }

    merge_adjacent_assistant_text(into, &from);
}

/// 反复合并相邻 `assistant`（不删尾部空占位、不处理孤儿 `tool_calls`），供会话内存与裁剪后修复。
fn merge_all_consecutive_assistant_messages_in_vec(mut out: Vec<Message>) -> Vec<Message> {
    loop {
        let mut merged_any = false;
        let mut i = 0usize;
        while i + 1 < out.len() {
            if !(is_assistant_role(&out[i].role) && is_assistant_role(&out[i + 1].role)) {
                i += 1;
                continue;
            }
            out[i].role = "assistant".to_string();
            let next = out.remove(i + 1);
            squash_consecutive_assistant_pair(&mut out[i], next);
            merged_any = true;
            i = i.saturating_sub(1);
        }
        if !merged_any {
            break;
        }
    }
    out
}

/// 就地合并相邻 `assistant`，**保留**末尾空助手占位（与 `normalize_messages_for_openai_compatible_request` 不同）。
pub fn merge_consecutive_assistants_in_place(messages: &mut Vec<Message>) {
    *messages = merge_all_consecutive_assistant_messages_in_vec(std::mem::take(messages));
}

/// 消除 OpenAI 兼容接口不允许的**相邻** `assistant`（无中间 `user`/`tool`）。
///
/// 典型来源：
/// - TUI 在用户消息后追加空助手占位，而历史中上一条已是助手正文；
/// - 流式阶段已向占位写入片段，模型回合结束又 `push` 了第二条助手；
/// - **`max_message_history` 等裁剪**删掉中间的 `role: tool`，却保留带 `tool_calls` 的 `assistant` 与其后的下一条助手；
/// - **后一条助手带 `tool_calls`**，前一条仅有正文：旧逻辑未合并，仍报 `Invalid consecutive assistant`。
///
/// 本函数仅用于拼装 **`ChatRequest.messages`**，**不**写入会话 `Vec<Message>`：会话尾部空占位须保留，
/// 供 [`crate::agent::agent_turn::push_assistant_merging_trailing_empty_placeholder`] 合并。
///
/// 末尾「仅有 `tool_calls`、无对应 `tool` 消息」的 assistant 会先清空 `tool_calls`（悬空调用非法）；
/// 若清空后正文仍为空，须再删掉该条，否则供应商返回 `content or tool_calls must be set`。
///
/// **`reasoning_content` 已剥离**后，会话里可能留下仅有思维链语义、正文为空的助手（中间或尾部）；
/// 须从**整条** `messages` 剔除，否则 DeepSeek 等返回 HTTP 400（`Invalid assistant message: content or tool_calls must be set`）。
pub fn normalize_messages_for_openai_compatible_request(msgs: Vec<Message>) -> Vec<Message> {
    let mut out = merge_all_consecutive_assistant_messages_in_vec(msgs);
    remove_all_assistants_lacking_openai_content_or_tool_calls(&mut out);
    if let Some(last) = out.last_mut()
        && is_assistant_role(&last.role)
        && assistant_has_non_empty_tool_calls(last)
    {
        last.tool_calls = None;
    }
    remove_all_assistants_lacking_openai_content_or_tool_calls(&mut out);
    out
}

#[inline]
fn role_is_system_for_vendor(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("system")
}

/// 将独立 **`role: "system"`** 折叠进后续 **`user`**，避免上游返回 HTTP 400（如 **`invalid message role: system`**）。MiniMax OpenAI 兼容域名上**实测常见**该错误；CrabMate 在识别为 MiniMax 时由 [`crate::llm::fold_system_into_user_for_config`] 为真并走本函数。
///
/// 将连续 **`system`** 的正文按顺序拼接后，合并进**下一条** **`user`** 的 `content` 之前（中间空一行）；若下一条非 `user`，则先插入一条仅含该拼接正文的 **`user`**。**不**写入会话，仅用于拼装出站 `ChatRequest.messages`。
pub fn fold_system_messages_into_following_user(msgs: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(msgs.len());
    let mut pending: Vec<String> = Vec::new();

    let push_merged_user = |pending: &mut Vec<String>, out: &mut Vec<Message>, mut msg: Message| {
        if pending.is_empty() {
            out.push(msg);
            return;
        }
        let prefix = pending.join("\n\n");
        pending.clear();
        merge_system_text_prefix_into_user_content(&mut msg, &prefix);
        out.push(msg);
    };

    for m in msgs {
        if role_is_system_for_vendor(&m.role) {
            if let Some(c) = message_content_as_str(&m.content)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                pending.push(c.to_string());
            }
            continue;
        }
        let is_user = m.role.trim().eq_ignore_ascii_case("user");
        if is_user {
            push_merged_user(&mut pending, &mut out, m);
        } else {
            if !pending.is_empty() {
                let prefix = pending.join("\n\n");
                pending.clear();
                out.push(Message::user_only(prefix));
            }
            out.push(m);
        }
    }
    if !pending.is_empty() {
        out.push(Message::user_only(pending.join("\n\n")));
    }
    out
}

/// `assistant` 在 OpenAI 兼容 `chat/completions` 下非法：**无**非空 `content`（含多模态 `Parts`）且**无** `tool_calls`。
#[inline]
fn assistant_lacks_openai_content_and_tool_calls(m: &Message) -> bool {
    if !is_assistant_role(m.role.as_str()) {
        return false;
    }
    if assistant_has_non_empty_tool_calls(m) {
        return false;
    }
    message_content_as_str(&m.content)
        .map(|s| s.trim().is_empty())
        .unwrap_or_else(|| message_content_is_effectively_empty(m))
}

/// 删除**任意位置**「无正文且无 `tool_calls`」的 assistant（与仅 `pop` 尾部等价类，兼修 strip 后留在中间的垃圾助手）。
fn remove_all_assistants_lacking_openai_content_or_tool_calls(out: &mut Vec<Message>) {
    out.retain(|m| !assistant_lacks_openai_content_and_tool_calls(m));
}

/// Web `/chat`、队列任务与 CLI 单次问答共用的首轮：`[system, user]`。
pub fn messages_chat_seed(system_prompt: &str, user_text: &str) -> Vec<Message> {
    vec![
        Message::system_only(system_prompt.to_string()),
        Message::user_only(user_text.to_string()),
    ]
}
