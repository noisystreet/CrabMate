//! 助手消息 Markdown 渲染（随会话信号刷新 DOM）；超长回复默认全文，可由用户折叠。
//!
//! 展示链与 HTML 出口见 [`crate::message_render`]。
//!
//! ## `assistant_markdown_collapsible_view` 内 **`Effect`** 与快照 Memo
//!
//! - 展示快照由 [`super::helpers::assistant_markdown_display_memo`] 统一计算；**`Effect`** 与 **`class=`** / **`Show`** 共享同一 **`Memo`**，避免多处重复 `sessions.with`。
//! - **`Effect`** 依赖 **`snapshot_memo.get()`**、**`collapsed_long_assistant_ids`**、**`markdown_render`**（Markdown 开关影响流式阶段 `md_on`）；Memo 内已跟踪 **`sessions` / `active_id` / `locale` / `apply_*` / `stream_text_overlay`**。
//! - **`request_animation_frame`**：合并同一帧内多次内容更新；写入回调里对 **`NodeRef`** 使用 **`get_untracked`**，DOM 写入不进入信号依赖图。
//! - **流式节流**：助手仍处于 **`is_loading`** 时，两次写入回答区 DOM 至少相隔约 **40–72ms**（动态检测帧耗时，见 **`adaptive_stream_interval`**），高频 SSE 片段由尾随定时器刷新；世代门禁防止尾随回调覆盖终态 Markdown。**终态**在同一段同步逻辑内 **`innerHTML` 刷新**并清空队列，避免再等一帧或遭陈旧 rAF 干扰。
//!
//! 当前活动会话中按 `message_id` 取展示快照见 [`helpers::snapshot_assistant_message_for_mid`]，
//! 与 [`helpers::assistant_markdown_display_memo`]、折叠条、`class=` 分支共用。

mod helpers;
mod md_answer_effect;
mod view;

pub use view::{AssistantMarkdownCollapsibleWire, assistant_markdown_collapsible_view};
