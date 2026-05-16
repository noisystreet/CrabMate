use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 未设置工作区（或尚未从 `GET /workspace` 得到根路径）时使用的会话 JSON 键（与旧版一致）。
pub const SESSIONS_KEY_LEGACY: &str = "agent-demo-sessions-v1";
/// 与 [`SESSIONS_KEY_LEGACY`] 配对的当前活动会话 id 键。
pub const ACTIVE_ID_KEY_LEGACY: &str = "agent-demo-active-session-id";

/// 新建会话默认标题（**存储用**，与语言无关）；界面展示用 [`crate::i18n::session_title_for_display`]。
pub const DEFAULT_CHAT_SESSION_TITLE: &str = "New chat";

/// `StoredMessageState::TimelineUiJson` 内嵌 JSON 的判别键 `k`（时间线侧栏；与旧版字符串协议一致）。
pub const TIMELINE_UI_STATE_KEY: &str = "cm_tl";

/// 本地会话消息 UI / 流式协议状态（原 `Option<String>`，现枚举化；JSON 仍存为同一字符串）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredMessageState {
    Loading,
    Error,
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
}

impl ChatSession {
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
/// 在从 `localStorage` 恢复会话列表时调用（见 `wire_initial_sessions_from_storage`）。
pub fn clear_stale_assistant_loading_states(messages: &mut [StoredMessage]) {
    for m in messages.iter_mut() {
        if m.role == "assistant" && m.state.as_ref().is_some_and(|s| s.is_loading()) {
            m.state = None;
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionsFile {
    sessions: Vec<ChatSession>,
}

pub fn window_storage() -> Option<web_sys::Storage> {
    crate::app_prefs::local_storage()
}

/// 与 `GET /workspace` 返回的 `path` 对齐的规范化字符串，用于派生存储桶键（仅本地，不上传）。
#[must_use]
pub fn normalize_workspace_partition_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_string()
}

/// 给定工作区根路径（空表示未设置），返回本会话列表在 `localStorage` 中的 JSON 键。
#[must_use]
pub fn sessions_json_storage_key(workspace_root: &str) -> String {
    let n = normalize_workspace_partition_path(workspace_root);
    if n.is_empty() {
        return SESSIONS_KEY_LEGACY.to_string();
    }
    let digest = Sha256::digest(n.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{SESSIONS_KEY_LEGACY}::ws::{hex}")
}

#[must_use]
pub fn active_id_storage_key_for_sessions_json(sessions_json_key: &str) -> String {
    if sessions_json_key == SESSIONS_KEY_LEGACY {
        ACTIVE_ID_KEY_LEGACY.to_string()
    } else {
        format!("{ACTIVE_ID_KEY_LEGACY}##{sessions_json_key}")
    }
}

/// 从指定存储桶读取会话列表与活动 id。
pub fn load_sessions_at_storage_key(sessions_json_key: &str) -> (Vec<ChatSession>, Option<String>) {
    let Some(st) = window_storage() else {
        return (Vec::new(), None);
    };
    let active_key = active_id_storage_key_for_sessions_json(sessions_json_key);
    let active = st.get_item(&active_key).ok().flatten();
    let raw = match st.get_item(sessions_json_key).ok().flatten() {
        Some(r) => r,
        None => return (Vec::new(), active),
    };
    let parsed: SessionsFile = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), active),
    };
    (parsed.sessions, active)
}

/// 将会话列表与活动 id 写入指定存储桶。
pub fn save_sessions_at_storage_key(
    sessions_json_key: &str,
    sessions: &[ChatSession],
    active_id: Option<&str>,
) {
    let Some(st) = window_storage() else {
        return;
    };
    let file = SessionsFile {
        sessions: sessions.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = st.set_item(sessions_json_key, &json);
    }
    let active_key = active_id_storage_key_for_sessions_json(sessions_json_key);
    match active_id {
        Some(id) if !id.is_empty() => {
            let _ = st.set_item(&active_key, id);
        }
        _ => {
            let _ = st.remove_item(&active_key);
        }
    }
}

/// 与旧版一致：读写未分桶的默认键（未设置工作区时）。
pub fn load_sessions() -> (Vec<ChatSession>, Option<String>) {
    load_sessions_at_storage_key(SESSIONS_KEY_LEGACY)
}

/// 与旧版一致：写入未分桶的默认键（未设置工作区时）；新逻辑使用 [`save_sessions_at_storage_key`]。
#[allow(dead_code)]
pub fn save_sessions(sessions: &[ChatSession], active_id: Option<&str>) {
    save_sessions_at_storage_key(SESSIONS_KEY_LEGACY, sessions, active_id);
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
        title: default_title,
        draft: String::new(),
        messages: Vec::new(),
        updated_at: now,
        pinned: false,
        starred: false,
        server_conversation_id: None,
        server_revision: None,
        workspace_root: None,
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
    fn sessions_json_storage_key_empty_is_legacy() {
        assert_eq!(
            sessions_json_storage_key(""),
            SESSIONS_KEY_LEGACY.to_string()
        );
        assert_eq!(
            sessions_json_storage_key("   "),
            SESSIONS_KEY_LEGACY.to_string()
        );
    }

    #[test]
    fn sessions_json_storage_key_stable_for_path() {
        let a = sessions_json_storage_key("/home/foo/proj");
        let b = sessions_json_storage_key("/home/foo/proj/");
        assert_eq!(a, b);
        assert!(a.starts_with(SESSIONS_KEY_LEGACY));
        assert_ne!(a, SESSIONS_KEY_LEGACY);
    }
}
