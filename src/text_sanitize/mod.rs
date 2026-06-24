//! 面向**用户可见**正文的轻量清洗（聊天、规划摘要等）。
//!
//! 与 **`redact`** 分工不同：本模块不负责日志脱敏或 HTTP 体截断。
//! DeepSeek DSML 解析与物化见 [`crate::dsml`]。

mod assistant_tail;

pub(crate) use assistant_tail::{
    dedupe_plain_assistant_preamble, naturalize_assistant_plan_prose_tail,
    naturalize_plan_step_description,
};

#[cfg(test)]
mod tests;
