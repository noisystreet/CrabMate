//! IDE 设置页草稿状态与保存/放弃逻辑。

use std::rc::Rc;

use leptos::prelude::*;

use crate::app::app_signals::IdeEditorSignals;

pub struct IdeSettingsDraftState {
    pub draft_editor: IdeEditorSignals,
    pub dirty: Memo<bool>,
    pub discard_rc: Rc<dyn Fn()>,
    pub save_rc: Rc<dyn Fn()>,
    pub on_back: Rc<dyn Fn()>,
}

pub fn wire_ide_settings_draft(
    ide_settings_page: RwSignal<bool>,
    editor: IdeEditorSignals,
) -> IdeSettingsDraftState {
    let draft_font_slug = RwSignal::new(editor.font_slug.get_untracked());
    let draft_font_size_px = RwSignal::new(editor.font_size_px.get_untracked());
    let draft_line_numbers = RwSignal::new(editor.line_numbers.get_untracked());
    let draft_word_wrap = RwSignal::new(editor.word_wrap.get_untracked());
    let draft_tab_size = RwSignal::new(editor.tab_size.get_untracked());

    let draft_editor = IdeEditorSignals {
        font_slug: draft_font_slug,
        font_size_px: draft_font_size_px,
        line_numbers: draft_line_numbers,
        word_wrap: draft_word_wrap,
        tab_size: draft_tab_size,
    };

    let dirty = Memo::new(move |_| {
        draft_font_slug.get() != editor.font_slug.get()
            || (draft_font_size_px.get() - editor.font_size_px.get()).abs() > f64::EPSILON
            || draft_line_numbers.get() != editor.line_numbers.get()
            || draft_word_wrap.get() != editor.word_wrap.get()
            || draft_tab_size.get() != editor.tab_size.get()
    });

    let sync_draft = move || {
        draft_font_slug.set(editor.font_slug.get_untracked());
        draft_font_size_px.set(editor.font_size_px.get_untracked());
        draft_line_numbers.set(editor.line_numbers.get_untracked());
        draft_word_wrap.set(editor.word_wrap.get_untracked());
        draft_tab_size.set(editor.tab_size.get_untracked());
    };

    Effect::new(move |_| {
        if !ide_settings_page.get() {
            return;
        }
        sync_draft();
    });

    let discard_rc: Rc<dyn Fn()> = Rc::new(sync_draft);

    let save_rc: Rc<dyn Fn()> = {
        let discard_rc = Rc::clone(&discard_rc);
        Rc::new(move || {
            editor.font_slug.set(draft_font_slug.get_untracked());
            editor.font_size_px.set(draft_font_size_px.get_untracked());
            editor.line_numbers.set(draft_line_numbers.get_untracked());
            editor.word_wrap.set(draft_word_wrap.get_untracked());
            editor.tab_size.set(draft_tab_size.get_untracked());
            editor.persist_from_signals();
            discard_rc();
        })
    };

    let on_back: Rc<dyn Fn()> = {
        let dirty = dirty;
        let discard_rc = Rc::clone(&discard_rc);
        Rc::new(move || {
            if dirty.get() {
                discard_rc();
            }
            ide_settings_page.set(false);
        })
    };

    IdeSettingsDraftState {
        draft_editor,
        dirty,
        discard_rc,
        save_rc,
        on_back,
    }
}
