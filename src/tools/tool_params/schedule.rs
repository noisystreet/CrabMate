//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_add_reminder() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "提醒内容" },
            "due_at": { "type": "string", "description": "可选：到期时间（支持 RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD）" }
        },
        "required": ["title"]
    })
}

pub(in crate::tools) fn params_list_reminders() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "include_done": { "type": "boolean", "description": "是否包含已完成提醒，默认 false" },
            "future_days": { "type": "integer", "description": "可选：仅显示未来 N 天内到期的提醒（只筛选有 due_at 的提醒）", "minimum": 0 }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_update_reminder() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "提醒 id" },
            "title": { "type": "string", "description": "可选：更新标题" },
            "due_at": { "type": "string", "description": "可选：更新到期时间（空字符串表示清空）" },
            "done": { "type": "boolean", "description": "可选：更新完成状态" }
        },
        "required": ["id"]
    })
}

pub(in crate::tools) fn params_id_only() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "id": { "type": "string", "description": "条目 id" } },
        "required": ["id"]
    })
}

pub(in crate::tools) fn params_add_event() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "日程标题" },
            "start_at": { "type": "string", "description": "开始时间（RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD）" },
            "end_at": { "type": "string", "description": "可选：结束时间（同 start_at 格式）" },
            "location": { "type": "string", "description": "可选：地点" },
            "notes": { "type": "string", "description": "可选：备注" }
        },
        "required": ["title", "start_at"]
    })
}

pub(in crate::tools) fn params_list_events() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "year": { "type": "integer", "description": "可选：按年份过滤（如 2026）" },
            "month": { "type": "integer", "description": "可选：按月份过滤（1-12，通常与 year 一起用）", "minimum": 1, "maximum": 12 },
            "future_days": { "type": "integer", "description": "可选：仅显示未来 N 天内开始的日程（按 start_at 过滤）", "minimum": 0 }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_update_event() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "日程 id" },
            "title": { "type": "string", "description": "可选：更新标题" },
            "start_at": { "type": "string", "description": "可选：更新开始时间" },
            "end_at": { "type": "string", "description": "可选：更新结束时间（空字符串表示清空）" },
            "location": { "type": "string", "description": "可选：更新地点（空字符串表示清空）" },
            "notes": { "type": "string", "description": "可选：更新备注（空字符串表示清空）" }
        },
        "required": ["id"]
    })
}

// ── Git 写操作补全 ──────────────────────────────────────────
