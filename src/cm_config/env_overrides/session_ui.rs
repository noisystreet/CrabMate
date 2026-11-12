//! `session_ui.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::apply_parse;

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.session_ui.max_message_history,
        "CM_MAX_MESSAGE_HISTORY",
    );
}
