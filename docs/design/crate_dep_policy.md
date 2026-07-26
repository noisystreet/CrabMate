# Crate 依赖策略（降低耦合）

## 目标

打断 **`crabmate-workflow → crabmate-internal`**，并收窄根包把 internal 整包升格为公共 API 的做法，使领域 DAG 与稳定对外面分离。

## 第 1 轮：审批 crate

- 新增 **`crates/crabmate-approval`**：`SensitiveCapability`、`ApprovalRequestSpec`、`WebApprovalSink`、`run_web_tool_approval` 等。
- **`crabmate-workflow`** 仅依赖 **`crabmate-approval`**（不再依赖 internal）。
- **`crabmate-internal::tool_approval`** 再导出 approval，并保留 CLI/TUI / `interactive_gate_*`。

## 第 2 轮：根包 `lib.rs` 再导出

- 原 `pub use crabmate_internal::{…}` 改为 **`pub(crate) use`**：内部模块仅包内可见（`crate::tools` 等路径不变）。
- 去掉根上未再经 `crate::` 引用的再导出（如 `cargo_metadata`、`dynamic_tools`、`health_dep_compat`）；需用时直接依赖 **`crabmate-internal`** 或走已有工具路径。
- 显式保留 **`pub use crabmate_internal::tool_sandbox`**（`main` 的 `tool-runner-internal`）。
- 稳定对外面仍为显式 `pub use`：`run_agent_turn`、`load_config`、`build_tools*`、`ProcessHandles`、`ChatCompletionsBackend`、CLI 解析符号等。

## 禁止边（门禁）

见 **`scripts/check-crate-deps.sh`**：

| 包 | 不得依赖 |
|----|----------|
| `crabmate-workflow` | `crabmate-internal` |
| `crabmate-tools` | `crabmate-internal` |
| `crabmate-agent` | `crabmate-internal` |
| `crabmate-approval` | `crabmate-internal` |

## 后续（未做）

- `AppState` 消费面切片
- turn IO / `ProcessHandles` 瘦身
