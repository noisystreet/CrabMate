//! 主发送与「截断再生 / 失败重试」共用的**显式门闩**（与 [`super::composer_wires`] 配合）。
//!
//! 将原先散在 `if` 链中的布尔组合收进具名类型，便于单测与排障时对照字段语义。

use leptos::prelude::Get;

use crate::chat_session_state::ChatSessionSignals;

use super::handles::ComposerStreamShell;

/// 截断再生 `attach` 前：壳层 busy / 工具忙 / 中止槽占用 / 其它助手 Loading 的快照（**不计**当前尾条 `asst_id` 自身占位）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RegenAttachGate {
    pub status_busy: bool,
    pub tool_busy: bool,
    pub abort_present: bool,
    pub conflict_loading: bool,
}

impl RegenAttachGate {
    #[must_use]
    pub(crate) fn capture(
        shell: &ComposerStreamShell,
        chat: ChatSessionSignals,
        exclude_asst_id: &str,
    ) -> Self {
        let lc = shell.stream.turn_lifecycle.get();
        let status_busy = crate::app::turn_lifecycle::turn_lifecycle_model_ui_busy(lc);
        let tool_busy = crate::app::turn_lifecycle::turn_lifecycle_tool_ui_busy(lc);
        let abort_present = shell
            .stream
            .abort_cell
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        let conflict_loading =
            crate::chat_session_state::session_has_conflicting_stream_loading_placeholders(
                chat,
                exclude_asst_id,
            );
        Self {
            status_busy,
            tool_busy,
            abort_present,
            conflict_loading,
        }
    }

    #[must_use]
    pub(crate) const fn is_blocked(self) -> bool {
        self.status_busy || self.tool_busy || self.abort_present || self.conflict_loading
    }
}

/// 主发送路径：初始化完成、回合不忙、且输入（正文 / 附图 / 澄清答案）至少有一项。
#[must_use]
pub(crate) const fn compose_user_send_allowed(
    initialized: bool,
    stream_turn_busy_ui: bool,
    user_line_empty: bool,
    imgs_empty: bool,
    clarify_json_none: bool,
) -> bool {
    initialized && !stream_turn_busy_ui && !(user_line_empty && imgs_empty && clarify_json_none)
}

#[cfg(test)]
mod tests {
    use super::{RegenAttachGate, compose_user_send_allowed};

    #[test]
    fn compose_send_requires_initialized_and_not_busy() {
        assert!(!compose_user_send_allowed(false, false, false, false, true));
        assert!(compose_user_send_allowed(true, false, false, false, true));
        assert!(!compose_user_send_allowed(true, true, false, false, true));
    }

    #[test]
    fn compose_send_allows_clarify_only() {
        assert!(compose_user_send_allowed(true, false, true, true, false));
    }

    #[test]
    fn regen_blocked_if_any_gate_flag() {
        assert!(
            RegenAttachGate {
                status_busy: true,
                ..Default::default()
            }
            .is_blocked()
        );
        assert!(
            RegenAttachGate {
                tool_busy: true,
                ..Default::default()
            }
            .is_blocked()
        );
        assert!(
            RegenAttachGate {
                abort_present: true,
                ..Default::default()
            }
            .is_blocked()
        );
        assert!(
            RegenAttachGate {
                conflict_loading: true,
                ..Default::default()
            }
            .is_blocked()
        );
        assert!(!RegenAttachGate::default().is_blocked());
    }
}
