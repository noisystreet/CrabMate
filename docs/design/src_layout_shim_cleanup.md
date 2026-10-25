# `src/` 布局收敛 —— 转发壳与重复模块清理

> **状态**：P0 **已实施**（`cargo test --all-features` 1976 项全通过、`pre-commit run --all-files` 全绿）；P1–P3 待办。
> **范围**：仅 `src/` 内部模块布局与模块可见性；**不改** HTTP 路由、JSON 字段、SSE 帧、配置键、CLI 参数。
> **关联**：[`crates_io_single_package.md`](./crates_io_single_package.md)（单包化决策）、[`agent_turn_split.md`](./agent_turn_split.md)（P1 对象）、[`web_host_p5_placement.md`](./web_host_p5_placement.md)（`cm_web_host` 边界）。
> **非目标**：回退 `cm_` 前缀；删除 `agent` / `config` / `llm` / `sse` / `types` 五个组合面模块名。

---

## 1. 背景与根因

仓库曾是 Cargo workspace（根包 + 约 19 个 `crates/*`）。按 [`crates_io_single_package.md`](./crates_io_single_package.md) 合并为单包 `crabmate` 后，原 crate 名以 **`cm_` 前缀模块**形式保留（`cm_types` / `cm_sse_protocol` / `cm_api_contract` / `cm_web_host` / `cm_tools` / `cm_agent` / `cm_llm` / `cm_config` / `cm_workflow` / `cm_memory` / `cm_mcp` / `cm_internal` / `cm_runtime` / `cm_benchmark` / `cm_approval` / `cm_display_rules` / `cm_turn_layout` / `cm_chat_export`）。

同时，为兑现 `src/lib.rs` 文档承诺的「`server` feature 下**组合面**模块名 `agent` / `config` / `llm` / `sse` / `types` 的**存在**」，旧名目录被保留为**兼容门面**。

结果：**同一职责存在三套物理位置** —— 真源在 `cm_*`，旧名门面在根层，而迁移过程中部分文件又在中途留了第二层转发壳。这是当前布局可读性问题的唯一主因，**不是**目录层级过深。

### 1.1 层级不是主要矛盾（P0 前基线）

| 深度（`src/` 之下） | 目录数 |
|---|---|
| 0（根层） | 29 |
| 1 | 47 |
| 2 | 30 |
| 3 | 6 |
| 4 | 1 |
| 5 | 1 |

合计 115 个目录、728 个 `.rs`。89% 的目录在 ≤2 层；最深仅 5 层（`agent/agent_turn/host/execute/tools/serial`）。因此「减少目录层级」的收益有限，**合并重复职责**才是主线。

### 1.2 真正的问题：转发链与同名双份

| 位置 | 现象 | P0 是否处理 |
|---|---|---|
| `cm_api_contract` → `cm_web_host/http_types` → `web/http_types` | 一次 `use` 穿 3 个模块；中间一层 6 个文件是 3 行纯壳 | ✅ 已收敛为 1 跳 |
| `runtime/cli_exit.rs` 与 `cm_runtime/cli_exit.rs` | 两份 39 行同名文件（一为壳、一为真实现） | ✅ 已合并 |
| `cm_chat_export/` 与 `cm_runtime/chat_export.rs` | 后者再导出前者全部符号再加落盘 IO，命名几乎相同 | ✅ 已改名 |
| `cm_tools/tools/mod/`、`src/lib/` | 目录名仅为绕开 `mod.rs` / `lib.rs` 命名冲突而存在 | ✅ 已归位 |
| `agent/agent_turn/` 与 `cm_agent/agent_turn/` | 一个逻辑模块在两个顶层目录各叫一次 `agent_turn` | ⬜ P1 |
| `crate::tools` | 解析链 `lib_server → cm_internal → cm_tools::tools`，三级别名 | ⬜ P2 |
| `cm_tools/tools/` | 66 个平铺 `.rs` + 763 行 `mod.rs`（63 条 `mod` 声明） | ⬜ P3 |
| 42 个 `.inc.rs`（5521 行） | `include!` 文本分片，用于绕开 `fn-nloc-ratchet`，属「假模块」 | ⬜ P3 |
| `#[path = "loop/mod.rs"] mod turn_loop;` | `loop` 是 Rust 关键字，被迫加一层间接 | ⬜ P1 |

---

## 2. 阶段划分

| 阶段 | 内容 | 行为变化 | 需同步 `docs/开发文档.md` |
|---|---|---|---|
| **P0** ✅ | 删纯转发壳；合并同名双份；tests 目录归位 | **无** | 否 |
| P1 | 合并 `agent_turn` 双树为单目录（`engine/` + `policy/`） | 无（纯搬家） | 是 |
| P2 | 拆 `cm_internal`；`crate::tools` 收敛为一级别名 | 无 | 是 |
| P3 | `cm_tools/tools` 按域分组；`.inc.rs` 收敛为真子模块 | 无 | 否（无顶层模块增减） |

P0 之所以可以做到「无行为变化」，是因为它只删除**转发层**：删除前后，所有符号的定义位置、路径可见性、序列化形状均不变。

---

## 3. P0 改动清单（已实施）

### 3.1 HTTP 契约转发链：3 跳 → 1 跳

**事实基线**（`cm_web_host/http_types/`，11 文件 380 行）：

- 纯壳 6 个：`api.rs`(3) / `chat.rs`(3) / `chat_keys.rs`(3) / `tool_jobs.rs`(3) / `workspace.rs`(3) / `tasks.rs`(3)
- 真定义 4 个：`github.rs`(11) / `skills.rs`(25) / `limits.rs`(174) / `tools.rs`(140)

**已做**：

1. 删除上述 6 个纯壳；`cm_web_host/http_types/mod.rs` 只剩 `github` / `limits` / `skills` / `tools`。
2. 引用直连真源（共 9 处 + `limits.rs` 的 2 处 `super::` 相对引用）：

| 引用点 | 原 | 新 |
|---|---|---|
| `web/http_types/chat.rs` | `cm_web_host::http_types::api::…` | `cm_api_contract::api::…` |
| `web/http_types/chat.rs` | `cm_web_host::http_types::chat::…` | `cm_api_contract::chat::…` |
| `web/http_types/tool_jobs.rs` | `cm_web_host::http_types::tool_jobs::*` | `cm_api_contract::tool_jobs::*` |
| `web/http_types/workspace.rs` | `cm_web_host::http_types::workspace::*` | `cm_api_contract::workspace::*` |
| `web/http_types/tasks.rs` | `cm_web_host::http_types::tasks::TasksData` | `cm_tools::workspace::tasks_side::TasksData` |
| `web/http_types/validation.rs` | `cm_web_host::http_types::chat_keys::…` | `cm_api_contract::chat_keys::…` |
| `web/workspace/projects.rs`、`clone_stream.rs` | `cm_web_host::http_types::workspace::…` | `cm_api_contract::workspace::…` |
| `web/http_types/memory.rs`（文档注释） | `cm_web_host::http_types::api::ApiError` | `cm_api_contract::api::ApiError` |
| `cm_web_host/http_types/limits.rs` | `super::chat` / `super::workspace` | `crate::cm_api_contract::chat` / `::workspace` |

> 实施中发现：`limits.rs` 用的是 `super::chat` / `super::workspace` **相对**路径，仅按绝对路径 grep 会漏。删除转发壳时必须同时检查 `super::` 引用。

3. `web/http_types/` 的 4 个壳（`skills` / `tasks` / `tool_jobs` / `workspace`）**保留**，但改为 1 跳直连真源 —— handler 侧约 40 处 `use crate::web::http_types::…` 不动。

**为什么没一步删完 `web/http_types` 的壳**：那会波及 ~40 处 handler import，P0 要保持「删除转发层」的单一语义与可审阅规模。这一层已从 2 跳降到 1 跳，收益已兑现；彻底删除留待 P1。

### 3.2 `cli_exit` 双份合并

**事实**：`runtime/cli_exit.rs`（39 行）= `pub use crate::cm_runtime::cli_exit::*;` + `classify_model_error_message`；`cm_runtime/cli_exit.rs`（39 行）= `EXIT_*` 常量 + `CliExitError` 真定义。

**落位决策与初稿不同，按依赖方向修正**：`cm_runtime/mod.rs` 明文约束「各模块无 `crabmate` 根包依赖，可独立编译」。把依赖 `crate::agent_errors` 的 `classify_model_error_message` 塞进 `cm_runtime` 会破坏这条叶子性质，故改置 `cm_internal::agent_errors`（它本就持有 `is_quota_or_rate_limit_llm_message`），依赖方向 `cm_internal → cm_runtime` 正确。

**已做**：

1. `classify_model_error_message` 与其两项单元测试移入 `cm_internal/agent_errors.rs`（引用 `crate::cm_runtime::cli_exit::EXIT_*`）。
2. 删除 `runtime/cli_exit.rs`；`runtime/mod.rs` 的 `pub mod cli_exit;` 改为 `pub use crate::cm_runtime::cli_exit;`。
3. `lib_server.rs` 的 crate 根再导出拆成两行，`crate::classify_model_error_message` 路径保持可用：
   ```rust
   pub use runtime::cli_exit::{CliExitError, EXIT_GENERAL, …};
   pub use agent_errors::classify_model_error_message;
   ```
4. `runtime/cli/commands.rs` 的 `crate::runtime::cli_exit::…` 引用未动。

### 3.3 `chat_export` 命名消歧

**事实**：`cm_chat_export/`（435 行）= 契约 schema + Markdown 分段，**无 I/O**，属 `protocol` feature；`cm_runtime/chat_export.rs`（362 行）= 再导出上述全部符号 + 写 `<workspace>/.crabmate/exports/`。

两者是**契约**与**落盘**两层，分层本身正确；问题是文件同名，读代码无法判断该看哪个。

**已做**：`cm_runtime/chat_export.rs` → `cm_runtime/session_export.rs`；10 处 `crate::runtime::chat_export::…` 引用改名。**未合并**两个模块（分层有效）。

### 3.4 tests 目录归位

两处目录的存在理由仅是绕开命名冲突；`#[path]` 已足够表达，目录本身无信息量：

| 目录 | 原挂载 | 现 |
|---|---|---|
| `src/cm_tools/tools/mod/tests.rs` | `#[path = "mod/tests.rs"] mod tests;` | `src/cm_tools/tools/tests.rs` + `mod tests;` |
| `src/lib/tests.rs` | `#[path = "lib/tests.rs"] mod tests;` | `src/tests.rs` + `mod tests;` |

两个空目录 `src/lib/`、`src/cm_tools/tools/mod/` 已删除。

---

## 4. 不变式（已核对）

1. `src/lib.rs` 的 6 个 `protocol` 模块 + `lib_server.rs` 的顶层 `mod` 清单**未变**。
2. `crate::web::http_types::*`、`crate::runtime::cli_exit::*`、`crate::classify_model_error_message` 等既有路径仍可解析。
3. 无 `#[serde]` / 路由 / SSE / 配置键 / CLI 参数改动。
4. 无新增依赖；无 `unsafe`；无 `unwrap()` / `expect()` 进入生产路径。

## 5. 验证（已执行）

```bash
cargo check   --all-targets --all-features   # ok
cargo clippy  --all-targets --all-features -- -D warnings   # ok
cargo test    --all-features                 # 1976 passed
pre-commit run --all-files                   # 全绿（含 lizard / fn-param / fn-nloc / taplo / typos / docs link）
```

按 `AGENTS.md`「跨模块类型 / `src/lib.rs` / 公开 API」口径执行了**完整** `cargo test`。

## 6. 结果

| 指标 | P0 前 | P0 后 | 变化 |
|---|---|---|---|
| 目录数 | 115 | 113 | −2 |
| `.rs` 文件数 | 728 | 721 | −7 |
| HTTP 契约 `use` 跳数 | 3 | 1 | −2 |
| 同名双份模块 | 3 组 | 0 组 | −3 |
| `#[path]` 绕名 hack | 3 处 | 1 处（`turn_loop`，P1 处理） | −2 |

## 7. 后续阶段要点（未实施）

- **P1**：`agent_turn/` 合并为单一目录，`engine/`（现 `agent/agent_turn/{host,loop,plan_reflect}`）与 `policy/`（现 `cm_agent/agent_turn/*`）作子目录；顺带解掉 `#[path = "loop/mod.rs"]`，并删除 `web/http_types` 的 4 个残留壳（约 40 处 handler import）。须保留 `agent_turn_split.md` 定义的「纯函数不依赖 IO」分层——用子目录表达，而非顶层目录名。需同步 `docs/开发文档.md`。
- **P2**：`cm_internal`（18 文件）既是别名表、又转发 12 个 `cm_tools` 模块、又有自身职责（`tool_registry` / `tool_jobs` / `tool_sandbox` / `terminal_session` / `user_data` / `context_bootstrap` / `observability` / `health`）。按职责拆分为工具基建 / 上下文装配 / 可观测三块，并把 `crate::tools` 收敛为一级别名。需同步 `docs/开发文档.md`。
- **P3**：`cm_tools/tools/` 40 个残留散文件按域分组（`command*.rs` 已达 10 个）；42 个 `.inc.rs` 中仅 `tool_specs_registry/specs/`（20 个，表驱动、语义自洽）保留，其余转真子模块。
