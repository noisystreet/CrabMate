//! **R（Reflect）** 步：终答后的反思与结构化规划检查入口（实现见 **`reflect_impl`**；侧向语义 LLM 后走
//! **`per_coord::final_plan_gate::run_final_plan_gate_semantic_completed`**
//! + **`apply_plan_rewrite_count_from_gate`**，与静态终答门控的计数写入对齐）。

mod reflect_impl;

pub(crate) use reflect_impl::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
