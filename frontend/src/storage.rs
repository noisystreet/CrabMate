use serde::{Deserialize, Serialize};

/// 新建会话默认标题（**存储用**，与语言无关）；界面展示用 [`crate::i18n::session_title_for_display`]。
pub const DEFAULT_CHAT_SESSION_TITLE: &str = "New chat";
/// 旧会话未记录布局版本时使用的兼容版本。
pub const LEGACY_LAYOUT_SCHEMA_VERSION: u32 = 1;
/// 当前 Web 流式投影布局版本：closed commentary 按稳定行分别持久化。
pub const CURRENT_LAYOUT_SCHEMA_VERSION: u32 = 2;
/// v2 closed commentary 的持久化消息 ID 前缀。
pub const V2_COMMENTARY_ROW_ID_PREFIX: &str = "turn-commentary-";
/// v2 终答的持久化消息 ID。
pub const V2_FINAL_ANSWER_ROW_ID: &str = "turn-final-answer";

const fn default_layout_schema_version() -> u32 {
    LEGACY_LAYOUT_SCHEMA_VERSION
}

/// `StoredMessageState::TimelineUiJson` 内嵌 JSON 的判别键 `k`（时间线侧栏；与旧版字符串协议一致）。
pub const TIMELINE_UI_STATE_KEY: &str = "cm_tl";

/// 本地会话消息 UI / 流式协议状态（原 `Option<String>`，现枚举化；JSON 仍存为同一字符串）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredMessageState {
    Loading,
    Error,
    /// 同一模型轮次在 `tool_calls` 之前流出的正文旁注（不进主气泡/导出）。
    CommentaryBeforeTools,
    /// `hierarchical-subgoal:…` 完整标记。
    HierarchicalSubgoal(String),
    /// 侧栏时间线：`k` 为 [`TIMELINE_UI_STATE_KEY`] 的 JSON。
    TimelineUiJson(String),
    /// 未能归入已知变体的字符串（兼容往返）。
    Opaque(String),
}

impl StoredMessageState {
    pub fn from_wire(s: String) -> Self {
        match s.as_str() {
            "loading" => return Self::Loading,
            "error" => return Self::Error,
            "commentary_before_tools" => return Self::CommentaryBeforeTools,
            _ => {}
        }
        if s.starts_with("hierarchical-subgoal:") {
            return Self::HierarchicalSubgoal(s);
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
            && v.get("k").and_then(|x| x.as_str()) == Some(TIMELINE_UI_STATE_KEY)
        {
            return Self::TimelineUiJson(s);
        }
        Self::Opaque(s)
    }

    pub fn to_wire(&self) -> String {
        match self {
            Self::Loading => "loading".to_string(),
            Self::Error => "error".to_string(),
            Self::CommentaryBeforeTools => "commentary_before_tools".to_string(),
            Self::HierarchicalSubgoal(s) | Self::TimelineUiJson(s) | Self::Opaque(s) => s.clone(),
        }
    }

    #[inline]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    #[inline]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    pub fn matches_full_marker(&self, marker: &str) -> bool {
        match self {
            Self::HierarchicalSubgoal(s) | Self::Opaque(s) => s == marker,
            _ => false,
        }
    }

    pub fn looks_like_hierarchical_subgoal(&self) -> bool {
        match self {
            Self::HierarchicalSubgoal(_) => true,
            Self::Opaque(s) => s.starts_with("hierarchical-subgoal:"),
            _ => false,
        }
    }

    /// 若非空则交给 [`crate::timeline_scan::timeline_entry_for_message`] 内的 JSON 解析（校验 `k`）。
    pub fn as_timeline_parse_candidate(&self) -> Option<&str> {
        match self {
            Self::TimelineUiJson(s) | Self::Opaque(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 服务端快照合并时：本地时间线旁注是否应保留。
    pub fn is_local_timeline_snapshot_row(&self) -> bool {
        match self {
            Self::TimelineUiJson(_) => true,
            Self::Opaque(s) => s.contains(TIMELINE_UI_STATE_KEY),
            _ => false,
        }
    }
}

mod serde_opt_stored_message_state {
    use super::StoredMessageState;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        value: &Option<StoredMessageState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => Option::<String>::None.serialize(serializer),
            Some(st) => Some(st.to_wire()).serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<StoredMessageState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        Ok(opt.map(StoredMessageState::from_wire))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub text: String,
    /// 助手思维链（与 `text` 终答分隔；流式经 `assistant_answer_phase` 后写入 `text`）；旧数据缺省为空。
    #[serde(default)]
    pub reasoning_text: String,
    /// 用户消息附带的图片（`/uploads/...`）；旧数据缺省为空。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "serde_opt_stored_message_state"
    )]
    pub state: Option<StoredMessageState>,
    #[serde(default)]
    pub is_tool: bool,
    /// 与 SSE `tool_call` / `tool_result` 的 `tool_call_id` 对齐；旧数据缺省为无（按 FIFO 配对结果）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// OpenAI function `name`（蛇形）；供工具气泡图标等 UI，**不**拼进可复制正文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// 消息创建时间（毫秒，与 `js_sys::Date::now()` 一致）；旧数据缺省为 0，UI 不显示时钟点。
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    /// Web 消息行投影版本。旧缓存缺省为 v1；新建会话写 v2。
    #[serde(default = "default_layout_schema_version")]
    pub layout_schema_version: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub draft: String,
    #[serde(default)]
    pub messages: Vec<StoredMessage>,
    /// 旧版前端未存该字段时默认为 0。
    #[serde(default)]
    pub updated_at: i64,
    /// 置顶：侧栏排序优先于收藏与普通会话。
    #[serde(default)]
    pub pinned: bool,
    /// 收藏：侧栏排序次于置顶、优于仅按时间。
    #[serde(default)]
    pub starred: bool,
    /// 与服务端 `conversation_id` 对齐（`POST /chat/stream` 响应头或 `GET /conversation/messages`）；无则纯本地会话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_conversation_id: Option<String>,
    /// 最近一次已知的 `conversation_saved.revision` 或服务端 `GET /conversation/messages` 的 revision。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_revision: Option<u64>,
    /// 本会话绑定的 Web 工作区根（与 `POST /workspace` 一致）；切换到此会话时自动应用。旧数据缺省为不绑定。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    /// 服务端过滤后消息总数（分页水合时写入）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_total: Option<u32>,
    /// 当前 `messages[0]` 在服务端过滤数组中的下标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_window_start: Option<u32>,
    /// 是否还有更早消息（`GET /conversation/messages?before_index=`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_has_older: Option<bool>,
}

impl ChatSession {
    /// 是否已有可直接复用的 v2 投影。稳定 key 优先于旧缓存中可能缺失的版本字段。
    #[must_use]
    pub fn has_v2_layout_projection(&self) -> bool {
        self.layout_schema_version >= CURRENT_LAYOUT_SCHEMA_VERSION || self.has_v2_finalized_rows()
    }

    /// 是否已经持久化至少一条可安全复用的 v2 finalized 行。
    #[must_use]
    pub fn has_v2_finalized_rows(&self) -> bool {
        self.messages.iter().any(|m| {
            m.id.starts_with(V2_COMMENTARY_ROW_ID_PREFIX) || m.id == V2_FINAL_ANSWER_ROW_ID
        })
    }

    /// 加载旧缓存后补齐可由稳定 projection key 明确判定的 v2 版本。
    pub fn normalize_layout_schema_version(&mut self) {
        if self.has_v2_finalized_rows() {
            self.layout_schema_version = CURRENT_LAYOUT_SCHEMA_VERSION;
        } else if self.layout_schema_version == 0 {
            self.layout_schema_version = LEGACY_LAYOUT_SCHEMA_VERSION;
        }
    }

    #[must_use]
    pub fn history_has_older_flag(&self) -> bool {
        self.history_has_older.unwrap_or(false)
    }

    /// 非空且 trim 后的 `server_conversation_id`；与 `GET /conversation/messages` 路径参数对齐。
    #[must_use]
    pub fn trimmed_server_conversation_id(&self) -> Option<&str> {
        self.server_conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
    }
}

/// 进程重启后不再有挂起的 SSE；本地持久化的助手 `loading` 占位若不清理会永久显示「生成中」。
/// 在从 `/user-data` 恢复会话列表时调用（见 `wire_initial_sessions_from_storage`）。
pub fn clear_stale_assistant_loading_states(messages: &mut [StoredMessage]) {
    use crate::message_loading::is_loading_plain_assistant;
    for m in messages.iter_mut() {
        if is_loading_plain_assistant(m) {
            m.state = None;
        }
    }
}

/// 从 `/user-data` 恢复会话时：清助手 `Loading`，并收口无 SSE 可配对的僵尸工具「执行中」。
pub fn clear_stale_stream_loading_states(messages: &mut [StoredMessage], loc: crate::i18n::Locale) {
    clear_stale_assistant_loading_states(messages);
    crate::message_loading::finalize_loading_tool_placeholders(
        messages,
        loc,
        crate::message_loading::ToolLoadingFinalizeKind::OrphanStale,
    );
}

/// 与 `GET /workspace` 返回的 `path` 对齐的规范化字符串（与服务端分桶一致）。
#[must_use]
pub fn normalize_workspace_partition_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_string()
}

pub fn make_session_id() -> String {
    format!(
        "s_{}_{}",
        js_sys::Date::now() as i64,
        (js_sys::Math::random() * 1_000_000_000.0) as i64
    )
}

pub fn ensure_at_least_one(
    mut sessions: Vec<ChatSession>,
    default_title: String,
) -> (Vec<ChatSession>, String) {
    if !sessions.is_empty() {
        let id = sessions[0].id.clone();
        return (sessions, id);
    }
    let now = js_sys::Date::now() as i64;
    let s = ChatSession {
        id: make_session_id(),
        layout_schema_version: CURRENT_LAYOUT_SCHEMA_VERSION,
        title: default_title,
        draft: String::new(),
        messages: Vec::new(),
        updated_at: now,
        pinned: false,
        starred: false,
        server_conversation_id: None,
        server_revision: None,
        workspace_root: None,
        history_total: None,
        history_window_start: None,
        history_has_older: None,
    };
    let id = s.id.clone();
    sessions.push(s);
    (sessions, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_preserves_wire_strings() {
        let m = StoredMessage {
            id: "m".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: StoredMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.state, Some(StoredMessageState::Loading));
    }

    #[test]
    fn from_wire_classifies_timeline_json() {
        let raw = r#"{"k":"cm_tl","t":"tool","msg":"x","ok":true}"#.to_string();
        let st = StoredMessageState::from_wire(raw.clone());
        assert!(matches!(st, StoredMessageState::TimelineUiJson(s) if s == raw));
    }

    #[test]
    fn clear_stale_assistant_loading_clears_assistant_only() {
        let mut msgs = vec![
            StoredMessage {
                id: "a".into(),
                role: "assistant".into(),
                text: "partial".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "u".into(),
                role: "user".into(),
                text: "hi".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        clear_stale_assistant_loading_states(&mut msgs);
        assert!(msgs[0].state.is_none());
        assert_eq!(msgs[1].state, Some(StoredMessageState::Loading));
    }

    #[test]
    fn clear_stale_stream_loading_finalizes_orphan_tool() {
        let mut msgs = vec![StoredMessage {
            id: "t".into(),
            role: "system".into(),
            text: "工具：http_fetch".into(),
            reasoning_text: "tool: http_fetch\nstatus: running".into(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: Some("c1".into()),
            tool_name: Some("http_fetch".into()),
            created_at: 0,
        }];
        clear_stale_stream_loading_states(&mut msgs, crate::i18n::Locale::ZhHans);
        assert!(msgs[0].state.is_none());
        assert!(msgs[0].reasoning_text.contains("interrupted (stale)"));
        assert!(msgs[0].text.contains("已中断"));
    }

    #[test]
    fn normalize_workspace_partition_trims_slash() {
        assert_eq!(normalize_workspace_partition_path("/tmp/ws/"), "/tmp/ws");
    }

    #[test]
    fn legacy_session_without_layout_version_defaults_to_v1() {
        let session: ChatSession = serde_json::from_str(r#"{"id":"s","messages":[]}"#)
            .expect("deserialize legacy session");
        assert_eq!(session.layout_schema_version, LEGACY_LAYOUT_SCHEMA_VERSION);
        assert!(!session.has_v2_layout_projection());
    }

    #[test]
    fn stable_commentary_key_upgrades_cache_to_v2() {
        let mut session: ChatSession = serde_json::from_str(
            r#"{"id":"s","messages":[{"id":"turn-commentary-t1","role":"assistant"}]}"#,
        )
        .expect("deserialize pre-versioned v2 session");
        session.normalize_layout_schema_version();
        assert_eq!(session.layout_schema_version, CURRENT_LAYOUT_SCHEMA_VERSION);
        assert!(session.has_v2_layout_projection());
    }
}
