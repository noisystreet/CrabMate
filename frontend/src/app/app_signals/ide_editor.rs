//! IDE 内置编辑器本机偏好信号。

use leptos::prelude::*;

use crate::ide_editor_prefs::IdeEditorPrefs;

#[derive(Clone, Copy)]
pub struct IdeEditorSignals {
    pub font_slug: RwSignal<String>,
    pub font_size_px: RwSignal<f64>,
    pub line_numbers: RwSignal<bool>,
    pub word_wrap: RwSignal<bool>,
    pub tab_size: RwSignal<u8>,
}

impl IdeEditorSignals {
    pub fn new() -> Self {
        let p = IdeEditorPrefs::load();
        Self {
            font_slug: RwSignal::new(p.font_slug),
            font_size_px: RwSignal::new(p.font_size_px),
            line_numbers: RwSignal::new(p.line_numbers),
            word_wrap: RwSignal::new(p.word_wrap),
            tab_size: RwSignal::new(p.tab_size),
        }
    }

    pub fn persist_from_signals(&self) {
        IdeEditorPrefs {
            font_slug: self.font_slug.get_untracked(),
            font_size_px: self.font_size_px.get_untracked(),
            line_numbers: self.line_numbers.get_untracked(),
            word_wrap: self.word_wrap.get_untracked(),
            tab_size: self.tab_size.get_untracked(),
        }
        .persist();
    }
}

impl Default for IdeEditorSignals {
    fn default() -> Self {
        Self::new()
    }
}
