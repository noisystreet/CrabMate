//! `think` / `redacted_thinking` 标签剥离与内联思维链拆分（与 [`super::plan_fence`] 互补）。

mod filter;
mod scan;

pub(crate) use filter::{
    assistant_thinking_body_and_answer_raw, filter_assistant_thinking_markers_for_display,
};
// 测试与 fixture 校验专用；常规 lib 依赖经 `filter_assistant_thinking_markers_for_display` 间接覆盖。
#[allow(unused_imports)]
pub(crate) use filter::filter_redacted_thinking_for_display;
