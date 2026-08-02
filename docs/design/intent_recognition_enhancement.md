# 意图识别增强（已归档）

> **ARCHIVED（2026-08，L2 退役后无兼容清理）**：本文档原为 L1/L2 意图管线与门控早退设计。  
> **现行实现**：`session_mode`（Ask/Plan/Act）→ Act 句关键词启发式 → `assess_turn_routing` → ReAct。  
> 确认续接与失败续跑见 `intent_router` / `intent_l0` / `agent_turn::intent::user`。  
> 勿按下文恢复 classifier、`IntentDecision` 早退或 `intent_analysis` SSE。

历史正文已删除；追溯请查 Git 历史。
