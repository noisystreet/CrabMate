//! **R（Reflect）** 步：终答后的反思与结构化规划检查入口。

mod reflect_impl;
mod reflect_semantic;

pub(crate) use reflect_impl::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
