use serde::{Deserialize, Serialize};

const SESSIONS_KEY: &str = "agent-demo-sessions-v1";
const ACTIVE_ID_KEY: &str = "agent-demo-active-session-id";

/// 新建会话默认标题（**存储用**，与语言无关）；界面展示用 [`crate::i18n::session_title_for_display`]。
pub const DEFAULT_CHAT_SESSION_TITLE: &str = "New chat";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default)]
    pub is_tool: bool,
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
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionsFile {
    sessions: Vec<ChatSession>,
}

pub fn window_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn load_sessions() -> (Vec<ChatSession>, Option<String>) {
    let Some(st) = window_storage() else {
        return (Vec::new(), None);
    };
    let active = st.get_item(ACTIVE_ID_KEY).ok().flatten();
    let raw = match st.get_item(SESSIONS_KEY).ok().flatten() {
        Some(r) => r,
        None => return (Vec::new(), active),
    };
    let parsed: SessionsFile = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), active),
    };
    (parsed.sessions, active)
}

pub fn save_sessions(sessions: &[ChatSession], active_id: Option<&str>) {
    let Some(st) = window_storage() else {
        return;
    };
    let file = SessionsFile {
        sessions: sessions.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = st.set_item(SESSIONS_KEY, &json);
    }
    match active_id {
        Some(id) if !id.is_empty() => {
            let _ = st.set_item(ACTIVE_ID_KEY, id);
        }
        _ => {
            let _ = st.remove_item(ACTIVE_ID_KEY);
        }
    }
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
    };
    let id = s.id.clone();
    sessions.push(s);
    (sessions, id)
}
