# SDK 面与命令薄封装（设计草案）

**状态**：**草案；S5-a 已实施（#1274 / #1275）；S1-a 与 S1-b 的 feature 解绑已实施（#1279）；S1-b 的 `SessionStore` 端口（#1284 / #1286）、S3-a 的审批端口（#1280）与 S3-b 的 `PolicyEngine` 端口（#1282）已实施；S2 的 S2-0 / S2-1（#1288）、S2-2（#1291）、S2-4（#1290）已实施，S2-3 核销放弃；S3-b 的 `ApprovalBroker` 端口收窄已实施（5 → 4 方法）**。S4 / S5 / S6 未开工。P1–P7 现状证据见 §2 与 §10.1；§10.2 原列的三条「实施前必查」已于 2026-11-04 核销（§10.3），Q1–Q4 已决（§11）。原先唯一带**硬截止**的 **S5-a**（§12）已解除：`cli_run.rs` **912 → 493** 行（file 棘轮上限 920，余量 **427**）。S2-0 顺带把 `run_agent_turn` 从 **nloc 197** 降到 **177**，S2-4 再降到 **185**（fn 棘轮上限 200）。
**受众**：维护者、Client（Web / `crabmate-tui`）维护者、贡献者。
**语言**：中文。
**维护**：实施或废弃某一切片时，同步修订本文头部状态、[`docs/design/README.md`](README.md) 索引与 [`docs/待办清单.md`](../待办清单.md)。跟踪 Issue：[#1272](https://github.com/noisystreet/CrabMate/issues/1272)（索引与决策记录，不承载改动）。

---

## 1. 问题

目标是两件事：

1. 把几个模块整理成**清晰、稳定的 SDK 面与接口**；
2. **命令只做薄封装**，不在 CLI 层承载业务编排。

现状不是「没做解耦」——`protocol` / `server` feature 切分、`cm_*` 模块化、`scripts/check-crate-deps.sh` 禁边、`AppState` facet、`TurnRunner` 注入点都已落地。缺的是最后一层：**可复用的单位仍是「模块 + `pub(crate)` 再导出」，而不是「owned 的 SDK 类型 + trait」**。命令层因此只能自己拼装，越拼越厚。

## 2. 现状证据（可核对）

度量基线：单包 `crabmate`，`src/` **773** 个 `.rs` / **189 534** 行（`find src -name '*.rs' | wc -l`；`… -print0 | xargs -0 cat | wc -l`），核对于 `main` `cb31eb2e`。

| 事实 | 位置（以符号为准） | 核对 |
|---|---|---|
| `protocol` 六个模块与 `server` 组合面已按 feature 分离 | `src/lib.rs:29-43` | ✅ |
| 根上一次性 `pub(crate) use` `cm_internal` 的 **27** 项 | `src/lib_server.rs:33` | ✅ 逐项数 |
| `RunAgentTurnParams` 分 6 组，其中**只有 3 组带 `'a`**（`shared` / `session` / `transport`），另 3 组**已经 owned**（`llm` / `attach` / `obs`） | `src/lib_server.rs:179` | ✅ |
| `WebChatStreamBuildArgs` **25** 个平铺字段 / `WebChatJsonBuildArgs` **22** 个 | `src/lib_server.rs:190` / `:223` | ✅ |
| `TurnRunner` 注入点的参数袋仍是 `RunAgentTurnParams<'_>` | `src/turn_runner.rs:19` | ✅ |
| `conversation_store` 是 **13 个 `pub fn` 收 `&Connection`**（`migrate`:51、`open_file`:137、`load`:210、`save_if_revision`:307、`set_active_state`:389、`save_with_archive_if_revision`:427……），且被 `web` feature 门控 | `src/conversation_store.rs`；`src/lib_server.rs:53` | ✅ |
| `cm_internal` 暴露 **25** 个 `pub mod` + 多组再导出 | `src/cm_internal/mod.rs:11-43` | ✅ |
| 全仓 **66** 个文件引用 `crate::cm_internal`，其中**外部（非 `cm_internal/` 内）20 个** | `grep -rln crate::cm_internal src --include=*.rs` | ✅ |
| 外部 20 个的分布：`web/` 6、`src/` 根 4、`agent/agent_turn/host/` 系 5、`chat_job_queue/worker` 2、`user_data` 1、`cm_llm` 1、`cm_config` 1 | 同上 | ✅ 决定 S4 的拆分粒度，见 §10 |
| 下层反向依赖 **只有 1 处真实 `use`**：`cm_llm` → `cm_internal::user_message_file_refs` | `src/cm_llm/outbound_workspace_images.rs:6` | ✅ 附注：`src/cm_config/xdg.rs:1` 也命中 grep，但那只是 doc comment 提到路径，**不是**依赖——按「grep 命中数」下结论会高估 |
| `tool_job_registry` 以具体类型出现在回合入参袋 | `src/agent/agent_turn/host/params.rs:100` | ✅ |
| clap 的 `Commands` 有 **14** 个变体（`Serve`/`Bench`/`Config`/`Doctor`/`WebBearer`/`Models`/`Probe`/`SaveSession`/`ToolReplay`/`SseReplay`/`Mcp`/`Plugin`/`Workflow`/`E2e`），解析后摊平成 `ParsedCliArgs` 的 **24** 个平铺字段 | `src/cm_config/cli/definitions.rs:422` / `:493` | ✅ |
| `cli_run.rs` 用 **13** 字段的 `EarlyCliDispatch` + **11** 个 `try_early_*` + 一处 `ExtraCliCommand` match 重新分派 | `src/cli_run.rs:100`、`:822` | ✅ |
| `cli_run.rs` 曾达 **912** 行（file 棘轮上限 **920**），只剩 **8 行**余量；S5-a 后 **493** 行（余量 **427**） | `wc -l src/cli_run.rs` | ✅ 当时不是「逼近」，是**随时会撞线**；见 §12 |
| **拆分已有先例但没拆完**：`cli_run.rs` 用 `#[path]` 挂了 `cli_run_serve.rs`（拆分前 **730** 行；S5-a 后 **410** 行，见 §12.2）与 `cli_run_session.rs`（**135** 行），但 serve 装配（`build_serve_runtime_state`、`spawn_uploads_cleanup`、`build_serve_shutdown_signal`、两个 feature 变体的 `run_serve_branch`）**已由 S5-a（#1274）搬进 `cli_run_serve_branch.rs`**，主文件 912 → **493** 行 | `grep -n '#\[path' src/cli_run.rs`；`wc -l src/cli_run_serve.rs` | ✅ S5-a 已完成——当初不能直接搬进 `cli_run_serve.rs`（会到 950+ 行），故另开模块，见 §12.2 |
| `server` 面承诺的是「模块**存在**」，不是签名 | `docs/design/crates_io_single_package.md:103` | ⚠️ 该行号未复核 |
| 协议层有分类器但**无生产消费者**——文件头自述「刻意保留的语义对齐锚点」并带 `#![allow(dead_code)]` | `src/cm_sse_protocol/sse/line.rs:1-8` | ✅ |
| 禁边门禁只覆盖 5 个模块，且只针对 `cm_internal` | `docs/design/crate_dep_policy.md:42-51` | ⚠️ 该行号未复核 |

## 3. 问题清单

| # | 问题 | 影响 |
|---|---|---|
| P1 | `cm_internal` 是事实上的 god module（**20** 处外部引用、**25** 个子模块） | 新能力默认落这里；禁边只挡 5 个模块 |
| P2 | 回合执行面没有 owned 请求/事件模型 | **25/22** 字段的 `WebChat*BuildArgs` 是「参数袋装不下」的补偿；第三个宿主会再复制一份 |
| P3 | 工具执行面没有端口（trait），靠具体类型耦合 | 执行面 **trait 签名**直接收具体类型：`src/agent/agent_turn/host/execute/tool_execution_trait.rs:24` 的 `pub web_tool_ctx: Option<&'a WebToolRuntime>`。（2026-11-04 核对：`src/cm_tools/` 内并无 `web_tool_ctx` / `tool_job_registry` 引用——渗漏点在 trait，不在 `tool_registry` 签名，见 §10.3） |
| P4 | CLI 丢失了 `Commands` 枚举的形状：enum → 平铺 Option 汤 → if 链 | 加一个子命令必须改中心文件；`cli_run.rs` **912/920 行**——下一个碰它的 PR 就会撞棘轮 |
| P5 | 会话存储绑死在 `web` feature | `save-session` / `bench` / 未来终端无法复用同一存储 |
| P6 | 承诺面承诺「模块存在」 | semver 约束不到签名，下游一改内部路径就断但「合法」 |
| P7 | 线协议只有服务端编码器 + 无消费者分类器 | 集成方必须重写 parser |

## 4. 目标架构

四个根级 owned 面 + 一层端口 + 收敛后的 `internal`：

```
bin / serve / bench / cm_ctl
        │  薄命令层：clap → CliCommand，每命令 ≤100 行，只调根级 API
        ▼
crabmate::{  TurnEngine / TurnRequest / TurnEvent / TurnOutcome   ← 回合执行面
             ToolHost / ToolOutcome / ToolCtx                     ← 工具执行面
             SessionStore / Conversation / Revision               ← 会话存储面
             LlmGateway                                           ← LLM 面
             prelude::*                                           ← 常用组合
          }
                 │  实现依赖（不对外）
        ports：PolicyEngine / ApprovalBroker / ToolSandbox / EventSink
                 │
        internal::*（pub(crate)）：tool_host | context | data | obs | jobs
```

分层判据（可写进禁边脚本）：**承诺面 = crate 根上显式 `pub use` 的那一组类型；`internal::*` 不得出现在根上任何 `pub use` 的签名里。**

### 4.1 为什么不用 `sdk::` 前缀

早期草案写的是 `sdk::agent` / `sdk::tools` / `sdk::session` / `sdk::llm` 四个伞形模块，**已决定去掉**：

| 事实 | 位置 | 含义 |
|---|---|---|
| 根上已公开 `pub mod agent;` 与 `pub mod llm;` | `src/lib_server.rs:29`、`:55` | `sdk::agent` / `sdk::llm` **直接撞名**，读者得记「哪个 `agent` 才是承诺面」——前缀本想省认知税，结果制造了它 |
| 根上已有扁平 re-export 惯例：`pub use llm::{ChatCompletionsBackend, StreamChatParams, …}`、`pub use crate::cm_config as config`、`pub use crate::cm_types as types` | `src/lib_server.rs:48-50`、`:532` | 「公开模块 + 根上扁平导出」**已经在跑**；`sdk::` 会在 `crabmate::llm::X` 之外再造 `crabmate::sdk::llm::X` |
| `tools` 亦近名占用（`pub use tools::{build_tools, …}`） | `src/lib_server.rs:543-544` | 同上 |

理由：**Rust 的命名空间惯例是标记「要藏的」，不是「要露的」**——公开面天然是默认，需要提醒的只有内部。所以 `internal::` 保留（标记内部），根上不加伞；承诺面由 §11 Q2 的显式清单（rustdoc 白名单 / `cargo public-api`）兜住，而不是靠名字。

**反证**：只有未来要并行维护 `v1` / `v2` 契约时才需要版本化模块（`v2::`）。当前没有这个需求——`client-contract-v*` 是仓外 tag，[`crates_io_single_package.md`](crates_io_single_package.md) 也已声明单包承诺。

**顺带的好处**：根级导出让「违禁直出」更显眼。伞形前缀在「有人从内部直出到根」时毫无信号；`internal` 字样一旦出现在根的 `pub use` 上，就一眼可见。

## 5. 接口草案

### 5.1 回合执行面（根级 `TurnEngine` / `TurnRequest` / `TurnEvent`）

```rust
pub struct TurnEngine { /* 内部全 Arc，无生命周期参数 */ }

impl TurnEngine {
    pub fn builder(cfg: std::sync::Arc<AgentConfig>) -> TurnEngineBuilder;
    pub async fn run(&self, req: TurnRequest) -> Result<TurnOutcome, TurnError>;
    pub fn run_stream(&self, req: TurnRequest) -> TurnEventStream;
}

/// 全 owned：可 Clone、可序列化，便于 fixture 与回放。
pub struct TurnRequest {
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub workspace: std::path::PathBuf,
    pub transport: TurnTransport,        // Streaming { .. } | Json | Silent
    pub llm: LlmOverrides,
    pub permission: TurnPermissionStages,
    pub limits: TurnLimits,
}

/// 与 `cm_sse_protocol` 的 `SsePayload` 同构的 Rust 侧镜像（线协议一个字段不改）。
pub enum TurnEvent {
    TextDelta(String),
    ToolCall { .. },
    ToolResult { .. },
    Approval { .. },
    Finished(..),
}
```

要点：

- **可行性已有局部先例**：`RunAgentTurnParams` 的 6 组里，`RunAgentTurnLlmOverrides` / `RunAgentTurnAttach` / `RunAgentTurnObs` **本来就是 owned**（无生命周期），说明「把入参搬成 owned 结构」不需要先消灭全部借用，可以一组一组搬——这是 S2 能把风险拆碎的证据（见 §2）。
- `AgentTurnTransport` / `RunAgentTurnSharedInputs` / `WebChatStreamBuildArgs` / `WebChatJsonBuildArgs` 退为**构造 `TurnRequest` 的内部细节**；旧名可保留 `#[deprecated]` 薄封装（与 `run_command` 的旧 6–7 参入口同一套路）。
- `TurnRunner` 签名改为 `run(&self, req: TurnRequest)`，否则注入点仍在传递 6 组借用袋。
- 收益可测：**25** 字段的 `WebChatStreamBuildArgs` 消失；`chat_job_queue`、`bench`、未来终端走同一条构造路径。

#### 5.1.1 `TurnEvent` 的投影口径（S2-2，ADR）

上面那个 5 变体的 `TurnEvent` 是**草案**。落到代码前必须先把口径钉死，否则会做出一个「看起来存在、但消费方无法安全穷尽」的枚举。本节记录实测事实、决策与映射表。

**实测事实一：线上不是「一个事件流」，是三层。**

| 层 | 类型 | 变体数 | 谁在读 |
|---|---|---|---|
| 线上帧 | `cm_sse_protocol::sse::ag_ui_event::AgUiEvent` | **16** | 前端 `parser_v2.rs`（Client 仓）；`AGENTS.md` 的 `client-contract-v*` 钉的是它 |
| `CUSTOM` 载荷 | `SsePayload` | **22** | 作为 `AgUiEvent::Custom { custom_type, data }` 的 `data` 出现 |
| reducer 输入 | `cm_turn_layout::event::TurnEvent` | **6** | 仅 `cm_turn_layout`（`project.rs` / `replay.rs`），根上无 re-export |

**实测事实二：草案 §7 风险表里「`SsePayload` → `TurnEvent` 单向派生」的前提不成立。** 文本增量**不在** `SsePayload` 里——22 个变体中没有任何 `*Delta` 载荷，`AssistantAnswerPhase` 只是个 `bool` 标志位。真正的文本流是 `AgUiEvent::TextMessageContent { delta }`。所以正确的派生源是**线上帧 `AgUiEvent`**，`SsePayload` 只是它的 `Custom` 子载荷。

**实测事实三：`AgUiEvent` 只有 `Serialize`，没有 `Deserialize`。** 因此服务端侧的投影实现只能基于 `serde_json::Value` 读 `type` / `customType` 字段（这也是 `replay.rs` 现有做法），**不可能**用 `match event { AgUiEvent::.. }` 拿到编译期穷尽性。穷尽性改由「已知 `type` 常量表 + 测试遍历」保证，见下文。

> **决策（ADR）**：`TurnEvent` 的**唯一真源是线上 AG-UI 帧**；SDK 面 `TurnEvent` 是它的**有损投影**；`cm_turn_layout::TurnEvent` 保持独立（reducer 输入），**不合并**。三者关系单向：`AgUiEvent` → `SdkTurnEvent`，且 `cm_turn_layout` 不复用 SDK 面的枚举（两者投影目标不同：一个喂 reducer 算 UI 状态，一个给 SDK 消费方观察回合）。

**映射表 A：AG-UI 帧（16）**

| # | `type` | → `SdkTurnEvent` | 理由 |
|---|---|---|---|
| 1 | `RUN_STARTED` | **丢弃** | 无回合语义（调用方自己知道开始了） |
| 2 | `RUN_FINISHED` | `Finished { status: Ok }` | 唯一的正常终止信号 |
| 3 | `RUN_ERROR` | `Finished { status: Err(..) }` | 错误也是终止；**不**另起 `Error` 变体，避免消费方要匹配两处终止 |
| 4 | `TEXT_MESSAGE_START` | **丢弃** | 消息生命周期 |
| 5 | `TEXT_MESSAGE_CONTENT` | `TextDelta { delta }` | **唯一的文本增量源**（见事实二） |
| 6 | `TEXT_MESSAGE_END` | **丢弃** | 消息生命周期 |
| 7 | `TOOL_CALL_START` | `ToolCall { tool_call_id, name, summary }` | 卡片标题所需字段在 START 帧已齐 |
| 8 | `TOOL_CALL_ARGS` | **丢弃** | 增量参数；`ToolCall` 已带预览 |
| 9 | `TOOL_CALL_END` | **丢弃** | 结束标记 |
| 10 | `TOOL_CALL_RESULT` | `ToolResult { tool_call_id, content, partial }` | **必须带 `partial`**：`tool_output_chunk` 也落到本事件上（见事实四） |
| 11 | `REASONING_MESSAGE_START` | **丢弃** | 思维链不进 SDK 面（见缺口 G2） |
| 12 | `REASONING_MESSAGE_CONTENT` | **丢弃** | 同上 |
| 13 | `REASONING_MESSAGE_END` | **丢弃** | 同上 |
| 14 | `STATE_SNAPSHOT` | **丢弃** | 客户端状态同步，非回合语义 |
| 15 | `STATE_DELTA` | **丢弃** | 同上 |
| 16 | `CUSTOM` | **按 `customType` 分流** → 表 B | |

**实测事实四：`SsePayload` 的 22 个变体只有 17 个走 `Custom`，另 5 个走标准事件——而 `tool_output_chunk` 是其中最危险的一个。**

`cm_sse_protocol::sse::ag_ui_convert` 是唯一的 `SsePayload → AgUiEvent` 转换点：

| `SsePayload` | → AG-UI |
|---|---|
| `StreamEnded` | `RUN_FINISHED` |
| `Error` | `RUN_ERROR` |
| `ToolCall` | `TOOL_CALL_START`（再补 `TOOL_CALL_ARGS` / `TOOL_CALL_END`） |
| `ToolResult` | `TOOL_CALL_RESULT` |
| `ToolOutputChunk` | `TOOL_CALL_RESULT` + `metadata.partial = true` |
| 其余 **17** 个 | `CUSTOM` |

所以 `customType` 全集是 **18** 个（17 个来自 `SsePayload`，加 `orchestration_route`——后者由编码器内部/测试直接构造，不来自 `SsePayload`）。**它与 `SsePayload` 的 22 个不是同一个集合**，按 `SsePayload` 数变体是错的。

**这里有一个必须点明的真实陷阱**：`tool_output_chunk`（长任务的流式输出片段）与 `tool_result`（最终结果）落在**同一个** `TOOL_CALL_RESULT` 上，只靠 `metadata.partial` 区分。若 SDK 的 `ToolResult` 不带 `partial`，消费方会把每一段流式片段都当成最终结果——这是**语义错误**，不是信息缺失。因此 `ToolResult` 变体**必须**携带 `partial: bool`，本片据此落地。

**映射表 B：`customType`（18）**

| `customType` | → `SdkTurnEvent` | 理由 |
|---|---|---|
| `command_approval` | `Approval { command, args, permission_level }` | 唯一需要人工介入的回合内事件 |
| `clarification_questionnaire` | **丢弃 —— 已知缺口 G1** | 见下 |
| `tool_job_finished` | **丢弃** | 后台任务域的完成通知；回合内已由 `TOOL_CALL_RESULT` 覆盖 |
| `tool_running` / `parsing_tool_calls` / `assistant_answer_phase` / `plan_required` / `turn_tool_phase_end` / `stream_draining` / `workspace_changed` | **丢弃** | 纯 UI 状态位（多为 `bool`），不属于回合语义 |
| `turn_segment_start` / `turn_segment_end` / `chat_ui_separator` / `timeline_log` / `thinking_trace` | **丢弃** | 渲染面，喂 `cm_turn_layout` 的 reducer |
| `conversation_saved` / `sse_capabilities` | **丢弃** | 持久化 / 能力协商 |
| `orchestration_route` | **丢弃** | 编排内部路由，不属回合事件 |

**「不投影」的表达方式：三态，不是 `Option` 也不是 `Other` 变体。** 投影结果必须区分两件事——「映射表里**明确丢弃**」与「映射表**漏了**」。前者是决策，后者是缺陷。合成一个 `None` 就又回到 `replay.rs` 现有的静默兜底；留一个 `TurnEvent::Other(Value)` 则会让消费方的 `match` 永久有默认分支，穷尽性保护全部失效。两者都正是 S2-2 要消除的漂移，所以用 `Event(..)` / `Dropped` / `Unknown` 三态。

**已知缺口（不擅自扩面，记录在案）**

- **G1 `clarification_questionnaire`**：问卷是交互面。`Approval` 变体已承载「回合暂停等人」这一类，但把问卷塞进 `Approval` 会让两者的处理逻辑混淆。**本片不扩**——此阶段 SDK 消费方拿不到问卷。
- **G2 `REASONING_MESSAGE_*`（思维链）**：草案 5 变体无位置，且「思维链是否属回合可观察面」本身需要产品判断。**本片不扩**。

> 原先以为的「`tool_output_chunk` 缺口」经核实**不成立**：它已被合流到 `TOOL_CALL_RESULT`，闭合方式是给 `ToolResult` 加 `partial`（见事实四），而非新增变体。这条留在这里，是因为「拍脑袋列缺口」很容易漏掉这种——真实情况往往比缺口清单更复杂。

**穷尽性怎么保证（`AgUiEvent` 没有 `Deserialize` 的前提下）**

1. 代码维护 `AG_UI_KNOWN_TYPES`（16 项）与 `CUSTOM_KNOWN_TYPES`（18 项）两张常量表，两者**互不相交**。
2. 测试遍历两张表的每一项，断言投影结果是 `Event` 或 `Dropped`，**不得落进 `Unknown`**；再断言表长等于各自的变体数。新增变体而忘了更新表 → 测试红。
3. 未知字面量必须落进 `Unknown`（用一个反向测试钉住），证明兜底不是静默的。

### 5.2 工具执行面（`ToolHost` + 三端口）

```rust
pub trait PolicyEngine  { fn decide(&self, call: &ToolCall, ctx: &ToolCtx) -> PolicyDecision; }
pub trait ApprovalBroker{ async fn ask(&self, spec: ApprovalSpec) -> ApprovalOutcome; }
pub trait ToolSandbox   { async fn run(&self, req: SandboxReq) -> ToolOutcome; }

pub struct ToolHost { /* 注册表 + 三端口 + 超时/缓存策略 */ }
impl ToolHost {
    pub async fn execute(&self, call: ToolCall, ctx: ToolCtx) -> ToolOutcome;
}
```

`WebToolRuntime` 降级为 `ApprovalBroker` 的一个**实现**（SSE 审批流），不再出现在 `tool_registry` 的公开签名里。这条同时消掉 P3 和 [`crate_dep_policy.md`](crate_dep_policy.md) 遗留的「减少对 internal `tool_registry` 门面字面依赖」。`ApprovalBroker` 只包现有审批语义，**不新增**任何审批门（与 [`permission_levels.md`](permission_levels.md) §9 MB-4 的结论无关）。

### 5.3 会话存储面（`SessionStore`）

```rust
pub trait SessionStore: Send + Sync {
    fn load(&self, id: &ConversationId) -> Result<Option<LoadedConversation>, StoreError>;
    fn save_if_revision(&self, conv: &Conversation, expect: Revision) -> Result<SaveConversationOutcome, StoreError>;
    fn set_active_state(&self, id: &ConversationId, patch: ActiveStatePatch) -> Result<(), StoreError>;
}
pub struct InMemoryStore;                          // 无 feature
#[cfg(feature = "sqlite")] pub struct SqliteStore; // 现有 rusqlite 实现搬进来
```

现有 **13** 个自由函数（`migrate` / `open_file` / `load` / `save_if_revision` / `set_active_state` / `save_with_archive_if_revision` / `delete_by_id` / `prune` / `count` / `list_all_messages_json` 等）成为 `SqliteStore` 的私有实现；`web` 侧的会话存储调用变成 handler 侧适配层。**关键收益：从 `web` feature 里解绑**。

### 5.4 LLM 面与 `protocol`

- `llm` 已有 `ChatCompletionsBackend`，骨架不动；只需收口成 `LlmGateway::complete` / `stream`，并把 `api::stream_chat` 彻底私有化（现在「勿在 `agent` 直连」只靠文档纪律，改成靠可见性）。
- `protocol` 六个模块**不动**。补两项：`protocol::prelude`，以及把 `classify_agent_sse_line` 提升为官方客户端 API `ChatStreamDecoder`——把 P7 从「故意保留的死代码」变成「有消费者的契约锚点」。注意 [`AGENTS.md`](../../AGENTS.md) 明确该分类器是**刻意保留**的锚点，改造时须同步两处说明（代码注释与 [`SSE协议.md`](../SSE协议.md)）。

## 6. 命令薄封装

现状是**两段有损往返**：clap `Commands`（14 变体）→ `ParsedCliArgs`（24 平铺字段）→ `EarlyCliDispatch`（**13** 字段）→ 11 个 `try_early_*` + 1 个 match。目标：

```rust
// src/cm_cli/mod.rs —— 只有分派，无业务
pub struct CliContext { pub cfg: Arc<AgentConfig>, pub workspace: PathBuf, pub log: Option<PathBuf> }

pub async fn dispatch(args: CliArgs) -> Result<ExitCode, CliError> {
    match args.command {
        Cmd::Doctor            => doctor::run(..).await,
        Cmd::SaveSession(c)    => save_session::run(c, ..).await,
        Cmd::Serve(c)          => serve::run(c, ..).await,
        Cmd::ToolReplay(c)     => tool_replay::run(c, ..).await,
        // …一行一个，无业务逻辑
    }
}
```

三条硬约束（可直接进 pre-commit）：

1. `cm_cli/*.rs` 单文件 ≤ 200 行，**每命令一个模块**；
2. `cm_cli/` **禁止**出现 `crate::cm_internal::*` —— 加进 `scripts/check-crate-deps.sh` 禁边表（与既有 5 条同格式）；
3. `serve.rs` 只允许 `Server::builder(cfg).workspace(..).cors(..).build()?.run(ctrlc).await` —— **现状的装配在 `cli_run_serve_branch.rs`（S5-a 已搬，#1274），仍不在 `cm_web_host::serve` 的 builder 里**：`cli_run.rs` 用 `#[path]` 挂出 `cli_run_serve.rs`（paths / bearer / health）、`cli_run_serve_lock.rs`（instance lock 一族）与 `cli_run_serve_branch.rs`（`build_serve_runtime_state`、`spawn_uploads_cleanup`、`build_serve_shutdown_signal`、两个 feature 变体的 `run_serve_branch`），主文件只剩早退子命令分派。搬去 `cm_web_host::serve` 的 builder 是 S5 的**第二步**，不是 §12 的 S5-a。

另加一条显式二分：**`EarlyCommand`**（doctor / save-session / web-bearer / plugin / workflow-validate，不碰网络）与 **`ServerCommand`**（serve / bench）。这样 11 个 `try_early_*` 归并为一次 `match`，`run_cli_from_parsed` 只剩「建 `CliContext` → dispatch」。

## 7. 分阶段计划

每阶段独立可合并、独立可验证；任一阶段失败可原地回退（都是新增面 + `#[deprecated]` 转接，不改线协议）。

| 阶段 | 内容 | 验证锚点 | 依赖 / 回退 |
|---|---|---|---|
| **S5-a**（先做） | `cli_run.rs` 减行，纯搬移一分为三 | 见 §12.3 | 见 §12.2；**唯一有硬截止的一步** |
| **S1** | 会话存储面从 `web` feature 解绑（P5） | `cargo check --no-default-features --features protocol,sqlite` **真的编译**存储（此前只跑 `--features protocol` 不编译它，证明不了什么）；`--features protocol` 单独不引入存储 | **S1-a ✅**（#1279）：`ConversationBacking` 收拢为唯一分派点、`AppStateConversationRuntime` 变薄委托，`app_state.rs` 920 → 480（业务行）；**S1-b 解绑 ✅**（#1279）：新建 `sqlite` feature、`StructuralCommit` 上移 `cm_types`、模块移出 `lib_server.rs`；**`SessionStore` trait ⏳ 未做**——形态取决于目标（复用 vs 承诺面），见 §7.1 |
| **S2** | `TurnRequest` / `TurnEvent` / `TurnEngine`；旧 BuildArgs 转 `#[deprecated]` 封装（P2） | `fixtures/sse_ag_ui_golden.jsonl`、`src/cm_tools/fixtures/tool_result_envelope_golden.jsonl` **不改即绿** | **无前置**（2026-11-04 已核销：`RunAgentTurnSession<'a>` 只有 3 个字段、**不含** store 句柄 ⇒ S1/S2/S3 互不阻塞）；按「一组一组搬」，每组一次提交。**进度**：**S2-0 / S2-1 / S2-4 ✅**：三项正名已落地（`TurnRequest` / `TurnEngine` / `TurnOutcome`；`RunAgentTurnParams` 留 `#[deprecated]` 别名，因它经 `include!` 落在 crate 根属公开 API）；`TurnOutcome` 已 struct 化为 `{ commit, stats: TurnStats }`，六个聚合字段**全部取自既有账本**（`TurnBudgetCounter` × 5 + `PerTurnFlight::last_assistant_finish_truncated`），**刻意不含「本轮工具调用次数」**（`ToolOutcomeRecorder` 是进程级累计，无单轮维度）；`run_agent_turn` `nloc` 197 → 177 → 185；**S2-2 ✅**：投影口径见 §5.1.1（三层结构 + 映射表 A/B + `ToolResult` 必须带 `partial` + 缺口 G1/G2 + 三态 `Dropped`/`Unknown`），代码落在 `cm_sdk_event`，`replay` 的静默兜底改为 debug 诊断；`messages` owned 化经核销**建议放弃**（`&mut` 仅跨 3 行，且 `ModelContextView::derive` 已 `to_vec()`） |
| **S3** | 工具执行面三端口（`ToolHost`）；`WebToolRuntime` 降为实现（P3） | `command_approval` 相关金样 + `execute_tests` | **S3-a ✅**（#1280）：`ApprovalBroker` 端口落地，执行面签名改为 `Option<&dyn ApprovalBroker>`，`WebToolRuntime` 成为其实现之一，`src/agent/` 内不再出现该具体类型。**S3-b ✅**（#1282/S3-b）：`PolicyEngine` 端口已落地（`cm_agent::permission::PolicyEngine`，执行面签名由 `Option<&PermissionCaps>` 改为 `Option<&dyn PolicyEngine>`，并加「端口 ≡ 自由函数」等价性测试）；**`ToolSandbox` 早已是端口**（`Arc<dyn SyncDefaultSandboxBackend>`，`cm_internal::tool_sandbox`），**`ToolHost` 本体也已存在**（`ToolExecutionHost` + `ToolDispatch`）。**端口收窄已完成**：`ApprovalBroker` 从 5 个方法收到 **4**（`cancel()` 全仓 0 调用点，能力已由 `approval_sink().cancel` 送达，详见 §7.2）；`ports` 命名收口**主动不做**（收益在 S4 的禁边表，单独落只是无消费者的间接层，见 §8）。全程只包现有代码，**不新增审批语义** |
| **S4** | `cm_internal` 拆桶；禁边表改为「上层只准引用 `ports`」（P1） | `scripts/check-crate-deps.sh` 白名单式规则；外部引用数 **20 → 0** | **前置**：先跑 `code_nav call_graph_sketch` 复核分桶假设（§10.2）；回退成本最高，务必最后做 |
| **S5** | CLI 分派表 + `cm_cli/` 禁边 + serve builder（P4） | `crabmate --help` 与 `man/crabmate.1` **无 diff** | 须在 S5-a 之后：余量恢复后才值得做语义重构 |
| **S6** | 承诺面从「模块存在」升级为「类型 + trait 清单」（P6、P7） | rustdoc 白名单 + semver 检查脚本 | ⚠️ 承诺面所在章节未复核（早期稿引「§2.4」但未定位）；改承诺面即关联 Client 的 `client-contract-v*` 钉版本 |

**S3-b 的持有形态口径**（#1282）：`PermissionCaps` 仍是 owned 快照，构造点在 `agent_turn::host::params`（回合参数里一份）；执行面只借用 `&dyn PolicyEngine`。分层因此明确——持有者负责 `project_caps` / min 链，借用方只消费端口，端口实现里不再出现具体类型。端口面按 §7.2 逐个收窄：`net_scope()` 交枚举已改为 `outbound_denied()` / `outbound_allows_any()` / `outbound_prefixes()` 三方法 + 派生的 `outbound_uses_prefixes()`；**S3-b ✅**（`ApprovalBroker` 收窄）：端口 5 → **4**，删掉 0 调用点的 `cancel()`（能力经 `approval_sink().cancel` 送达；详见 §7.2 的核实表）；`ports` 命名收口**主动不做**（收益在 S4 禁边表，单独做只是无消费者的间接层，违反 §8）。`ToolHost` 本体与 `ToolSandbox` 经核销**已经存在**（`ToolExecutionHost` + `ToolDispatch`；`Arc<dyn SyncDefaultSandboxBackend>`），不需要新建。

### 7.1 S1-b 的 `SessionStore` trait 为什么没有跟着解绑一起做

解绑本身已落地（#1279）：新建 `sqlite` feature（`web = ["sqlite"]`）、`StructuralCommit` 从 `agent::model_context_view` 上移到 `cm_types`、`conversation_store` 从 `lib_server.rs` 移到 `lib.rs` 的 `#[cfg(feature = "sqlite")]`。`conversation_store` 现在**结构上**不依赖 Axum 与回合编排——`cargo check --no-default-features --features protocol,sqlite` 会**真的编译**它（59 条 `dead_code` 警告就是它进了编译图、而消费者都在 `server` 的证据），而 `--features protocol` 不会。

trait 没实现，是因为它对应两个互斥目标，产物不同：

| 目标 | 做法 | 代价 |
|---|---|---|
| **复用**：`save-session` / `bench` / 未来终端直接用同一存储 | 把 `ConversationBacking` 从 `web/` 提到顶层；它还依赖 `web::ConversationTurnSeed`，需要一起上移 | 动 `web/` 的类型归属，影响面大于 S1-a |
| **承诺面**：为 S6 准备契约（§5.3 的 `SessionStore` / `LoadedConversation` / `Revision`） | 按 §5.3 定义 owned 请求 / 结果类型，`ConversationBacking` 只作为实现之一 | 需先定 §11 Q2 的「承诺清单」要列哪些项，否则又是「先写 trait 再找消费者」 |

直接照 §5.3 草案签名抽一个 trait 会同时踩两条：它拿 `web` 的类型当参数（污染承诺面），而当前唯一消费者 `AppStateConversationRuntime` 也不需要一个 trait（它持有具体 enum 就够）。**等目标定下来再做，不为「把 S1 打勾」而抽。**

### 7.2 S3-a 的端口面为什么比 §5.2 草案宽

§5.2 草案只写了一个 `async fn ask(&self, spec) -> ApprovalOutcome`。实际动手时，编译器把执行面**真正在用的全部能力**列了出来：审批 `sink`、`turn_job_id`、`out_tx`、`persistent_allowlist`、`cancel`——其中后三项原本是**直接读字段**（`workflow_tool_dispatch` 组装 `WorkflowApprovalMode::Interactive`、`background_job` 取 `out_tx` 的弱引用、`http/gate` 查白名单表）。

于是 `ApprovalBroker` 按**实际消费面**收口，而不是先定一个更窄的 trait、再在调用点 `downcast` 回 `WebToolRuntime`。后者会造出「端口存在但立刻被穿透」的假解耦，与 S1-b 那次「check 通过、目标模块根本没编译」是同一类错误。

代价是端口面偏宽（5 个方法）。这是诚实的：执行面确实需要这些能力。若 S3-b 引入 `ToolHost` 后某些能力只由宿主使用，再逐个从端口上收窄。

**S3-b 核实结果（已收窄为 4）**：逐个点消费点后，`cancel()` 是唯一没有消费者的方法——全仓 **0 个调用点**。而它想提供的能力并没有丢：`approval_sink()` 返回的 `WebApprovalSink` 本来就带着 `cancel: Option<&'a Arc<AtomicBool>>`，审批等待靠它感知「停止」。也就是说**同一条能力开了两条路**，端口因此看起来比实际需要的宽。删掉 `cancel()` 方法、保留 `WebToolRuntime::cancel` 字段（`approval_sink` 仍用它），端口从 5 收到 4。

其余四个都留着，各有真实消费点且语义不重叠：

| 方法 | 消费点 | 为什么不能并 |
|---|---|---|
| `approval_sink()` | 6 处（`workflow_tool_dispatch` + `cm_internal` 审批链）| 交互审批的唯一入口 |
| `turn_job_id()` | 1 处（`background_job.rs`）| 级联取消要写入 `JobRecord.source_turn_job_id` |
| `out_tx()` | 1 处（`background_job.rs`）| 后台任务只发 SSE、不审批；经 `approval_sink()` 取会语义失真 |
| `persistent_allowlist()` | 4 处（审批写入 + `http/gate` 查询）| 「永久允许」是共享状态，不是通道 |

**`ports` 命名收口主动不做**：它的收益在 S4 的禁边表（「上层只准引用 `ports`」），单独做只是加一层无消费者的间接——与 §8「不为看起来干净而搬文件」冲突。S4 开工时一并落，届时有真实的引用面约束替它证明价值。

**当前推荐顺序（2026-11-05 更新）**：S5-a（§12）、S1-a / S1-b（含 `SessionStore` 端口）、S2（全四片）、S3-a / S3-b（含本次端口收窄）**全部已完成**，S1 / S2 / S3 三条线闭环。剩余只有 **S4**（`cm_internal` 拆桶，仍放最后，开工前先跑 `call_graph_sketch`）与 **S6**（改承诺面须同步判断 Client `client-contract-v*`）；S5 的其余部分视 CLI 需求插入。下方「排序依据」保留为那一轮决策的历史，其中「只剩 8 行余量」已不成立。

**排序依据**：

1. **有截止的先做**：`cli_run.rs` 只剩 8 行余量，任何触碰它的 PR 都会撞棘轮——先把 S5-a（只做减行，不做 `cm_cli/` 全面重排）解掉（见 §12）。
2. **风险低的并行做**：S1（纯新增面）与 S3（端口包现有实现）可并行，互不依赖。
3. **收益大但不可逆的留后**：S4 要动 20 个外部引用点，且分桶方案仍未验证，放最后。

> 早期稿的「按 S1→S6 顺序」不建议采用：它把唯一**有硬截止**的一步（S5）排在第五，把回退成本最高的一步（S4）排在第四。

## 8. 非目标

- **不引入 `sdk::`（或任何伞形）命名空间**：公开面就是 crate 根上那组显式 `pub use`，理由见 §4.1。已公开的 `pub mod agent` / `pub mod llm` 保持原样，不再叠一层。
- **不拆多 crate**：`cargo metadata` 显示 workspace 只有 `crabmate` 一个包；[`crate_dep_policy.md`](crate_dep_policy.md) 的 P4/P5 结论（执行面留根包、queue 不迁 web-host）仍成立。拆包换来的税（版本对齐、Client 钉版本、发布流程）大于收益。
- **不动 SSE 线协议**：`SSE_PROTOCOL_VERSION` 是另一条轴。S2/S3 只做 Rust 侧类型化，事件名与字段一个不改（见 [`SSE协议.md`](../SSE协议.md)）。
- **不顺手全仓重命名**：`cm_*` 前缀是切仓锚点，改名留到 S6 之后单独一轮。
- **不为了「看起来干净」做无行为变化的搬文件**（S5-a 例外：它有硬截止）。

## 9. 风险

| 风险 | 说明 |
|---|---|
| 类型化 `TurnEvent` 与 `SsePayload` 双真源 | 必须单向派生（`SsePayload` → `TurnEvent`），或让 `TurnEvent` 成为唯一真源、`SsePayload` 由它编码。二者择一写进 ADR，否则会二次漂移（倾向见 §11 Q1） |
| `#[deprecated]` 过渡期双路径 | 旧 BuildArgs 与新 `TurnRequest` 并存期间，两条路径的行为必须由同一批金样覆盖，否则「新旧不一致」会被藏住 |
| 棘轮上限 | fn nloc ≤ 200 / file ≤ 920。**现状**：`cli_run.rs` 912 仍逼近上限，`cli_run_serve.rs` 经 S5-a 第一步已降到 **391**。所以「把 serve 装配搬进 `cli_run_serve.rs`」这种最直觉的做法在拆 lock 之前会让后者到 950+ 行，必须一分为三（见 §12.2）。任何搬移后都要复测，别只看总行数减少 |
| 与 Client 契约耦合 | S2/S6 触碰 `cm_sse_protocol` 的公开面即关联 `client-contract-v*` 钉版本（见 [`client_contract_versioning.md`](client_contract_versioning.md)），需确认是否同时出 pin |

## 10. 核对状态

### 10.1 已核对（命令可复现）

| 项 | 命令 | 结果 |
|---|---|---|
| 源文件数 / 总行 | `find src -name '*.rs' \| wc -l`；`find src -name '*.rs' -print0 \| xargs -0 cat \| wc -l` | 773 / 189 534 |
| `cm_internal` 引用面 | `grep -rln 'crate::cm_internal' src --include='*.rs' \| wc -l` | 66（内部 46 / 外部 **20**） |
| 外部引用分布 | 同上 + `sed 's#/[^/]*$##' \| sort \| uniq -c` | `web/` 6、`src/` 根 4、`agent/agent_turn/host/` 系 5、`chat_job_queue/worker` 2、`user_data` 1、`cm_llm` 1、`cm_config` 1 |
| `cli_run.rs` 行数 | `wc -l src/cli_run.rs` | **912**（上限 920） |
| `Commands` 变体数 | `awk '/pub enum Commands/,/^}/' … \| grep -cE '^    [A-Z]'` | 14（变体名见 §2） |
| `ParsedCliArgs` 字段 / `EarlyCliDispatch` 字段 / `try_early_*` 数 | 同上手法 | 24 / 13 / 11 |
| `WebChat*BuildArgs` 字段数 | `awk '/pub struct WebChatStreamBuildArgs/,/^}/' \| grep -c 'pub '` | 25 / 22 |
| `RunAgentTurnParams` 各组生命周期 | `grep -E "pub struct <组名>(<'a>)?"` | 3 组带 `'a`、3 组 owned |
| `conversation_store` 自由函数 | `grep -cE '^pub fn ' src/conversation_store.rs` | 13 |
| `classify_agent_sse_line` 消费者 | `grep -rn classify_agent_sse_line src \| grep -v line.rs` | 0（文件头已自述「刻意保留锚点」） |

**核对推翻的两处早期判断**（早期稿的数字因此不可直接引用）：

- 「`RunAgentTurnParams` 全部带 `'a`」→ 实际 3/6 组已 owned ⇒ **S2 的风险低于早期假设**，可按组迁移而不是一次性 owned 化。
- 「外部 19 个、其中 8 个在 `web/`」→ 实际 20 个、`web/` 系 6 个、根 4 个 ⇒ S4 的拆分粒度要按「`web/` 6 + 根 4 + host 系 5」三簇来定，不是「web 为主」。

### 10.2 仍未核对（实施前必查）

- **未运行** `cargo build` / `cargo test` / `pre-commit`；本文所有行为性结论都来自只读阅读与 `grep`。**S5-a 完成后仍未在本文件补记实测**——它的真实验证记录在 §12.2.2 与 #1274 / #1275 的 PR 正文里。
- **`cm_internal` 的分桶方案仍是建议值**：未按实际调用图验证。S4 开工前先跑一轮 `code_nav call_graph_sketch`（或 `cargo modules`）看真实扇出，别按直觉切。
- 原列的「两处行号未复核」「`SessionStore` 与 `ConversationBacking` 是否重复抽象」「S2 与 S1 的耦合」三条**已于 2026-11-04 核销**，见 §10.3。

### 10.3 已核销（2026-11-04，只读核对）

| 原「实施前必查」项 | 核对结果 | 决策影响 |
|---|---|---|
| `SessionStore` 与 `web/app_state.rs` 的 `ConversationBacking` 是否重复抽象 | **确实重复，且不止「两套」**：`ConversationBacking` 已是 `Memory(HashMap)` / `Sqlite(Connection)` 双态门面（`src/web/app_state.rs:211`，`app_state.rs` 与 `app_state_facets.rs` 合计 10+ 处 `match`），`conversation_store` 的 13 个自由函数又在它下面加了一层；且 `mod conversation_store` 被 `#[cfg(feature = "web")]` 门控（`src/lib_server.rs:53`） | **S1 必须拆成 S1-a + S1-b**（见 §7 表）。按原计划「纯新增 trait」会造出第三套存储门面 |
| S2 与 S1 的耦合：`RunAgentTurnSession<'a>` 是否含 store 句柄 | **不含**：`src/lib_server.rs:141` 的 `RunAgentTurnSession<'a>` 只有 3 个字段——`messages: &'a mut Vec<types::Message>`、`effective_working_dir: &'a std::path::Path`、`workspace_is_set: bool` | **不耦合**：§7 表里 S2 的 ⚠️ 解除，S1 / S2 / S3 可并行排序 |
| `docs/design/crates_io_single_package.md:103`（承诺「模块存在」） | **成立**：§2.4「首发 `0.4.0` 的 semver 承诺」表（实测位于 `:95-112`）明确 `server` 面只承诺「组合面模块名的**存在**」+ 根上显式 `pub use` | S6 的前提不变，引用行号改为 `:95-112` |
| `docs/design/crate_dep_policy.md:42-51`（禁边只覆盖 5 个模块） | **成立**：「禁止边（门禁）」表（实测位于 `:42-51`）只列 `cm_workflow` / `cm_tools` / `cm_agent` / `cm_approval` / `cm_web_host` 五条，且只针对 `crate::cm_internal` | S4 的前提不变 |
| §3 P3 说的「`web_tool_ctx` / `tool_job_registry` 泄进 `tool_registry`」 | **结论成立、位置写错**：`src/cm_tools/` 内这两个名字命中 **0** 处；真实渗漏点是执行面 **trait 签名**——`src/agent/agent_turn/host/execute/tool_execution_trait.rs:24` 的 `pub web_tool_ctx: Option<&'a WebToolRuntime>`（`WebToolRuntime` 定义在 `src/cm_tools/tool_runtime.rs:16`） | **S3 的切口改为执行面 trait**，不是 `cm_tools` 内部；P3 行已相应修正 |

## 11. 决策记录（原「待决问题」，Q1–Q4 均已决）

- **Q1：`TurnEvent` 与 `SsePayload` 谁是真源？** — **已决（2026-11-04）：`SsePayload` 是真源，`TurnEvent` 由它单向派生**（`impl From<SsePayload> for TurnEvent`，反向不提供）。
  理由：`SsePayload` 已被 [`SSE协议.md`](../SSE协议.md) + Client `parser_v2.rs` + 金样三方钉死，让它当唯一真源不需要改任何既有测试；反过来让 `TurnEvent` 当真源，等于给线协议引入第二个定义点，§9 首条风险会从「可能」变成「必然」。
  代价：`TurnEvent` 无法表达 `SsePayload` 之外的聚合语义（如「本轮共几次工具调用」）——这类信息放 `TurnOutcome`，不要塞进事件流。
  约束：S2 实施时**不得**新增 `SsePayload` 之外的事件变体；要新事件就先改线协议（含 `SSE_PROTOCOL_VERSION` 与 Client 解析器），再派生。

- **Q2：根级导出是否进 `server` 承诺面？** — **已决（2026-11-04）：进，但用「承诺清单」显式列出**（rustdoc 白名单 + 一份 `pub use` 名单），而不是承诺「根上所有 `pub` 项」这个自动集合。
  理由：不进承诺面则第三方无法依赖，SDK 化的收益基本归零（这正是本文要解决的问题）；但承诺整个自动集合会让 S6 之后每次调整都变成 semver 破坏。
  代价：每次新增公开项都要改那份清单——这是**刻意的**摩擦，防止公开面再次无意识膨胀；也正因如此，§4.1 才不需要前缀来「圈定」公开面。
  约束：清单外的项视为「可改」，但若已在清单内，删除 / 改签名即 breaking，须同步 CHANGELOG 与 Client `client-contract-v*` 判断。

- **Q3：S1 的 `sqlite` feature 新建还是复用 `web`？** — **已决（2026-11-04）：新建 `sqlite` feature，`web` 依赖它**（`web = ["sqlite", ...]`）。
  理由：复用 `web` 等于把「会话存储需要 SQLite」和「进程要跑 HTTP 服务」绑成同一件事，S1 的目的正好是把两者拆开；用 `default = ["web"]` 保证现有构建行为不变。
  代价：多一条 feature 组合要进 CI 矩阵（`--no-default-features --features protocol` 与 `--features sqlite` 两组）；`.pre-commit-config.yaml` 已有 feature-gating 检查，扩它即可。
  顺序：该 feature 在 **S1-b** 落地，**不阻塞 S1-a**（S1-a 只合并存储门面、不动 feature 图）。

- **Q4：Issue 怎么切？**（已决定，记录于此）
  **按切片开 Issue，不做一个大 Issue**：S5-a 先行（见 §12），其余切片各自独立成 Issue；[#1272](https://github.com/noisystreet/CrabMate/issues/1272) 仅作索引与决策记录，本身不承载改动。
  理由：S1–S6 独立可合并，一个大 Issue 会让「完成」永远无法成立，也会诱使在一个 PR 里跨阶段改动，与 GitHub Flow 的小 PR 要求冲突。

## 12. 最小可行切片：S5-a

### 12.1 为什么是它，而不是管线和端口

SDK 化整体（S1–S6）是**改善**，可以慢慢来；`cli_run.rs`（**912** 行 / 上限 **920**）是**阻塞**：下一个想给 `crabmate` 加子命令或改 CLI 的 PR，只要在 `cli_run.rs` 里多写 8 行就会撞上 `fn nloc + rust file lines` 棘轮。届时只有两个选择——为通过门禁而临时放宽上限（把安全阀拆掉），或在功能 PR 里顺手做一次大重构（把小 PR 变成大 PR）。两者都该避免，所以先把行数问题单独解掉。**已解**：S5-a 完成后 `cli_run.rs` **493** 行（余量 **427**），见 §12.2；本节保留为决策记录。

### 12.2 做法：纯搬移，一分为三（**不能一步搬进 `cli_run_serve.rs`**）

实测 `cli_run.rs` **912** 行、`cli_run_serve.rs` **730** 行。serve 装配在 `cli_run.rs` 里合计 **232** 行：`build_serve_runtime_state` 432-520（89）、`spawn_uploads_cleanup` 586-609（24）、`build_serve_shutdown_signal` 714-734（21，web-only）、`run_serve_branch` 的 web 变体 624-712（89）与非 web 变体 737-745（9）。`run_benchmark_batch_if_requested`（764-814，51）**没有 `cfg` 门**，两套构建都在，单独一站。**直接搬进 `cli_run_serve.rs` → 约 950 行，立刻撞上限**。按下面的分配搬（行范围已实测，见 §12.2.1）：

| 目标文件 | 收什么 | 状态 / 行数 |
|---|---|---|
| `cli_run_serve.rs`（原 730） | 拆出 instance lock 一族 | **已完成（#1275）：730 → 391**；#1274 又接收唯一调用方同族的 `validate_bind_auth`，现 **410**（见 §12.2.2） |
| **新** `cli_run_serve_lock.rs` | 常量 ×3、`parse_instance_id` 166-191、`serve_instance_id` 194-205、`env_flag_enabled` 207-214、`serve_instance_lock_targets` 267-287、`serve_acquire_instance_lock_at` 301-352、`serve_acquire_instance_locks` 372-403，+ 7 个单测 | **已建：369 行**（业务 151 + 测试 135 + 头/常量） |
| **新** `cli_run_serve_branch.rs` | `build_serve_runtime_state` 432-520、`spawn_uploads_cleanup` 586-609、`build_serve_shutdown_signal` 714-734、`run_serve_branch` 两变体 624-712 / 737-745，**外加 `ServeBranchArgs` 的两个 cfg 变体（404 web / 748 非 web）**；审查跟进又把同族的 `parse_bind_ip` / `serve_log_bind_warnings` / `print_desktop_ready_json` / `spawn_conversation_prune` 一并归入 | **已完成（#1274）：382 物理行 / 341 非注释非空行**（`ServeRuntimeBuildInput` 随 `build_serve_runtime_state` 一并搬入） |
| **新** `cli_run_bench.rs` | `run_benchmark_batch_if_requested` 764-814（**无 `cfg` 门**） | **已完成（#1274）：60 物理行 / 50 非注释非空行** |
| `cli_run.rs` | 只剩 `EarlyCliDispatch`、`run_cli_from_parsed` 的一次分派、各 `try_early_*` | **已完成（#1274）：912 → 493**（余量 8 → **427**）——serve 相关 helper 全部归位后，主文件不再有任何 `web` 装配痕迹 |

**每一步都是纯移动**，不改任何函数体。第一步已落地（#1275）：`cli_run_serve.rs` 730 → **391**，比本表原估的「≈580」更彻底——原估漏算了随代码搬走的 135 行单测。第二、三步同 PR 落地（#1274）：`cli_run.rs` 912 → **493**（比原估的「≈650」还低：`ServeRuntimeBuildInput` 与四个 serve helper 一并搬走，`HashMap` / `AtomicU64` / `chat_job_queue` / `web` / `runtime` 五处 `use` 随之失去消费点）。**S5-a 到此完成**，`cli_run.rs` 只剩早退子命令分派。

#### 12.2.1 已实测的搬移约束（开工前核对，勿凭直觉改）

> **本节已执行完毕**（#1275 第一步、#1274 第二/三步）。保留原文是因为它记录「为什么会踩坑」，
> 不是待办清单——照着再做一遍只会重复搬移。

1. **`ServeBranchArgs` 有两个 `cfg` 变体**：`cli_run.rs:404`（`#[cfg(feature = "web")]`）与 `:748`（`#[cfg(not(feature = "web"))]`）。字段集合相同，但 web 版字段**私有**、非 web 版是 `pub`（非 web 版由 `cli_run_session.rs` 构造）。它们必须与 `run_serve_branch` 一起搬进**同一个**新模块——只搬函数不搬类型，会在两个 feature 分支各留一个孤儿类型；搬类型时还要按新位置重定可见性，否则 web 版构造不出来。
2. **`run_serve_branch` 的调用方在兄弟模块**：`cli_run_session.rs:92` 写的是 `super::run_serve_branch(super::ServeBranchArgs { … })`，搬后要改成指向新模块（或在新模块里 `pub(super) use` 后维持 `super::` 路径）。反过来，`build_serve_runtime_state` / `spawn_uploads_cleanup` **只在 `cli_run.rs` 内部调用**，搬走后给 `pub(super)` 即可。
3. **`run_benchmark_batch_if_requested` 没有 `cfg` 门**（早期版本误判为「只在非 web 构建存在」，见 §12.2.2）：`cli_run.rs:747` 那条 `#[cfg(not(feature = "web"))]` 只作用于紧随其后的 `ServeBranchArgs`，762 行之后的函数不受它管辖。它被 `cli_run_session.rs:118` 无条件调用，两套构建都编译。
4. **instance lock 一族只被 `cli_run_serve.rs` 内部调用**（`serve_precheck` / `serve_validate_bind_gates` 等），拆出去后本文件的调用点需加 `use super::cli_run_serve_lock::{…}`。不跨文件、不改外部路径，故风险最低，放第一步。
5. **两条 feature 路径都要编译**：默认（web）与 `--no-default-features`（非 web）。`run_serve_branch` 与 `ServeBranchArgs` 各有 web / 非 web 两个变体，只跑默认构建会漏掉另一半。

顺序（每步一次提交、每步跑一次 `cargo test`）：

1. 拆 `cli_run_serve_lock.rs` —— 只动 `cli_run_serve.rs`，风险最低，先做；
2. 拆 `cli_run_serve_branch.rs` —— 从 `cli_run.rs` 搬约 232 行（含 `build_serve_shutdown_signal`）进去，同时改 `use` 指向；
3. 拆 `cli_run_bench.rs`；
4. 收尾：检查 `cli_run.rs` 里剩下的 `try_early_*` 是否还值得继续拆（那时已不紧急）。

**不动**：`cm_config/cli/definitions.rs` 的 clap 定义、`ParsedCliArgs`、`EarlyCliDispatch` 的字段集合、任何业务逻辑——这样 `--help` 与 man 页必然无 diff。

#### 12.2.2 核对修订（2026-11-04）

记录被实测推翻的判断，避免旧结论继续被引用：

| 早期判断 | 实测 | 影响 |
|---|---|---|
| 「`run_benchmark_batch_if_requested` 只在非 web 构建存在」 | **错**：它没有 `cfg` 门，两套构建都在（`#[cfg]` 只作用于紧跟的下一项） | 搬它不需要搬 `cfg`；原先「漏搬 cfg 会在默认构建报找不到函数」的告警作废 |
| serve 装配「合计 211 行」 | **232 行**：漏了 `build_serve_shutdown_signal`（714-734，web-only，21 行） | 第二步的搬移清单与预计行数已相应调整 |
| 第一步后 `cli_run_serve.rs`「≈580 行」 | **391 行**：原估只算业务代码，漏算随代码搬走的 135 行单测 | 该文件已无行数压力；第二步的目标回到 `cli_run.rs` 本身 |
| — | **新增规律：搬移会改变可见性需求** | `ServeInstanceLocks` 必须 `pub(in crate::cli_run)`（出现在 `ServePrecheck` 公开字段类型里）；`ENV_DISABLE_INSTANCE_LOCK` 必须 `pub(super)`（自检日志要打印它）。照抄原可见性会在 `-D warnings` 下失败 |
| — | **新增（#1274）：`server` feature 无条件包含 `web`** | `#[cfg(not(feature = "web"))]` 的 `run_serve_branch` / `ServeBranchArgs` 变体，在现有 feature 图下**任何组合都不编译**；`cargo check --no-default-features` 也不覆盖它（那组 feature 下 `cli_run` 整个不进编译图） | 搬移保持原样、未新增缺口；下文验收里「`--no-default-features` 覆盖两条 cfg 路径」这句**不成立**，不要再当已验证据引用 |
| 「每个新文件 ≤ 200 行 nloc」（§12.3） | 与本表「`cli_run_serve_branch.rs` ≈232 + 类型定义」**自相矛盾**；最终 **382** 物理行 / **341** 非注释非空 | 按本表的三文件分配执行（审查跟进又并入 4 个同族 helper），未为凑 200 再切第四个文件：生效门禁是 fn nloc ≤ 200（最大 **89**）与 file ≤ 920（**382**，余量 538） |
| `cli_run_serve.rs`「**只减不增**」（§12.3） | **391 → 410**（+19）：审查发现 `validate_bind_auth` 的唯一调用方是 `cli_run_serve::serve_validate_bind_gates`，原先留在 `cli_run.rs` 是错位 | 接受这 +19：410 / 920 余量 510，不构成「把地雷转移到下一处」；为守字面「不上升」而把它塞进 `cli_run_serve_branch.rs`，反而会造出 `cli_run_serve → cli_run_serve_branch` 的反向依赖 |

### 12.3 验收

- `crabmate --help`、`crabmate <每个子命令> --help` 输出与改前**逐字相同**；`man/crabmate.1` 无 diff（`cargo run --features gen-man --bin crabmate-gen-man` 后 `git diff` 空闲）。
- `cargo test` 全绿（尤其是 CLI 解析与 `serve` 启动相关单测）。
- **行数必须双向达标**：`cli_run.rs` 明显下降、`cli_run_serve.rs` **不得上升**、每个新文件 ≤ 200 行 nloc——只减一个文件而把压力转移到另一个文件，等于把地雷埋到下一处。
- `pre-commit run --all-files` 全绿（crate-dep policy 与 fn-param/nloc 两个 ratchet 都会看新文件）。
- 额外自检：`cargo clippy --all-targets --all-features -- -D warnings`——纯搬移最容易引入 `unused import` / 可见性过宽。

### 12.4 已知风险

- **可见性**：`try_early_*` 现在被同一文件内的私有调用，搬走后会变成跨模块调用，需要 `pub(super)` 或 `pub(crate)`——按最小可见性给，别图省事全开 `pub`。
- **挂载方式必须与既有两个子模块一致**：现有写法是 `src/cli_run.rs` 顶部 `#[path = "cli_run_serve.rs"] mod cli_run_serve;`——新文件同样放 `src/` 根、同样用 `#[path]`，不要顺手改成 `src/cli_run/` 子目录（那会让 `#[path]` 与目录两份约定并存）。
- **`#[allow(...)]` 与测试**：若有 `#[cfg(test)] mod tests` 依赖同文件私有项，搬移时要一起搬或改可见性。
- **`run_serve_branch` 有两个 feature 变体**（`:624` 与 `:737`）：搬移时两个都要搬，并保留各自的 `#[cfg(...)]` 门——漏一个会在特定 feature 组合下编译失败，而那组 feature 恰恰可能不在本地默认构建里。
- **别顺手做 S5 的其余部分**：`cm_cli/` 禁边 + serve builder 是独立的一步，混进来会让本切片失去「纯搬移、可盲合」的性质。

### 12.5 做完之后

`cli_run.rs` 余量恢复后，S5 的其余部分（`cm_cli/` 分派表 + `EarlyCommand`/`ServerCommand` 二分 + serve builder）才值得做——那时它只是「变清晰」，不再是「解阻塞」。**顺序反过来会让一个紧急的机械搬移背上语义重构的风险。**
