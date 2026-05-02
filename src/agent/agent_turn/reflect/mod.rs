//! **R（Reflect）** 步：终答后的反思与结构化规划检查入口（实现见 **`reflect_impl`**；侧向语义 LLM 后与 **`per_coord::final_plan_gate::run_final_plan_gate_semantic_completed`** 对齐）。

mod reflect_impl;

pub(crate) use reflect_impl::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
