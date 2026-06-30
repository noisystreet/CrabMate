//! IDE 布局壳层：查找/跳转、确认框、新建文件模态等。

use leptos::prelude::*;

use crate::ide_confirm::{IdeConfirmPrompt, IdeConfirmResult};

#[derive(Clone, Copy)]
pub struct IdeChromeSignals {
    pub find_panel_open: RwSignal<bool>,
    pub find_query: RwSignal<String>,
    pub find_match_index: RwSignal<usize>,
    pub goto_panel_open: RwSignal<bool>,
    pub goto_line: RwSignal<String>,
    pub close_active_tab_nonce: RwSignal<u64>,
    pub confirm_pending: RwSignal<Option<IdeConfirmPrompt>>,
    pub confirm_result: RwSignal<Option<IdeConfirmResult>>,
    pub new_file_modal_open: RwSignal<bool>,
    pub new_file_path_draft: RwSignal<String>,
}

impl IdeChromeSignals {
    pub fn new() -> Self {
        Self {
            find_panel_open: RwSignal::new(false),
            find_query: RwSignal::new(String::new()),
            find_match_index: RwSignal::new(0),
            goto_panel_open: RwSignal::new(false),
            goto_line: RwSignal::new(String::new()),
            close_active_tab_nonce: RwSignal::new(0),
            confirm_pending: RwSignal::new(None),
            confirm_result: RwSignal::new(None),
            new_file_modal_open: RwSignal::new(false),
            new_file_path_draft: RwSignal::new(String::new()),
        }
    }

    pub fn confirm_signals(&self) -> crate::ide_confirm::IdeConfirmSignals {
        crate::ide_confirm::IdeConfirmSignals {
            pending: self.confirm_pending,
            result: self.confirm_result,
        }
    }
}

impl Default for IdeChromeSignals {
    fn default() -> Self {
        Self::new()
    }
}
