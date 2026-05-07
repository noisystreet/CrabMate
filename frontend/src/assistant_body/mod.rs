//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复默认全文，可由用户折叠。
//!
//! 展示链与 HTML 出口见 [`crate::message_render`]。
//!
//! ## `assistant_markdown_collapsible_view` 内 **`Effect`** 与 **`get_untracked`**
//!
//! - **顶层**对 `sessions`、`active_id`、`collapsed_long_assistant_ids`、`locale`、`markdown_render`、
//!   `apply_assistant_display_filters` 逐一 **`.get()`**，保证任一变化都会重跑绘制逻辑。
//! - 在 **`sessions.with(|list| …)`** 内对 `active_id` / `locale` / `apply_*` 使用 **`get_untracked`**，
//!   避免在同一响应式子作用域内重复登记与外层相同的依赖（减少冗余追踪边）；取值仍与本次 Effect 调度一致。
//! - **`request_animation_frame`**：合并同一帧内多次内容更新；写入回调里对 **`NodeRef`** 使用 **`get_untracked`**，DOM 写入不进入信号依赖图。
//! - **流式节流**：助手仍处于 **`is_loading`** 时，两次写入回答区 DOM 至少相隔约 **72ms**（常量见 **`view.rs`**），高频 SSE 片段由尾随定时器刷新；完成后立即走 Markdown 终态渲染。
//!
//! 当前活动会话中按 `message_id` 取展示快照见 [`helpers::snapshot_assistant_message_for_mid`]，
//! 与折叠条、`class=` 分支共用，避免多处重复 `find`。

mod helpers;
mod view;

pub use view::assistant_markdown_collapsible_view;
