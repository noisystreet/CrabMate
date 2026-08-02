//! 浏览器内导出会话：schema / 展示投影信封来自 [`crabmate_chat_export`]（`projection=display`）；
//! 消息正文为展示过滤结果；下载壳为浏览器 / Tauri。
//!
//! 与 CLI/TUI **`projection=raw`** 的完整 [`crabmate_chat_export::ChatSessionFile`] 区分：
//! Web JSON **不可**直接作为 `tool-replay` 输入。

use crabmate_chat_export::{
    DisplayChatSessionFile, DisplayExportMessage, ExportMdLocale, markdown_from_role_bodies,
};
use gloo_timers::callback::Timeout;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::i18n::Locale;
use crate::message_format::{message_text_for_display_ex, stored_tool_message_detail_text};
use crate::storage::{ChatSession, StoredMessage};
use crate::visible_messages::{VisibleMessageScope, visible_message_indices};

pub use crabmate_chat_export::display_session_to_json_pretty;

#[cfg(test)]
use crabmate_chat_export::{
    CHAT_EXPORT_PROJECTION_DISPLAY, CHAT_EXPORT_SCHEMA_ID, CHAT_EXPORT_SCHEMA_VERSION,
    CHAT_SESSION_FILE_VERSION,
};

#[wasm_bindgen(inline_js = r#"
export function invokeTauriSaveTextFile(defaultName, body) {
  const invoke =
    (globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke) ||
    (globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke);
  if (typeof invoke !== "function") {
    throw new Error("Tauri invoke unavailable");
  }
  return invoke("save_text_file_via_dialog", {
    default_name: defaultName,
    defaultName,
    content: body
  });
}

export function invokeTauriPickWorkspaceFolder() {
  const invoke =
    (globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke) ||
    (globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke);
  if (typeof invoke !== "function") {
    throw new Error("Tauri invoke unavailable");
  }
  return invoke("pick_workspace_folder_via_dialog", {});
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = invokeTauriSaveTextFile)]
    fn invoke_tauri_save_text_file(default_name: &str, body: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriPickWorkspaceFolder)]
    fn invoke_tauri_pick_workspace_folder() -> js_sys::Promise;
}

/// 打开系统文件夹对话框；取消返回 `Ok(None)`。
pub(crate) async fn tauri_pick_workspace_folder() -> Result<Option<String>, String> {
    let promise = invoke_tauri_pick_workspace_folder();
    match JsFuture::from(promise).await {
        Ok(v) => {
            if v.is_null() || v.is_undefined() {
                return Ok(None);
            }
            v.as_string()
                .map(Some)
                .ok_or_else(|| "unexpected folder picker result".to_string())
        }
        Err(e) => Err(format!("{e:?}")),
    }
}

fn export_md_locale(loc: Locale) -> ExportMdLocale {
    match loc {
        Locale::ZhHans => ExportMdLocale::ZhHans,
        Locale::En => ExportMdLocale::En,
    }
}

pub fn session_to_export_file(
    session: &ChatSession,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> DisplayChatSessionFile {
    DisplayChatSessionFile::new(stored_messages_to_export(
        &session.messages,
        loc,
        apply_assistant_display_filters,
    ))
}

fn stored_messages_to_export(
    messages: &[StoredMessage],
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> Vec<DisplayExportMessage> {
    let indices = visible_message_indices(messages, VisibleMessageScope::Export);
    let mut out = Vec::new();
    for &idx in &indices {
        let m = &messages[idx];
        if m.role == "system" && m.is_tool {
            out.push(DisplayExportMessage {
                role: "tool".to_string(),
                content: Some(message_text_for_export(
                    m,
                    loc,
                    apply_assistant_display_filters,
                )),
                name: Some("tool".to_string()),
            });
            continue;
        }
        if m.role == "system" {
            continue;
        }
        out.push(DisplayExportMessage {
            role: m.role.clone(),
            content: Some(message_text_for_export(
                m,
                loc,
                apply_assistant_display_filters,
            )),
            name: None,
        });
    }
    out
}

fn message_text_for_export(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    let body = if m.is_tool {
        stored_tool_message_detail_text(m, loc)
    } else {
        message_text_for_display_ex(m, loc, apply_assistant_display_filters)
    };
    // 兼容旧会话：仅有“(无回复)”占位时，导出追加最小诊断摘要，便于离线排查。
    if m.role == "assistant"
        && (body.trim() == crate::i18n::stream_empty_reply(Locale::ZhHans)
            || body.trim() == crate::i18n::stream_empty_reply(Locale::En))
    {
        return format!(
            "{body}\n\n{}",
            crate::i18n::stream_empty_reply_diag_line(loc, None, false, 0)
        );
    }
    body
}

/// session.messages 顺序由 TurnLayout 实时保证（assistant → tools → next_assistant），不需要事后重排。
fn markdown_from_export_messages(
    title: &str,
    messages: &[DisplayExportMessage],
    loc: Locale,
) -> String {
    let pairs: Vec<(&str, &str)> = messages
        .iter()
        .map(|m| (m.role.as_str(), m.content.as_deref().unwrap_or("")))
        .collect();
    markdown_from_role_bodies(title, pairs, export_md_locale(loc))
}

/// 与 CLI `messages_to_markdown` 一致：跳过 `system`；`tool` 与 `user`/`assistant` 分段。
pub fn session_to_markdown(
    session: &ChatSession,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    let messages =
        stored_messages_to_export(&session.messages, loc, apply_assistant_display_filters);
    markdown_from_export_messages(
        crabmate_chat_export::export_md_title_full(export_md_locale(loc)),
        &messages,
        loc,
    )
}

/// 按会话内顺序导出**已选 id** 对应的消息（与全会话 Markdown 规则相同；未选中的 id 忽略）。
/// 多选导出 UI 已移除；保留给单元测试覆盖「按 id 子集」语义。
#[cfg(test)]
pub fn stored_messages_by_ids_to_markdown(
    all_messages: &[StoredMessage],
    selected_ids: &[String],
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    use std::collections::HashSet;

    let set: HashSet<&str> = selected_ids.iter().map(|s| s.as_str()).collect();
    let subset: Vec<StoredMessage> = all_messages
        .iter()
        .filter(|m| set.contains(m.id.as_str()))
        .cloned()
        .collect();
    let messages = stored_messages_to_export(&subset, loc, apply_assistant_display_filters);
    markdown_from_export_messages(
        crabmate_chat_export::export_md_title_selection(export_md_locale(loc)),
        &messages,
        loc,
    )
}

pub fn export_filename_stem(prefix: &str) -> String {
    let now = js_sys::Date::new_0();
    let y = now.get_full_year() as i32;
    let mo = now.get_month() + 1;
    let d = now.get_date();
    let h = now.get_hours();
    let mi = now.get_minutes();
    let s = now.get_seconds();
    format!(
        "{}_{:04}{:02}{:02}_{:02}{:02}{:02}",
        prefix, y, mo, d, h, mi, s
    )
}

/// 触发浏览器下载 UTF-8 文本；失败时返回说明字符串。
pub fn trigger_download(
    filename: &str,
    mime: &str,
    body: &str,
    loc: crate::i18n::Locale,
) -> Result<(), String> {
    if crate::tauri_shell::tauri_shell_available() {
        return trigger_download_via_tauri(filename, body, loc);
    }
    trigger_download_via_anchor(filename, mime, body)
}

fn trigger_download_via_tauri(
    filename: &str,
    body: &str,
    loc: crate::i18n::Locale,
) -> Result<(), String> {
    let default_name = filename.to_string();
    let content = body.to_string();
    let Some(w) = web_sys::window() else {
        return Err("no window".to_string());
    };
    spawn_local(async move {
        let p = invoke_tauri_save_text_file(&default_name, &content);
        match JsFuture::from(p).await {
            Ok(v) => {
                let cancelled = v.as_bool().is_some_and(|saved| !saved);
                if cancelled {
                    let _ =
                        w.alert_with_message(crate::i18n::export_tauri_save_cancelled_alert(loc));
                }
            }
            Err(e) => {
                let msg = crate::i18n::export_tauri_save_failed_alert(loc, &format!("{e:?}"));
                let _ = w.alert_with_message(&msg);
            }
        }
    });
    Ok(())
}

fn document_body_pair() -> Result<(web_sys::Document, web_sys::HtmlElement), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let document = window.document().ok_or_else(|| "no document".to_string())?;
    let body_el = document.body().ok_or_else(|| "no body".to_string())?;
    Ok((document, body_el))
}

fn create_download_anchor(
    document: &web_sys::Document,
    filename: &str,
) -> Result<web_sys::HtmlAnchorElement, String> {
    let a = document
        .create_element("a")
        .map_err(|e| format!("create a: {:?}", e))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "a element".to_string())?;
    a.set_download(filename);
    a.set_attribute("rel", "noopener")
        .map_err(|e| format!("rel: {:?}", e))?;
    a.style().set_property("display", "none").ok();
    Ok(a)
}

fn click_temp_anchor(
    body_el: &web_sys::HtmlElement,
    a: &web_sys::HtmlAnchorElement,
) -> Result<(), String> {
    body_el
        .append_child(a)
        .map_err(|e| format!("append: {:?}", e))?;
    a.click();
    body_el.remove_child(a).ok();
    Ok(())
}

fn anchor_download_with_href(
    document: &web_sys::Document,
    body_el: &web_sys::HtmlElement,
    filename: &str,
    href: &str,
) -> Result<(), String> {
    let a = create_download_anchor(document, filename)?;
    a.set_href(href);
    click_temp_anchor(body_el, &a)
}

fn utf8_blob_for_download(body: &str, mime: &str) -> Result<web_sys::Blob, String> {
    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(body));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)
        .map_err(|e| format!("Blob: {:?}", e))
}

fn schedule_revoke_object_url(blob_url: &str) {
    let url_clone = blob_url.to_string();
    Timeout::new(0, move || {
        let _ = web_sys::Url::revoke_object_url(&url_clone);
    })
    .forget();
}

fn trigger_download_via_anchor(filename: &str, mime: &str, body: &str) -> Result<(), String> {
    let (document, body_el) = document_body_pair()?;

    // 首选 Blob URL（体积更稳，支持更大文件）；若 WebView 下载策略不接受，再回退 data URL。
    let blob = utf8_blob_for_download(body, mime)?;
    let blob_url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("object URL: {:?}", e))?;

    let blob_attempt = anchor_download_with_href(&document, &body_el, filename, &blob_url);
    schedule_revoke_object_url(&blob_url);

    if blob_attempt.is_ok() {
        return Ok(());
    }

    let data_uri = format!(
        "data:{};charset=utf-8,{}",
        mime,
        js_sys::encode_uri_component(body)
    );
    let data_attempt = anchor_download_with_href(&document, &body_el, filename, &data_uri);

    if data_attempt.is_ok() {
        return Ok(());
    }

    Err(format!(
        "download failed: blob={:?}, data={:?}",
        blob_attempt.err(),
        data_attempt.err()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessage;

    fn msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn by_ids_keeps_session_order_and_omits_unselected() {
        let session = ChatSession {
            id: "s1".to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("a", "user", "first", false),
                msg("b", "assistant", "second", false),
                msg("c", "user", "third", false),
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let md = stored_messages_by_ids_to_markdown(
            &session.messages,
            &["c".into(), "a".into()],
            Locale::ZhHans,
            true,
        );
        assert!(md.contains("first"));
        assert!(!md.contains("second"));
        assert!(md.contains("third"));
        let pos_first = md.find("first").unwrap();
        let pos_third = md.find("third").unwrap();
        assert!(
            pos_first < pos_third,
            "export should follow session order, not selection order"
        );
    }

    #[test]
    fn skips_plain_system_keeps_tool_cards_as_tool_role() {
        let session = ChatSession {
            id: "s1".to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("1", "user", "hi", false),
                msg("2", "system", "hidden", false),
                msg("3", "system", "tool out", true),
                msg("4", "assistant", "ok", false),
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let file = session_to_export_file(&session, Locale::ZhHans, true);
        assert_eq!(file.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(file.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(file.projection, CHAT_EXPORT_PROJECTION_DISPLAY);
        assert_eq!(file.version, CHAT_SESSION_FILE_VERSION);
        assert_eq!(file.messages.len(), 3);
        assert_eq!(file.messages[0].role, "user");
        assert_eq!(file.messages[1].role, "tool");
        assert_eq!(file.messages[1].name.as_deref(), Some("tool"));
        assert_eq!(file.messages[2].role, "assistant");
    }

    #[test]
    fn export_skips_legacy_duplicate_local_snapshot_assistant() {
        let session = ChatSession {
            id: "s1".to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("1", "user", "hi", false),
                msg("2", "assistant", "canonical answer", false),
                StoredMessage {
                    id: "dup".to_string(),
                    role: "assistant".to_string(),
                    text: "canonical answer".to_string(),
                    reasoning_text: String::new(),
                    image_urls: vec![],
                    state: Some(crate::timeline_scan::timeline_state_local_snapshot()),
                    is_tool: false,
                    tool_call_id: None,
                    tool_name: None,
                    created_at: 2,
                },
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let md = session_to_markdown(&session, Locale::ZhHans, true);
        assert_eq!(md.matches("canonical answer").count(), 1);
    }

    #[test]
    fn export_tool_message_prefers_reasoning_full_card_over_compact_text() {
        let mut tool_msg = msg("3", "system", "读取目录 . 0 项", true);
        tool_msg.reasoning_text =
            "读取目录完成\n\n目录：foo\n\n总计遍历：1，展示：1\nfile: a.txt\n".to_string();
        let session = ChatSession {
            id: "s1".to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![msg("1", "user", "hi", false), tool_msg],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let md = session_to_markdown(&session, Locale::ZhHans, true);
        assert!(md.contains("总计遍历：1"), "md={md}");
        assert!(
            md.contains("file: a.txt"),
            "expected raw listing lines from tool_card_text: {md}"
        );
    }

    #[test]
    fn export_includes_both_fuzzy_duplicate_assistant_rows() {
        let listing = "当前目录下有三个压缩包：\n\n1. **A** — x\n\n2. **B** — y";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x\n2. **B** — y";
        let session = ChatSession {
            id: "s1".to_string(),
            layout_schema_version: crate::storage::CURRENT_LAYOUT_SCHEMA_VERSION,
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("u", "user", "分析", false),
                msg("a1", "assistant", listing, false),
                msg("a2", "assistant", compact, false),
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let md = session_to_markdown(&session, Locale::ZhHans, true);
        assert_eq!(md.matches("## 助手").count(), 2, "md={md}");
    }
}
