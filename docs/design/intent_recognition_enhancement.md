# 意图识别增强（已归档）

> **ARCHIVED（2026-08）**：L1/L2 意图管线、门控早退、`intent_l0` / `intent_router` 续接改写均已拆除。  
> **现行实现**：`session_mode`（Ask/Plan/Act）→ Act 句关键词启发式（最新真实 user 句）→ `assess_turn_routing` → ReAct。  
> 勿按历史设计恢复 classifier 或「有效任务句」改写。

历史正文已删除；追溯请查 Git 历史。
