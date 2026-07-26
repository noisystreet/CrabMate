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

## 第 3 轮：AppState 消费面切片

- 新增 **`WebChatJobAppFacet`**（`web/app_state`）：会话落盘 + 审批表 + `ProcessHandles`。
- **`WebChatJobEnvelope.app`** 由 **`Arc<AppState>`** 改为 **`WebChatJobAppFacet`**；LTM 索引走已有 **`WebChatQueueDeps.long_term_memory`**。
- 会话 load/save/truncate/delete/count 实现迁至 **`AppStateConversationRuntime`**；`AppState` 保留薄委托供 HTTP handler。

## 禁止边（门禁）

见 **`scripts/check-crate-deps.sh`**：

| 包 | 不得依赖 |
|----|----------|
| `crabmate-workflow` | `crabmate-internal` |
| `crabmate-tools` | `crabmate-internal` |
| `crabmate-agent` | `crabmate-internal` |
| `crabmate-approval` | `crabmate-internal` |

## 后续（未做）

- turn IO / `ProcessHandles` 瘦身
- handler 侧 `FromRef` / 更细 facet（upload、config_reload 等）
