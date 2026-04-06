//! 设置：主题、背景、本机模型覆盖。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::api::{
    clear_client_llm_api_key_storage, client_llm_storage_has_api_key, persist_client_llm_to_storage,
};

#[allow(clippy::too_many_arguments)]
pub fn settings_modal_view(
    settings_modal: RwSignal<bool>,
    theme: RwSignal<String>,
    bg_decor: RwSignal<bool>,
    #[allow(unused_variables)] status_data: RwSignal<Option<crate::api::StatusData>>,
    llm_api_base_draft: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    llm_settings_feedback: RwSignal<Option<String>>,
    client_llm_storage_tick: RwSignal<u64>,
) -> impl IntoView {
    view! {
            <Show when=move || settings_modal.get()>
                <div class="modal-backdrop" on:click=move |_| settings_modal.set(false)>
                    <div
                        class="modal"
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="settings-modal-title"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    >
                        <div class="modal-head">
                            <h2 class="modal-title" id="settings-modal-title">"设置"</h2>
                            <span class="modal-badge">"本机"</span>
                            <span class="modal-head-spacer"></span>
                            <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| settings_modal.set(false)>
                                "关闭"
                            </button>
                        </div>
                        <div class="modal-body">
                            <p class="modal-hint">"主题与页面背景保存在本机（localStorage）。模型网关与 API 密钥也可仅存本机；发消息时会在 JSON 中附带覆盖项，请仅在可信环境（HTTPS）使用。"</p>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"主题"</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "dark"
                                        on:click=move |_| theme.set("dark".to_string())
                                    >
                                        "深色"
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "light"
                                        on:click=move |_| theme.set("light".to_string())
                                    >
                                        "浅色"
                                    </button>
                                </div>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"页面背景"</h3>
                                <label class="settings-checkbox-label">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || bg_decor.get()
                                        on:change=move |_| bg_decor.update(|v| *v = !*v)
                                    />
                                    <span>"显示背景光晕（径向渐变）"</span>
                                </label>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"模型网关（可选覆盖）"</h3>
                                <p class="modal-hint settings-field-nested-hint">
                                    "留空则使用服务端配置与环境变量 API_KEY。API 密钥使用密码框，不会以明文显示。"
                                </p>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-base">
                                        "API 基址（api_base）"
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-api-base"
                                        class="settings-text-input"
                                        placeholder="例如 https://api.deepseek.com/v1"
                                        prop:value=move || llm_api_base_draft.get()
                                        on:input=move |ev| {
                                            llm_api_base_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-model">
                                        "模型名称（model）"
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-model"
                                        class="settings-text-input"
                                        placeholder="例如 deepseek-chat"
                                        prop:value=move || llm_model_draft.get()
                                        on:input=move |ev| {
                                            llm_model_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-key">
                                        "API 密钥（覆盖 API_KEY）"
                                    </label>
                                    <input
                                        type="password"
                                        id="settings-llm-api-key"
                                        class="settings-text-input"
                                        autocomplete="off"
                                        placeholder="留空保留已存密钥；填写新密钥后点保存"
                                        prop:value=move || llm_api_key_draft.get()
                                        on:input=move |ev| {
                                            llm_api_key_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <Show when=move || llm_has_saved_key.get()>
                                    <p class="modal-hint settings-field-nested-hint">
                                        "当前已在本机保存密钥（不会回显到输入框）。"
                                    </p>
                                </Show>
                                <div class="settings-actions-row">
                                    <button
                                        type="button"
                                        class="btn btn-primary btn-sm"
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let key_raw = llm_api_key_draft.get();
                                            let api_key_upd = if key_raw.trim().is_empty() {
                                                None
                                            } else {
                                                Some(key_raw)
                                            };
                                            let base = llm_api_base_draft.get();
                                            let model = llm_model_draft.get();
                                            match persist_client_llm_to_storage(
                                                &base,
                                                &model,
                                                api_key_upd.as_deref(),
                                            ) {
                                                Ok(()) => {
                                                    llm_api_key_draft.set(String::new());
                                                    llm_has_saved_key
                                                        .set(client_llm_storage_has_api_key());
                                                    client_llm_storage_tick
                                                        .update(|n| *n = n.wrapping_add(1));
                                                    llm_settings_feedback.set(Some(
                                                        "已保存到本机浏览器".into(),
                                                    ));
                                                }
                                                Err(e) => llm_settings_feedback.set(Some(e)),
                                            }
                                        }
                                    >
                                        "保存模型设置"
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        prop:disabled=move || !llm_has_saved_key.get()
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let _ = clear_client_llm_api_key_storage();
                                            llm_has_saved_key.set(false);
                                            llm_settings_feedback.set(Some(
                                                "已清除本机保存的密钥".into(),
                                            ));
                                        }
                                    >
                                        "清除已存密钥"
                                    </button>
                                </div>
                                <Show when=move || llm_settings_feedback.get().is_some()>
                                    <p class="settings-save-feedback">{move || {
                                        llm_settings_feedback.get().unwrap_or_default()
                                    }}</p>
                                </Show>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>
    }
}
