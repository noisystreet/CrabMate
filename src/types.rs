//! API 与对话相关类型

use serde::{Deserialize, Serialize};

/// 拼接在 `api_base` 后的 OpenAI 兼容 chat 路径（无前导斜杠）。
pub const OPENAI_CHAT_COMPLETIONS_REL_PATH: &str = "chat/completions";

/// 拼接在 `api_base` 后的 OpenAI 兼容模型列表路径（`GET`，无前导斜杠）；部分网关可能未实现。
pub const OPENAI_MODELS_REL_PATH: &str = "models";

/// 单次 `run_agent_turn` / HTTP 请求对 `chat/completions` 的 **`seed`** 覆盖（OpenAI 兼容字段；供应商不支持时通常会忽略）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LlmSeedOverride {
    /// 使用 [`AgentConfig::llm_seed`]（未配置则请求体不带 `seed`）。
    #[default]
    FromConfig,
    /// 强制在请求 JSON 中写入该整数 `seed`。
    Fixed(i64),
    /// 本回合请求体**不**含 `seed`（即使配置里设置了默认 seed）。
    OmitFromRequest,
}

/// 合并配置中的默认 seed 与单次回合覆盖，得到写入 `ChatRequest.seed` 的值。
#[inline]
pub fn resolved_llm_seed(base: Option<i64>, override_: LlmSeedOverride) -> Option<i64> {
    match override_ {
        LlmSeedOverride::FromConfig => base,
        LlmSeedOverride::Fixed(n) => Some(n),
        LlmSeedOverride::OmitFromRequest => None,
    }
}

#[cfg(test)]
mod llm_seed_tests {
    use super::{LlmSeedOverride, resolved_llm_seed};

    #[test]
    fn resolved_seed_respects_override() {
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::FromConfig),
            Some(1)
        );
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::Fixed(42)),
            Some(42)
        );
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::OmitFromRequest),
            None
        );
        assert_eq!(resolved_llm_seed(None, LlmSeedOverride::FromConfig), None);
    }
}

// ---------- 消息与请求 ----------

/// 对话消息（OpenAI 兼容格式）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    /// DeepSeek `deepseek-reasoner` 等非流式/流式响应中的思维链；**勿**在下一轮请求中回传供应商（见 [`messages_stripping_reasoning_for_api_request`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// 长期记忆注入条目的 `user.name`；仅供模型上下文使用，**不得**发往供应商 API。
pub const CRABMATE_LONG_TERM_MEMORY_NAME: &str = "crabmate_long_term_memory";

/// 会话工作区变更集注入（`user.name`）；每次调模型前由运行时刷新，**不应**持久化到会话存储。
pub const CRABMATE_WORKSPACE_CHANGELIST_NAME: &str = "crabmate_workspace_changelist";

#[inline]
pub fn is_long_term_memory_injection(m: &Message) -> bool {
    m.role == "user" && m.name.as_deref() == Some(CRABMATE_LONG_TERM_MEMORY_NAME)
}

#[inline]
pub fn is_workspace_changelist_injection(m: &Message) -> bool {
    m.role == "user" && m.name.as_deref() == Some(CRABMATE_WORKSPACE_CHANGELIST_NAME)
}

impl Message {
    /// 分阶段规划：每步完成后的短分隔线。仅用于 UI 与同步，调用模型前须过滤。
    pub fn chat_ui_separator(short: bool) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(if short { "short" } else { "long" }.to_string()),
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
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `user` 消息。
    pub fn user_only(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `assistant` 消息（如分阶段规划补丁合并后的 JSON 快照）。
    pub fn assistant_only(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }
}

/// 单条消息：供 API 请求使用，不携带 `reasoning_content`（与 [`messages_stripping_reasoning_for_api_request`] 单元素语义一致）。
#[inline]
pub(crate) fn message_clone_stripping_reasoning_for_api(m: &Message) -> Message {
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
pub fn messages_stripping_reasoning_for_api_request(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(message_clone_stripping_reasoning_for_api)
        .collect()
}

/// 会话切片 → API 消息：**跳过** [`is_chat_ui_separator`] 与 [`is_long_term_memory_injection`]，并剥离 `reasoning_content`。
/// 单次遍历，避免先 `filter+clone` 再 [`messages_stripping_reasoning_for_api_request`] 的二次全量拷贝。
pub fn messages_for_api_stripping_reasoning_skip_ui_separators(
    messages: &[Message],
) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| {
            !is_message_excluded_from_llm_context_except_memory(m)
                && !is_long_term_memory_injection(m)
                && !is_workspace_changelist_injection(m)
        })
        .map(message_clone_stripping_reasoning_for_api)
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
    let a = into.content.as_deref().map(str::trim).unwrap_or("");
    let b = from.content.as_deref().map(str::trim).unwrap_or("");
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
                Some(format!("{a}\n\n{b}"))
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
    let from_empty = from
        .content
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);

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
pub fn normalize_messages_for_openai_compatible_request(msgs: Vec<Message>) -> Vec<Message> {
    let mut out = merge_all_consecutive_assistant_messages_in_vec(msgs);
    pop_trailing_assistants_with_neither_content_nor_tool_calls(&mut out);
    if let Some(last) = out.last_mut()
        && is_assistant_role(&last.role)
        && assistant_has_non_empty_tool_calls(last)
    {
        last.tool_calls = None;
    }
    pop_trailing_assistants_with_neither_content_nor_tool_calls(&mut out);
    out
}

#[inline]
fn role_is_system_for_vendor(role: &str) -> bool {
    role.trim().eq_ignore_ascii_case("system")
}

/// 将独立 **`role: "system"`** 折叠进后续 **`user`**，避免上游返回 HTTP 400（如 **`invalid message role: system`**）。MiniMax OpenAI 兼容域名上**实测常见**该错误，与文档示例不完全一致；由配置 **`llm_fold_system_into_user`** 控制（嵌入 TOML 默认 **`false`**；接 MiniMax 等时通常需 **`true`**）。
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
        let merged = match msg.content.as_deref().map(str::trim) {
            Some(u) if !u.is_empty() => format!("{prefix}\n\n{u}"),
            _ => prefix,
        };
        msg.content = if merged.is_empty() {
            None
        } else {
            Some(merged)
        };
        out.push(msg);
    };

    for m in msgs {
        if role_is_system_for_vendor(&m.role) {
            if let Some(c) = m
                .content
                .as_ref()
                .map(|s| s.trim())
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

/// 删除尾部「无正文且无 `tool_calls`」的 assistant（OpenAI 兼容 API 不接受该形态）。
fn pop_trailing_assistants_with_neither_content_nor_tool_calls(out: &mut Vec<Message>) {
    while out.last().is_some_and(|m| {
        is_assistant_role(&m.role)
            && !assistant_has_non_empty_tool_calls(m)
            && m.content
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
    }) {
        out.pop();
    }
}

/// Web `/chat`、队列任务与 CLI 单次问答共用的首轮：`[system, user]`。
pub fn messages_chat_seed(system_prompt: &str, user_text: &str) -> Vec<Message> {
    vec![
        Message::system_only(system_prompt.to_string()),
        Message::user_only(user_text.to_string()),
    ]
}

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
#[must_use]
pub fn sanitize_tool_call_arguments_for_openai_compat(arguments: &str) -> String {
    let t = arguments.trim();
    if t.is_empty() {
        return "{}".to_string();
    }
    serde_json::from_str::<serde_json::Value>(t)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "{}".to_string())
}

/// 工具定义（传给 API）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub typ: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// 请求体
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// OpenAI 兼容 **`seed`**；`None` 则 JSON 省略该字段（由供应商默认随机性决定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// MiniMax OpenAI 兼容扩展：为 `true` 时流式/非流式可将思维链与正文分离（`delta.reasoning_details` / `message.reasoning_details`）。
    #[serde(skip_serializing_if = "Option::is_none", rename = "reasoning_split")]
    pub reasoning_split: Option<bool>,
}

// ---------- 非流式响应（`stream: false` 时 chat/completions 返回体） ----------

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: Message,
    #[serde(default)]
    pub finish_reason: String,
}

// ---------- 流式 chunk ----------

/// 流式 chunk 中的 delta（OpenAI 兼容）
#[derive(Debug, Default, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    /// 推理模型流式思维链（如 DeepSeek reasoner）。
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// MiniMax 在 **`reasoning_split: true`** 时流式返回；元素常为 `{"text": "…"}`，`text` 多为**累积**全文。
    #[serde(default)]
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    #[allow(dead_code)]
    pub role: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub struct StreamToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub typ: Option<String>,
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    /// 部分 OpenAI 兼容流会省略 `index`；缺省按 0 处理，避免整段 SSE 解析失败导致无输出。
    #[serde(default)]
    #[allow(dead_code)]
    pub index: u32,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Option<Vec<StreamChoice>>,
}

// TUI 中用于“人工审批”的决策结果：拒绝/允许一次/永久允许。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandApprovalDecision {
    Deny,
    AllowOnce,
    AllowAlways,
}

/// 流式输出被用户中止时 `stream_chat` 返回的 `finish_reason` 占位（非上游 API 原义）。
pub const USER_CANCELLED_FINISH_REASON: &str = "user_cancelled";

/// `complete_chat_retrying` 在用户取消时返回的错误消息（与 `run_agent_turn_common` 识别一致）。
pub const LLM_CANCELLED_ERROR: &str = "已取消";

/// `/chat/stream` 任务被取消且 SSE 仍可投递时，控制面 `SsePayload::Error` 的 **`code`**（与 `docs/SSE_PROTOCOL.md` 一致）。
pub const SSE_STREAM_CANCELLED_CODE: &str = "STREAM_CANCELLED";

#[cfg(test)]
mod api_messages_strip_tests {
    use super::*;

    #[test]
    fn skip_ui_separator_and_strip_reasoning_one_pass() {
        let sep = Message::chat_ui_separator(true);
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some("body".to_string()),
            reasoning_content: Some("chain".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::user_only("u"), sep, assistant];
        let out = messages_for_api_stripping_reasoning_skip_ui_separators(&v);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(out[1].content.as_deref(), Some("body"));
        assert!(out[1].reasoning_content.is_none());
    }

    #[test]
    fn strip_reasoning_only_matches_composing_without_separators() {
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some("x".to_string()),
            reasoning_content: Some("r".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::user_only("u"), assistant];
        let a = messages_stripping_reasoning_for_api_request(&v);
        let b = messages_for_api_stripping_reasoning_skip_ui_separators(&v);
        assert_eq!(a, b);
    }
}

#[cfg(test)]
mod normalize_messages_tests {
    use super::*;

    fn asst(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn asst_with_tc(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: "tc1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "noop".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn merges_adjacent_assistant_placeholder_after_prior_assistant() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("prior"),
            asst(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        assert_eq!(n[2].content.as_deref(), Some("prior"));
    }

    #[test]
    fn drops_trailing_empty_assistant() {
        let v = vec![Message::system_only("s"), Message::user_only("u"), asst("")];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].role, "user");
    }

    #[test]
    fn merges_streaming_partial_then_full_assistant() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("hel"),
            asst("hello"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].content.as_deref(), Some("hello"));
    }

    /// 裁剪掉 tool 后常见：带 tool_calls 的 assistant 紧挨下一条助手正文。
    #[test]
    fn strips_orphan_tool_calls_when_followed_by_assistant_reply() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc("calling tool"),
            asst("final answer"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        assert!(n[2].tool_calls.is_none());
        assert!(n[2].content.as_deref().unwrap().contains("calling tool"));
        assert!(n[2].content.as_deref().unwrap().contains("final answer"));
    }

    #[test]
    fn strips_tool_calls_when_followed_by_empty_assistant_only() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc("x"),
            asst(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].content.as_deref(), Some("x"));
        assert!(n[2].tool_calls.is_none());
    }

    /// 正文助手 + 带 tool_calls 的助手后仍有 tool 消息：必须保留 tool_calls（不得被末尾孤儿清理误伤）。
    #[test]
    fn preserves_merged_tool_calls_when_tool_follows() {
        let tool = Message {
            role: "tool".to_string(),
            content: Some(r#"{"ok":true}"#.to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("tc1".to_string()),
        };
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("reasoning"),
            asst_with_tc(""),
            tool,
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 4);
        assert_eq!(n[2].role, "assistant");
        assert!(n[2].tool_calls.as_ref().is_some_and(|c| !c.is_empty()));
        assert_eq!(n[3].role, "tool");
    }

    #[test]
    fn collapses_three_consecutive_assistants() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("a"),
            asst("b"),
            asst("c"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        let c = n[2].content.as_deref().unwrap();
        assert!(c.contains('a') && c.contains('c'));
    }

    #[test]
    fn merges_when_assistant_role_has_whitespace() {
        let mut odd = asst("x");
        odd.role = " Assistant ".to_string();
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            odd,
            asst("y"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
    }

    /// 前一条仅正文、后一条带 tool_calls：合并为一条；末尾无 `tool` 时孤儿 `tool_calls` 再被清掉（否则仍非法）。
    #[test]
    fn merges_assistant_then_assistant_with_tool_calls() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("partial"),
            asst_with_tc(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert!(n[2].tool_calls.is_none());
        assert!(n[2].content.as_deref().unwrap().contains("partial"));
    }

    /// 末尾仅 `tool_calls`、正文为空且无后续 `tool`：清空 `tool_calls` 后须整条删除，避免 API 400。
    #[test]
    fn drops_trailing_assistant_when_orphan_tool_calls_cleared_and_content_empty() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].role, "user");
    }
}

#[cfg(test)]
mod fold_system_messages_tests {
    use super::*;

    #[test]
    fn merges_system_into_following_user() {
        let v = vec![Message::system_only("sys"), Message::user_only("hi")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].role, "user");
        assert_eq!(o[0].content.as_deref(), Some("sys\n\nhi"));
    }

    #[test]
    fn joins_multiple_system_blocks() {
        let v = vec![
            Message::system_only("a"),
            Message::system_only("b"),
            Message::user_only("u"),
        ];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].content.as_deref(), Some("a\n\nb\n\nu"));
    }

    #[test]
    fn system_before_assistant_inserts_user_carrier() {
        let a = Message {
            role: "assistant".to_string(),
            content: Some("reply".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::system_only("instr"), a];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].role, "user");
        assert_eq!(o[0].content.as_deref(), Some("instr"));
        assert_eq!(o[1].role, "assistant");
    }

    #[test]
    fn trailing_system_only_becomes_user() {
        let v = vec![Message::system_only("orphan")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].role, "user");
        assert_eq!(o[0].content.as_deref(), Some("orphan"));
    }

    #[test]
    fn trims_system_role_case_and_whitespace() {
        let mut s = Message::system_only("x");
        s.role = " SYSTEM ".to_string();
        let v = vec![s, Message::user_only("y")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert!(o[0].content.as_deref().unwrap().starts_with("x"));
    }
}

#[cfg(test)]
mod sanitize_tool_call_arguments_tests {
    use super::sanitize_tool_call_arguments_for_openai_compat;

    #[test]
    fn empty_and_whitespace_become_empty_object() {
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat(""), "{}");
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat("   "), "{}");
    }

    #[test]
    fn valid_json_round_trips_compact() {
        assert_eq!(
            sanitize_tool_call_arguments_for_openai_compat(r#"{"path":"a"}"#),
            r#"{"path":"a"}"#
        );
    }

    #[test]
    fn invalid_json_becomes_empty_object() {
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat("{"), "{}");
        assert_eq!(
            sanitize_tool_call_arguments_for_openai_compat("not json"),
            "{}"
        );
    }
}
