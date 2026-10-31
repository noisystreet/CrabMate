[
        builtin_tool! {
            name: tool_names::REMINDER,
            args: args::ReminderArgs,
            category: Basic,
            tags: [],
            runner: runner_reminder_try,
            summary: none,
            description: "提醒事项（持久化到工作区的 `.crabmate/reminders.json`）。**`action` 必填**：`list` 列出提醒（默认不含已完成，可选 `include_done` / `future_days`，**只读**）；`add` 新增（`title` 必填，可选 `due_at`，支持 RFC3339 或 `YYYY-MM-DD HH:MM` / `YYYY-MM-DD`）；`update` 按 `id` 改 `title` / `due_at` / `done`（`due_at` 传空串表示清空）；`delete` 按 `id` 删除；`complete` 按 `id` 标记完成。`update` / `delete` / `complete` 必须传 `id`。除 `list` 外均写盘。与本 `action` 无关的字段会被忽略。",
        },
        builtin_tool! {
            name: tool_names::EVENT,
            args: args::EventArgs,
            category: Basic,
            tags: [],
            runner: runner_event_try,
            summary: none,
            description: "日程事件（持久化到工作区的 `.crabmate/events.json`）。**`action` 必填**：`list` 列出日程（可选 `year` / `month`（1–12）/ `future_days` 过滤，**只读**）；`add` 新增（`title` 与 `start_at` 必填，`end_at` / `location` / `notes` 可选）；`update` 按 `id` 改任意字段（`end_at` / `location` / `notes` 传空串表示清空）；`delete` 按 `id` 删除。`update` / `delete` 必须传 `id`。除 `list` 外均写盘。与本 `action` 无关的字段会被忽略。",
        },
]
