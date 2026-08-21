//! 出站前把用户消息里的 **`@` / `file:///`** 栅格图按 `read_file` 工作区策略读盘，打成 `image_url`。

use std::io::Read;
use std::path::Path;

use crate::cm_internal::user_message_file_refs::{
    collect_ordered_rel_paths_lenient, is_workspace_chat_image_rel,
};
use crate::cm_llm::outbound_images::{
    bytes_to_data_url, omit_note, InlineFail, FLATTEN_PLACEHOLDER,
};
use crate::cm_tools::tools::resolve_for_read_open;
use crate::cm_types::{Message, MessageContent};

pub(super) fn attach_workspace_image_refs(
    msg: &mut Message,
    allow_vision: bool,
    workspace_root: Option<&Path>,
    budget: &mut u64,
) {
    if msg.role != "user" {
        return;
    }
    let rels = workspace_image_rels_in_message(msg);
    if rels.is_empty() {
        return;
    }
    if !allow_vision {
        // 已有 `image_url` 时由 flatten 写一次说明，避免与 `@` 图重复。
        if !message_has_image_url_parts(msg) {
            append_text_note(msg, FLATTEN_PLACEHOLDER);
        }
        return;
    }
    let Some(root) = workspace_root else {
        append_text_note(msg, "（未设置工作区，已省略工作区图片引用。）");
        return;
    };
    let extra = rels
        .into_iter()
        .map(|rel| match read_workspace_image_data_url(root, &rel, budget) {
            Ok(data_url) => serde_json::json!({
                "type": "image_url",
                "image_url": {"url": data_url}
            }),
            Err(fail) => serde_json::json!({
                "type": "text",
                "text": omit_note(fail, &format!("@{rel}"))
            }),
        })
        .collect::<Vec<_>>();
    append_content_parts(msg, extra);
}

fn message_has_image_url_parts(msg: &Message) -> bool {
    let Some(MessageContent::Parts(parts)) = msg.content.as_ref() else {
        return false;
    };
    parts.iter().any(|p| p.get("type").and_then(|v| v.as_str()) == Some("image_url"))
}

fn workspace_image_rels_in_message(msg: &Message) -> Vec<String> {
    let mut texts = Vec::new();
    match msg.content.as_ref() {
        Some(MessageContent::Text(t)) => texts.push(t.as_str()),
        Some(MessageContent::Parts(parts)) => {
            for p in parts {
                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                    texts.push(t);
                }
            }
        }
        None => {}
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in texts {
        for rel in collect_ordered_rel_paths_lenient(t) {
            if is_workspace_chat_image_rel(&rel) && seen.insert(rel.clone()) {
                out.push(rel);
            }
        }
    }
    out
}

fn read_workspace_image_data_url(
    root: &Path,
    rel: &str,
    budget: &mut u64,
) -> Result<String, InlineFail> {
    let opened = resolve_for_read_open(root, rel).map_err(|_| InlineFail::Read)?;
    if !opened.metadata.is_file() {
        return Err(InlineFail::Read);
    }
    let len = opened.metadata.len();
    if len == 0 {
        return Err(InlineFail::Empty);
    }
    if len > *budget {
        return Err(InlineFail::TooLarge);
    }
    let mut file = opened.file;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| InlineFail::Read)?;
    if bytes.len() as u64 != len {
        return Err(InlineFail::Read);
    }
    bytes_to_data_url(&bytes, budget)
}

fn append_text_note(msg: &mut Message, note: &str) {
    append_content_parts(
        msg,
        vec![serde_json::json!({"type": "text", "text": note})],
    );
}

fn append_content_parts(msg: &mut Message, extra: Vec<serde_json::Value>) {
    if extra.is_empty() {
        return;
    }
    match msg.content.take() {
        Some(MessageContent::Parts(mut parts)) => {
            parts.extend(extra);
            msg.content = Some(MessageContent::Parts(parts));
        }
        Some(MessageContent::Text(t)) => {
            let mut parts = Vec::new();
            if !t.trim().is_empty() {
                parts.push(serde_json::json!({"type": "text", "text": t}));
            }
            parts.extend(extra);
            if parts.is_empty() {
                msg.content = Some(MessageContent::Text(FLATTEN_PLACEHOLDER.to_string()));
            } else {
                msg.content = Some(MessageContent::Parts(parts));
            }
        }
        None => {
            msg.content = Some(MessageContent::Parts(extra));
        }
    }
}
