//! 会话列表 / 设置 / 变更集等模态开关与变更集正文状态。

use leptos::prelude::*;

use crate::app::changelist_modal::ChangelistModalBodyState;

#[derive(Clone, Copy)]
pub struct ModalSignals {
    pub session_modal: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
    pub ide_settings_page: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_modal_body: RwSignal<ChangelistModalBodyState>,
    pub changelist_body_ref: NodeRef<leptos::html::Div>,
    pub changelist_fetch_nonce: RwSignal<u64>,
}

impl ModalSignals {
    pub fn new() -> Self {
        Self {
            session_modal: RwSignal::new(false),
            settings_modal: RwSignal::new(false),
            settings_page: RwSignal::new(false),
            ide_settings_page: RwSignal::new(false),
            changelist_modal_open: RwSignal::new(false),
            changelist_modal_body: RwSignal::new(ChangelistModalBodyState::default()),
            changelist_body_ref: NodeRef::new(),
            changelist_fetch_nonce: RwSignal::new(0),
        }
    }
}

impl Default for ModalSignals {
    fn default() -> Self {
        Self::new()
    }
}
