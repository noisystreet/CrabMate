# Crate 依赖策略（降低耦合 · 第 1 轮）

## 目标

打断 **`crabmate-workflow → crabmate-internal`**，使领域 DAG（workflow）不再通过审批胶水依赖整棵服务门面。

## 本轮变更

- 新增 **`crates/crabmate-approval`**：`SensitiveCapability`、`ApprovalRequestSpec`、`WebApprovalSink`、`run_web_tool_approval` 等。
- **`crabmate-workflow`** 仅依赖 **`crabmate-approval`**（不再依赖 internal）。
- **`crabmate-internal::tool_approval`** 再导出 approval，并保留 CLI/TUI / `interactive_gate_*`。

## 禁止边（门禁）

见 **`scripts/check-crate-deps.sh`**：

| 包 | 不得依赖 |
|----|----------|
| `crabmate-workflow` | `crabmate-internal` |
| `crabmate-tools` | `crabmate-internal` |
| `crabmate-agent` | `crabmate-internal` |
| `crabmate-approval` | `crabmate-internal` |

## 后续（未做）

- 收窄根包 `lib.rs` 整包 `pub use internal`
- `AppState` 消费面切片
- turn IO / `ProcessHandles` 瘦身
