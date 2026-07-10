//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{
    AddEventArgs, AddReminderArgs, IdOnlyArgs, ListEventsArgs, ListRemindersArgs, UpdateEventArgs,
    UpdateReminderArgs,
};

pub(in crate::tools) fn params_add_reminder() -> serde_json::Value {
    tool_parameters_schema_value::<AddReminderArgs>()
}

pub(in crate::tools) fn params_list_reminders() -> serde_json::Value {
    tool_parameters_schema_value::<ListRemindersArgs>()
}

pub(in crate::tools) fn params_update_reminder() -> serde_json::Value {
    tool_parameters_schema_value::<UpdateReminderArgs>()
}

pub(in crate::tools) fn params_id_only() -> serde_json::Value {
    tool_parameters_schema_value::<IdOnlyArgs>()
}

pub(in crate::tools) fn params_add_event() -> serde_json::Value {
    tool_parameters_schema_value::<AddEventArgs>()
}

pub(in crate::tools) fn params_list_events() -> serde_json::Value {
    tool_parameters_schema_value::<ListEventsArgs>()
}

pub(in crate::tools) fn params_update_event() -> serde_json::Value {
    tool_parameters_schema_value::<UpdateEventArgs>()
}

// ── Git 写操作补全 ──────────────────────────────────────────
