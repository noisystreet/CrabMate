//! 工作区侧栏数据刷新与主/侧列拖拽宽度。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use leptos::prelude::*;
use leptos_dom::helpers::WindowListenerHandle;
use leptos_dom::helpers::window_event_listener;

use crate::api::{WorkspaceData, fetch_workspace};
use crate::app_prefs::{SidePanelView, clamp_side_width_for_viewport};
use crate::i18n::Locale;

/// 工作区列表中「文件」行的图标类别（目录单独用文件夹图标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFileKind {
    Generic,
    Code,
    Data,
    Markdown,
    Image,
    Shell,
    Web,
    Lock,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceListRowKind {
    Dir,
    File(WorkspaceFileKind),
}

/// 由文件名（含后缀）推断图标类别；目录在调用方单独处理。
pub fn workspace_file_kind(name: &str) -> WorkspaceFileKind {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "makefile" | "gnumakefile" | "justfile" | "rakefile" | "procfile"
    ) {
        return WorkspaceFileKind::Shell;
    }
    if lower.starts_with("dockerfile") || lower.starts_with("containerfile") {
        return WorkspaceFileKind::Shell;
    }

    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = ext.as_str();

    match ext {
        "" => WorkspaceFileKind::Generic,
        "rs" | "go" | "py" | "pyi" | "pyw" | "c" | "h" | "hpp" | "hh" | "cpp" | "cc" | "cxx"
        | "java" | "kt" | "kts" | "swift" | "rb" | "php" | "cs" | "fs" | "fsi" | "fsx"
        | "scala" | "sc" | "r" | "dart" | "zig" | "nim" | "m" | "mm" | "pl" | "pm" | "lua"
        | "jl" | "ex" | "exs" | "erl" | "hrl" | "clj" | "cljs" | "hs" | "lhs" | "ml" | "mli"
        | "pas" | "pp" | "vim" | "el" | "cl" | "asm" | "s" | "ino" | "v" | "sv" | "svh"
        | "vhdl" | "vhd" => WorkspaceFileKind::Code,
        "html" | "htm" | "xhtml" | "css" | "scss" | "sass" | "less" | "vue" | "svelte" => {
            WorkspaceFileKind::Web
        }
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts" => WorkspaceFileKind::Web,
        "json" | "jsonc" | "toml" | "yaml" | "yml" | "xml" | "plist" => WorkspaceFileKind::Data,
        "md" | "mdx" | "rst" => WorkspaceFileKind::Markdown,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tiff" | "tif" | "heic"
        | "avif" | "jxl" | "svg" => WorkspaceFileKind::Image,
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "psm1" | "bat" | "cmd" | "awk" | "sed" => {
            WorkspaceFileKind::Shell
        }
        "lock" => WorkspaceFileKind::Lock,
        "txt" | "log" | "text" | "csv" | "tsv" => WorkspaceFileKind::Text,
        _ => WorkspaceFileKind::Generic,
    }
}

fn workspace_list_row_kind(is_dir: bool, name: &str) -> WorkspaceListRowKind {
    if is_dir {
        WorkspaceListRowKind::Dir
    } else {
        WorkspaceListRowKind::File(workspace_file_kind(name))
    }
}

fn workspace_file_kind_css(k: WorkspaceFileKind) -> &'static str {
    match k {
        WorkspaceFileKind::Generic => "workspace-file-kind--generic",
        WorkspaceFileKind::Code => "workspace-file-kind--code",
        WorkspaceFileKind::Data => "workspace-file-kind--data",
        WorkspaceFileKind::Markdown => "workspace-file-kind--markdown",
        WorkspaceFileKind::Image => "workspace-file-kind--image",
        WorkspaceFileKind::Shell => "workspace-file-kind--shell",
        WorkspaceFileKind::Web => "workspace-file-kind--web",
        WorkspaceFileKind::Lock => "workspace-file-kind--lock",
        WorkspaceFileKind::Text => "workspace-file-kind--text",
    }
}

/// `li` 的 `class` 字符串（含 `dir` 或 `file workspace-file-kind--*`）。
pub fn workspace_list_row_class(is_dir: bool, name: &str) -> String {
    match workspace_list_row_kind(is_dir, name) {
        WorkspaceListRowKind::Dir => "dir".to_string(),
        WorkspaceListRowKind::File(fk) => format!("file {}", workspace_file_kind_css(fk)),
    }
}

fn svg_common() -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    (
        "workspace-entry-icon workspace-entry-svg",
        "0 0 24 24",
        "none",
        "http://www.w3.org/2000/svg",
        "currentColor",
        "2",
    )
}

/// 与 `workspace_list_row_class` 配对的图标视图（目录为文件夹，文件按后缀）。
pub fn workspace_list_row_icon(is_dir: bool, name: &str) -> AnyView {
    let kind = workspace_list_row_kind(is_dir, name);
    let (cls, vb, fill, xmlns, stroke, sw) = svg_common();
    match kind {
        WorkspaceListRowKind::Dir => view! {
            <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                <path
                    d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
                    stroke=stroke
                    stroke-width=sw
                    stroke-linecap="round"
                    stroke-linejoin="round"
                />
            </svg>
        }
        .into_any(),
        WorkspaceListRowKind::File(fk) => match fk {
            WorkspaceFileKind::Generic => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <polyline
                        points="14 2 14 8 20 8"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Code => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path d="m10 12-2 2 2 2" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path d="m14 16 2-2-2-2" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Data => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path d="M10 12h2" stroke=stroke stroke-width=sw stroke-linecap="round" />
                    <path d="M10 15h6" stroke=stroke stroke-width=sw stroke-linecap="round" />
                    <path d="M10 18h4" stroke=stroke stroke-width=sw stroke-linecap="round" />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Markdown => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path
                        d="M9 17V9l3 3 3-3v8"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Image => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <rect
                        x="3"
                        y="3"
                        width="18"
                        height="18"
                        rx="2"
                        ry="2"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <circle
                        cx="8.5"
                        cy="8.5"
                        r="1.5"
                        fill="none"
                        stroke=stroke
                        stroke-width=sw
                    />
                    <path
                        d="m21 15-3.5-3.5a2 2 0 0 0-2.83 0L6 21"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Shell => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path d="M8 14h8" stroke=stroke stroke-width=sw stroke-linecap="round" />
                    <path d="m10 17 2 2 4-4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Web => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path
                        d="M8 13h8M10 10h4M10 16h4"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Lock => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <rect
                        x="8"
                        y="11"
                        width="8"
                        height="7"
                        rx="1"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path
                        d="M10 11V9a2 2 0 0 1 4 0v2"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                </svg>
            }
            .into_any(),
            WorkspaceFileKind::Text => view! {
                <svg class=cls viewBox=vb fill=fill xmlns=xmlns aria-hidden="true">
                    <path
                        d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"
                        stroke=stroke
                        stroke-width=sw
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    />
                    <path d="M14 2v4h4" stroke=stroke stroke-width=sw stroke-linecap="round" stroke-linejoin="round" />
                    <path d="M9 12h7M9 15h6M9 18h8" stroke=stroke stroke-width=sw stroke-linecap="round" />
                </svg>
            }
            .into_any(),
        },
    }
}

pub async fn reload_workspace_panel(
    workspace_loading: RwSignal<bool>,
    workspace_err: RwSignal<Option<String>>,
    workspace_path_draft: RwSignal<String>,
    workspace_data: RwSignal<Option<WorkspaceData>>,
    workspace_subtree_expanded: RwSignal<HashSet<String>>,
    workspace_subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    workspace_subtree_loading: RwSignal<HashSet<String>>,
    locale: Locale,
) {
    workspace_subtree_expanded.set(HashSet::new());
    workspace_subtree_cache.set(HashMap::new());
    workspace_subtree_loading.set(HashSet::new());
    workspace_loading.set(true);
    match fetch_workspace(None, locale).await {
        Ok(d) => {
            workspace_err.set(None);
            workspace_path_draft.set(d.path.clone());
            workspace_data.set(Some(d));
        }
        Err(e) => {
            workspace_err.set(Some(e));
            workspace_data.set(None);
        }
    }
    workspace_loading.set(false);
}

pub fn begin_side_column_resize(
    ev: web_sys::MouseEvent,
    side_panel_view: RwSignal<SidePanelView>,
    side_width: RwSignal<f64>,
    side_resize_dragging: RwSignal<bool>,
    side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>>,
) {
    if ev.button() != 0 {
        return;
    }
    if matches!(side_panel_view.get_untracked(), SidePanelView::None) {
        return;
    }
    ev.prevent_default();
    if let Some((m, u)) = side_resize_handles.borrow_mut().take() {
        m.remove();
        u.remove();
        *side_resize_session.borrow_mut() = None;
        side_resize_dragging.set(false);
    }

    *side_resize_session.borrow_mut() = Some((ev.client_x() as f64, side_width.get_untracked()));
    side_resize_dragging.set(true);

    let session_m = Rc::clone(&side_resize_session);
    let session_u = Rc::clone(&side_resize_session);
    let handles_slot = Rc::clone(&side_resize_handles);
    let side_w = side_width;
    let drag_sig = side_resize_dragging;

    let hm = window_event_listener(leptos::ev::mousemove, move |e: web_sys::MouseEvent| {
        let borrow = session_m.borrow();
        let Some((sx, sw)) = *borrow else {
            return;
        };
        let cx = e.client_x() as f64;
        side_w.set(clamp_side_width_for_viewport(sw - (cx - sx)));
    });

    let hu = window_event_listener(leptos::ev::mouseup, move |_e: web_sys::MouseEvent| {
        *session_u.borrow_mut() = None;
        drag_sig.set(false);
        if let Some((m, u)) = handles_slot.borrow_mut().take() {
            m.remove();
            u.remove();
        }
    });

    *side_resize_handles.borrow_mut() = Some((hm, hu));
}

#[cfg(test)]
mod workspace_file_kind_tests {
    use super::WorkspaceFileKind;
    use super::workspace_file_kind;

    #[test]
    fn ext_and_special_names() {
        assert_eq!(workspace_file_kind("lib.rs"), WorkspaceFileKind::Code);
        assert_eq!(workspace_file_kind("app.tsx"), WorkspaceFileKind::Web);
        assert_eq!(workspace_file_kind("package.json"), WorkspaceFileKind::Data);
        assert_eq!(
            workspace_file_kind("README.md"),
            WorkspaceFileKind::Markdown
        );
        assert_eq!(workspace_file_kind("icon.png"), WorkspaceFileKind::Image);
        assert_eq!(workspace_file_kind("run.sh"), WorkspaceFileKind::Shell);
        assert_eq!(workspace_file_kind("Cargo.lock"), WorkspaceFileKind::Lock);
        assert_eq!(workspace_file_kind("notes.txt"), WorkspaceFileKind::Text);
        assert_eq!(workspace_file_kind("Makefile"), WorkspaceFileKind::Shell);
        assert_eq!(workspace_file_kind("Dockerfile"), WorkspaceFileKind::Shell);
        assert_eq!(workspace_file_kind("weird.bin"), WorkspaceFileKind::Generic);
    }
}
