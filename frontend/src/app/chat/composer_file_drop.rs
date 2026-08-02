//! 将工作区文件拖放到 composer：插入 `@{rel}`；亦可落盘图片附件。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::upload_files_multipart;
use crate::i18n::{self, Locale};
use crate::workspace_tree::CRABMATE_WS_REL_MIME;

/// 将绝对 `file://` URI（或本机绝对路径）剥成相对当前工作区根的路径。
#[must_use]
pub(crate) fn workspace_rel_from_abs_path(abs: &str, workspace_root: &str) -> Option<String> {
    let root = workspace_root
        .trim()
        .trim_end_matches('/')
        .replace('\\', "/");
    if root.is_empty() {
        return None;
    }
    let mut path = abs.trim().replace('\\', "/");
    if let Some(rest) = path.strip_prefix("file://") {
        // file:///abs 或 file://localhost/abs
        path = rest
            .strip_prefix("//localhost")
            .or_else(|| rest.strip_prefix("//"))
            .unwrap_or(rest)
            .to_string();
        if let Some(decoded) = percent_decode_path(&path) {
            path = decoded;
        }
    }
    let path = path.trim_end_matches('/');
    if path == root.as_str() {
        return None;
    }
    let prefix = format!("{root}/");
    let rel = path.strip_prefix(prefix.as_str())?;
    if rel.is_empty() || rel.contains("..") {
        return None;
    }
    Some(rel.to_string())
}

fn percent_decode_path(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_string());
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16).ok()?;
            out.push(h);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 仅接受显式 `file:///{rel}` / `@{rel}`（或拖拽 MIME 里的纯相对路径行）；拒绝裸词以免误插。
fn normalize_ws_rel_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let rel = if let Some(rest) = t.strip_prefix("file:///") {
        // file:////abs → 绝对；file:///rel → 相对
        if rest.starts_with('/') {
            return None;
        }
        rest
    } else if let Some(rest) = t.strip_prefix('@') {
        rest
    } else {
        // 自定义 MIME 写入的是纯相对路径；其它 text/plain 裸词不接纳。
        t
    };
    let rel = rel.trim().trim_start_matches('/').replace('\\', "/");
    if rel.is_empty() || rel.contains("..") || rel.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    Some(rel)
}

fn normalize_explicit_file_ref_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    if !(t.starts_with("file:///") || t.starts_with('@')) {
        return None;
    }
    normalize_ws_rel_token(t)
}

fn push_unique_rel(out: &mut Vec<String>, rel: String) {
    if !out.iter().any(|x| x == &rel) {
        out.push(rel);
    }
}

fn collect_rels_from_ws_mime(raw: &str, out: &mut Vec<String>) {
    for line in raw.lines() {
        if let Some(rel) = normalize_ws_rel_token(line) {
            push_unique_rel(out, rel);
        }
    }
}

fn collect_rel_from_pathish(token: &str, workspace_root: &str, out: &mut Vec<String>) {
    if let Some(rel) = workspace_rel_from_abs_path(token, workspace_root) {
        push_unique_rel(out, rel);
    } else if let Some(rel) = normalize_explicit_file_ref_token(token) {
        push_unique_rel(out, rel);
    }
}

fn collect_rels_from_uri_list(raw: &str, workspace_root: &str, out: &mut Vec<String>) {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        collect_rel_from_pathish(line, workspace_root, out);
    }
}

fn collect_rels_from_plain_text(raw: &str, workspace_root: &str, out: &mut Vec<String>) {
    for part in raw.split_whitespace() {
        collect_rel_from_pathish(part, workspace_root, out);
    }
}

/// 从 `DataTransfer` 收集可插入的工作区相对路径（去重保序）。
#[must_use]
pub(crate) fn workspace_rels_from_data_transfer(
    dt: &web_sys::DataTransfer,
    workspace_root: &str,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(raw) = dt.get_data(CRABMATE_WS_REL_MIME) {
        collect_rels_from_ws_mime(&raw, &mut out);
    }
    if out.is_empty()
        && let Ok(raw) = dt.get_data("text/uri-list")
    {
        collect_rels_from_uri_list(&raw, workspace_root, &mut out);
    }
    if out.is_empty()
        && let Ok(raw) = dt.get_data("text/plain")
    {
        collect_rels_from_plain_text(&raw, workspace_root, &mut out);
    }
    out
}

fn data_transfer_type_strings(dt: &web_sys::DataTransfer) -> Vec<String> {
    let types = dt.types();
    let mut out = Vec::new();
    for i in 0..types.length() {
        if let Some(t) = types.get(i).as_string() {
            out.push(t);
        }
    }
    out
}

fn data_transfer_has_files(dt: &web_sys::DataTransfer) -> bool {
    dt.files().is_some_and(|f| f.length() > 0)
        || data_transfer_type_strings(dt).iter().any(|t| t == "Files")
}

/// `dragover` / `dragenter`：若可能接受则 `preventDefault` 并标为 copy。
pub(crate) fn composer_accept_drag_over(ev: &web_sys::DragEvent) {
    let Some(dt) = ev.data_transfer() else {
        return;
    };
    let accept = data_transfer_type_strings(&dt).into_iter().any(|t| {
        t == CRABMATE_WS_REL_MIME || t == "Files" || t == "text/uri-list" || t == "text/plain"
    });
    if !accept {
        return;
    }
    ev.prevent_default();
    ev.stop_propagation();
    dt.set_drop_effect("copy");
}

fn file_looks_like_image(f: &web_sys::File) -> bool {
    let ty = f.type_();
    if ty.starts_with("image/") {
        return true;
    }
    if !ty.is_empty() {
        return false;
    }
    let name = f.name().to_ascii_lowercase();
    name.ends_with(".png")
        || name.ends_with(".jpg")
        || name.ends_with(".jpeg")
        || name.ends_with(".webp")
        || name.ends_with(".gif")
}

fn append_image_files_to_form(list: &web_sys::FileList, form: &web_sys::FormData) -> bool {
    let n = list.length();
    let mut any = false;
    for i in 0..n {
        let Some(f) = list.item(i) else {
            continue;
        };
        if !file_looks_like_image(&f) {
            continue;
        }
        let name = f.name();
        let _ = form.append_with_blob_and_filename("file", &f, &name);
        any = true;
    }
    any
}

fn upload_image_files_from_list(
    list: web_sys::FileList,
    locale: RwSignal<Locale>,
    pending_images: RwSignal<Vec<String>>,
    status_err: RwSignal<Option<String>>,
) {
    if list.length() == 0 {
        return;
    }
    let form = web_sys::FormData::new().expect("FormData");
    if !append_image_files_to_form(&list, &form) {
        return;
    }
    spawn_local(async move {
        match upload_files_multipart(&form, locale.get_untracked()).await {
            Ok(urls) => {
                pending_images.update(|v| {
                    for u in urls {
                        if v.len() >= 6 {
                            break;
                        }
                        if !v.contains(&u) {
                            v.push(u);
                        }
                    }
                });
                status_err.set(None);
            }
            Err(e) => {
                status_err.set(Some(e));
            }
        }
    });
}

fn file_list_image_flags(files: &web_sys::FileList) -> (bool, bool) {
    let n = files.length();
    let mut has_image = false;
    let mut has_non_image = false;
    for i in 0..n {
        let Some(f) = files.item(i) else {
            continue;
        };
        if file_looks_like_image(&f) {
            has_image = true;
        } else {
            has_non_image = true;
        }
    }
    (has_image, has_non_image)
}

/// 处理 composer 上的 `drop`：优先插入工作区相对路径；否则尝试图片上传。
pub(crate) fn handle_composer_file_drop(
    ev: web_sys::DragEvent,
    workspace_root: &str,
    insert_workspace_file_ref: &dyn Fn(String),
    locale: RwSignal<Locale>,
    pending_images: RwSignal<Vec<String>>,
    status_err: RwSignal<Option<String>>,
) {
    ev.prevent_default();
    ev.stop_propagation();
    let Some(dt) = ev.data_transfer() else {
        return;
    };
    let rels = workspace_rels_from_data_transfer(&dt, workspace_root);
    if !rels.is_empty() {
        for rel in rels {
            insert_workspace_file_ref(rel);
        }
        return;
    }
    if let Some(files) = dt.files() {
        let (has_image, has_non_image) = file_list_image_flags(&files);
        if has_image {
            upload_image_files_from_list(files, locale, pending_images, status_err);
        }
        if has_non_image && !has_image {
            status_err.set(Some(
                i18n::composer_drop_need_workspace_tree(locale.get_untracked()).to_string(),
            ));
        }
        return;
    }
    if data_transfer_has_files(&dt) {
        status_err.set(Some(
            i18n::composer_drop_need_workspace_tree(locale.get_untracked()).to_string(),
        ));
    }
}

/// 在拖放目标上维护进入深度，避免子节点 `dragleave` 误关高亮。
#[derive(Clone, Copy)]
pub(crate) struct ComposerDropHighlight {
    pub depth: RwSignal<u32>,
}

impl ComposerDropHighlight {
    #[must_use]
    pub fn new() -> Self {
        Self {
            depth: RwSignal::new(0),
        }
    }

    pub fn on_drag_enter(&self, ev: &web_sys::DragEvent) {
        composer_accept_drag_over(ev);
        // 仅在本处理器接受拖放时计入深度，避免无关拖拽点亮输入区。
        if ev.default_prevented() {
            self.depth.update(|d| *d = d.saturating_add(1));
        }
    }

    pub fn on_drag_leave(&self, _ev: &web_sys::DragEvent) {
        self.depth.update(|d| *d = d.saturating_sub(1));
    }

    pub fn clear(&self) {
        self.depth.set(0);
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.depth.get() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_from_abs_under_root() {
        assert_eq!(
            workspace_rel_from_abs_path("/home/u/proj/src/a.rs", "/home/u/proj"),
            Some("src/a.rs".into())
        );
        assert_eq!(
            workspace_rel_from_abs_path("file:///home/u/proj/src/a.rs", "/home/u/proj"),
            Some("src/a.rs".into())
        );
        assert_eq!(
            workspace_rel_from_abs_path("file:///elsewhere/a.rs", "/home/u/proj"),
            None
        );
        assert_eq!(
            workspace_rel_from_abs_path("file:///home/u/proj/../etc/passwd", "/home/u/proj"),
            None
        );
        // 前缀误匹配：`/proj` 不得吃掉 `/project`
        assert_eq!(
            workspace_rel_from_abs_path("/home/u/project/src/a.rs", "/home/u/proj"),
            None
        );
    }

    #[test]
    fn normalize_tokens() {
        assert_eq!(
            normalize_ws_rel_token("file:///src/main.rs"),
            Some("src/main.rs".into())
        );
        assert_eq!(normalize_ws_rel_token("@lib.rs"), Some("lib.rs".into()));
        assert_eq!(normalize_ws_rel_token("src/a.rs"), Some("src/a.rs".into()));
        assert!(normalize_ws_rel_token("file:////etc/passwd").is_none());
        assert!(normalize_ws_rel_token("a b.rs").is_none());
        assert!(normalize_explicit_file_ref_token("please").is_none());
        assert!(normalize_explicit_file_ref_token("src/a.rs").is_none());
        assert_eq!(
            normalize_explicit_file_ref_token("file:///src/a.rs"),
            Some("src/a.rs".into())
        );
    }
}
