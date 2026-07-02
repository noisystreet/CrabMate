//! CodeMirror 6 编辑器桥接（`vendor/ide-codemirror.js`）。

use js_sys::{Function, Reflect};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

use crate::ide_editor_prefs::ide_editor_font_family_css;
use crate::ide_syntax_highlight::ide_syntax_lang_for_path;

const CM_MOUNT_MAX_RETRIES: u32 = 12;

fn cm_global() -> JsValue {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("CrabMateIdeEditor"))
        .unwrap_or(JsValue::NULL)
}

fn cm_fn(name: &str) -> Option<Function> {
    let g = cm_global();
    if g.is_null() || g.is_undefined() {
        return None;
    }
    Reflect::get(&g, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

fn call_cm1(name: &str, a0: &JsValue) {
    if let Some(f) = cm_fn(name) {
        let _ = f.call1(&cm_global(), a0);
    }
}

fn call_cm2(name: &str, a0: &JsValue, a1: &JsValue) {
    if let Some(f) = cm_fn(name) {
        let _ = f.call2(&cm_global(), a0, a1);
    }
}

/// CodeMirror 创建选项。
pub struct IdeCmCreateOptions<'a> {
    pub path: Option<&'a str>,
    pub doc: &'a str,
    pub read_only: bool,
    pub line_numbers: bool,
    pub word_wrap: bool,
    pub tab_size: u8,
    pub font_slug: &'a str,
    pub font_size_px: f64,
}

/// 编辑器偏好与缓冲信号（`wire_ide_codemirror` 入参）。
#[derive(Clone, Copy)]
pub struct IdeCmWireSignals {
    /// IDE 布局层是否可见（对话/IDE 叠层切换）；CM 须在可见后再挂载。
    pub editor_visible: RwSignal<bool>,
    pub ide_path: RwSignal<Option<String>>,
    pub ide_text: RwSignal<String>,
    pub ide_load_busy: RwSignal<bool>,
    pub line_numbers: RwSignal<bool>,
    pub word_wrap: RwSignal<bool>,
    pub tab_size: RwSignal<u8>,
    pub font_slug: RwSignal<String>,
    pub font_size_px: RwSignal<f64>,
    pub cm_init_failed: RwSignal<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdeCmHandle(pub i32);

/// 挂载 CodeMirror 的容器与句柄信号。
#[derive(Clone, Copy)]
pub struct IdeEditorHost {
    pub container: NodeRef<leptos::html::Div>,
    pub handle: RwSignal<Option<IdeCmHandle>>,
}

impl IdeEditorHost {
    #[must_use]
    pub fn new() -> Self {
        Self {
            container: NodeRef::new(),
            handle: RwSignal::new(None),
        }
    }

    #[must_use]
    pub fn cm_available() -> bool {
        !cm_global().is_null() && !cm_global().is_undefined()
    }

    pub fn destroy_if_any(&self) {
        if let Some(IdeCmHandle(id)) = self.handle.get_untracked() {
            call_cm1("destroy", &JsValue::from_f64(f64::from(id)));
            self.handle.set(None);
        }
    }

    pub fn get_doc(&self) -> Option<String> {
        let IdeCmHandle(id) = self.handle.get_untracked()?;
        let f = cm_fn("getDoc")?;
        let v = f
            .call1(&cm_global(), &JsValue::from_f64(f64::from(id)))
            .ok()?;
        v.as_string()
    }

    pub fn set_doc_if_changed(&self, text: &str) {
        let Some(IdeCmHandle(id)) = self.handle.get_untracked() else {
            return;
        };
        if self.get_doc().as_deref() == Some(text) {
            return;
        }
        call_cm2(
            "setDoc",
            &JsValue::from_f64(f64::from(id)),
            &JsValue::from_str(text),
        );
    }

    pub fn select_all(&self) {
        if let Some(IdeCmHandle(id)) = self.handle.get_untracked() {
            call_cm1("selectAll", &JsValue::from_f64(f64::from(id)));
        }
    }

    pub fn set_selection_chars(&self, start_char: usize, end_char: usize) {
        let Some(IdeCmHandle(id)) = self.handle.get_untracked() else {
            return;
        };
        let f = match cm_fn("setSelectionChars") {
            Some(f) => f,
            None => return,
        };
        let _ = f.call3(
            &cm_global(),
            &JsValue::from_f64(f64::from(id)),
            &JsValue::from_f64(start_char as f64),
            &JsValue::from_f64(end_char as f64),
        );
    }

    pub fn goto_line(&self, line_one_based: usize) {
        let Some(IdeCmHandle(id)) = self.handle.get_untracked() else {
            return;
        };
        let f = match cm_fn("gotoLine") {
            Some(f) => f,
            None => return,
        };
        let _ = f.call2(
            &cm_global(),
            &JsValue::from_f64(f64::from(id)),
            &JsValue::from_f64(line_one_based as f64),
        );
    }

    pub fn reconfigure(&self, patch: &JsValue) {
        let Some(IdeCmHandle(id)) = self.handle.get_untracked() else {
            return;
        };
        call_cm2("reconfigure", &JsValue::from_f64(f64::from(id)), patch);
    }

    pub fn request_measure(&self) {
        if let Some(IdeCmHandle(id)) = self.handle.get_untracked() {
            call_cm1("requestMeasure", &JsValue::from_f64(f64::from(id)));
        }
    }
}

fn lang_wire_id(path: Option<&str>) -> Option<&'static str> {
    use crate::ide_syntax_highlight::IdeSyntaxLang::{
        C, Cpp, Go, JavaScript, Json, Markdown, Python, Rust, Shell, Toml, TypeScript, Yaml,
    };
    match ide_syntax_lang_for_path(path)? {
        Rust => Some("rust"),
        Toml => Some("toml"),
        Yaml => Some("yaml"),
        C => Some("c"),
        Cpp => Some("cpp"),
        Python => Some("python"),
        JavaScript => Some("javascript"),
        TypeScript => Some("typescript"),
        Json => Some("json"),
        Markdown => Some("markdown"),
        Shell => Some("shell"),
        Go => Some("go"),
    }
}

fn js_options(options: &IdeCmCreateOptions<'_>) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("doc"),
        &JsValue::from_str(options.doc),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("readOnly"),
        &JsValue::from_bool(options.read_only),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("lineNumbers"),
        &JsValue::from_bool(options.line_numbers),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("wordWrap"),
        &JsValue::from_bool(options.word_wrap),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("tabSize"),
        &JsValue::from_f64(f64::from(options.tab_size)),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("fontSize"),
        &JsValue::from_f64(f64::from(options.font_size_px.round() as u32)),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("fontFamily"),
        &JsValue::from_str(ide_editor_font_family_css(options.font_slug)),
    );
    if let Some(lang) = lang_wire_id(options.path) {
        let _ = Reflect::set(&obj, &JsValue::from_str("lang"), &JsValue::from_str(lang));
    }
    obj.into()
}

fn js_patch(
    path: Option<&str>,
    read_only: bool,
    line_numbers: bool,
    word_wrap: bool,
    tab_size: u8,
    font_slug: &str,
    font_size_px: f64,
) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("readOnly"),
        &JsValue::from_bool(read_only),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("lineNumbers"),
        &JsValue::from_bool(line_numbers),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("wordWrap"),
        &JsValue::from_bool(word_wrap),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("tabSize"),
        &JsValue::from_f64(f64::from(tab_size)),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("fontSize"),
        &JsValue::from_f64(f64::from(font_size_px.round() as u32)),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("fontFamily"),
        &JsValue::from_str(ide_editor_font_family_css(font_slug)),
    );
    if let Some(lang) = lang_wire_id(path) {
        let _ = Reflect::set(&obj, &JsValue::from_str("lang"), &JsValue::from_str(lang));
    }
    obj.into()
}

/// 在 `parent` 上创建 CodeMirror 实例；`on_change` 在文档变更时回调 UTF-8 全文。
pub fn cm_create(
    parent: &HtmlElement,
    options: &IdeCmCreateOptions<'_>,
    on_change: &Function,
) -> Option<IdeCmHandle> {
    let create = cm_fn("create")?;
    let opts = js_options(options);
    let id = create
        .call3(&cm_global(), &parent.into(), &opts, on_change)
        .ok()?;
    id.as_f64().map(|n| IdeCmHandle(n as i32))
}

fn after_animation_frame(f: impl FnOnce() + 'static) {
    let Some(win) = web_sys::window() else {
        f();
        return;
    };
    let cb = Closure::once(Box::new(f));
    let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
    cb.forget();
}

/// 布局/visibility 切换后再挂载 CM（Tauri / WebKit 在 hidden→visible 同帧创建易失败）。
fn after_layout_settled(f: impl FnOnce() + 'static) {
    after_animation_frame(move || after_animation_frame(f));
}

fn mount_cm_instance(
    host: IdeEditorHost,
    parent: &HtmlElement,
    signals: IdeCmWireSignals,
    suppress_sync: RwSignal<bool>,
) -> bool {
    let IdeCmWireSignals {
        ide_path,
        ide_text,
        ide_load_busy,
        line_numbers,
        word_wrap,
        tab_size,
        font_slug,
        font_size_px,
        ..
    } = signals;

    host.destroy_if_any();
    parent.set_inner_html("");

    let ide_text_cb = ide_text;
    let suppress = suppress_sync;
    let on_change = Closure::wrap(Box::new(move |id: JsValue, text: JsValue| {
        let _ = id;
        let Some(s) = text.as_string() else {
            return;
        };
        suppress.set(true);
        ide_text_cb.set(s);
        suppress.set(false);
    }) as Box<dyn FnMut(JsValue, JsValue)>);

    let path = ide_path.get_untracked();
    let read_only = path.is_none() || ide_load_busy.get_untracked();
    let opts = IdeCmCreateOptions {
        path: path.as_deref(),
        doc: &ide_text.get_untracked(),
        read_only,
        line_numbers: line_numbers.get_untracked(),
        word_wrap: word_wrap.get_untracked(),
        tab_size: tab_size.get_untracked(),
        font_slug: &font_slug.get_untracked(),
        font_size_px: font_size_px.get_untracked(),
    };
    let ok = if let Some(handle) = cm_create(parent, &opts, on_change.as_ref().unchecked_ref()) {
        host.handle.set(Some(handle));
        host.request_measure();
        true
    } else {
        false
    };
    on_change.forget();
    ok
}

/// 挂载 / 更新 IDE 编辑器：容器就绪后创建 CM，并随偏好与路径重配置。
pub fn wire_ide_codemirror(host: IdeEditorHost, signals: IdeCmWireSignals) {
    let IdeCmWireSignals {
        editor_visible,
        ide_path,
        ide_text,
        ide_load_busy,
        line_numbers,
        word_wrap,
        tab_size,
        font_slug,
        font_size_px,
        cm_init_failed,
    } = signals;
    let suppress_sync = RwSignal::new(false);
    let cm_mount_retry = RwSignal::new(0u32);
    let cm_mount_generation = RwSignal::new(0u32);

    // 外部（切标签等）写入 ide_text → 同步到 CM
    Effect::new(move |_| {
        let text = ide_text.get();
        if suppress_sync.get_untracked() {
            return;
        }
        host.set_doc_if_changed(&text);
    });

    // 偏好 / 路径 / 只读 → 重配置
    Effect::new(move |_| {
        let _ = ide_path.get();
        let _ = ide_load_busy.get();
        let _ = line_numbers.get();
        let _ = word_wrap.get();
        let _ = tab_size.get();
        let _ = font_slug.get();
        let _ = font_size_px.get();

        let Some(IdeCmHandle(_)) = host.handle.get_untracked() else {
            return;
        };
        let read_only = ide_path.get_untracked().is_none() || ide_load_busy.get_untracked();
        let patch = js_patch(
            ide_path.get_untracked().as_deref(),
            read_only,
            line_numbers.get_untracked(),
            word_wrap.get_untracked(),
            tab_size.get_untracked(),
            &font_slug.get_untracked(),
            font_size_px.get_untracked(),
        );
        host.reconfigure(&patch);
    });

    // 容器挂载：仅在 IDE 层可见时**首次**创建 CM；切回对话模式**保留**实例（撤销栈等），再次显示时 requestMeasure。
    Effect::new(move |_| {
        let visible = editor_visible.get();
        let _retry = cm_mount_retry.get();
        let Some(el) = host.container.get() else {
            return;
        };
        if !visible {
            return;
        }

        if host.handle.get_untracked().is_some() {
            host.request_measure();
            cm_init_failed.set(false);
            return;
        }
        if !IdeEditorHost::cm_available() {
            return;
        }

        let parent: HtmlElement = el.unchecked_into();
        let generation = cm_mount_generation.get_untracked().wrapping_add(1);
        cm_mount_generation.set(generation);
        let attempt = cm_mount_retry.get_untracked();
        let wire = signals;

        after_layout_settled(move || {
            if cm_mount_generation.get_untracked() != generation {
                return;
            }
            if !editor_visible.get_untracked() || host.handle.get_untracked().is_some() {
                return;
            }
            if !IdeEditorHost::cm_available() {
                return;
            }

            cm_init_failed.set(false);
            if mount_cm_instance(host, &parent, wire, suppress_sync) {
                cm_mount_retry.set(0);
                cm_init_failed.set(false);
                return;
            }

            if attempt + 1 < CM_MOUNT_MAX_RETRIES {
                cm_mount_retry.set(attempt + 1);
            } else {
                cm_init_failed.set(true);
            }
        });
    });

    on_cleanup(move || {
        host.destroy_if_any();
    });
}

#[cfg(test)]
mod tests {
    use super::lang_wire_id;

    #[test]
    fn lang_wire_maps_rust() {
        assert_eq!(lang_wire_id(Some("src/lib.rs")), Some("rust"));
    }
}
