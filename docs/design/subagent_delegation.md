# 隔离上下文的子 Agent 委托（`delegate_task`）设计稿

> **状态**：设计已定稿（§9 的 5 个问题已落结论），实现未开始；P0 前置重构见 §12
> **跟踪**：[#839](https://github.com/noisystreet/CrabMate/issues/839) · **路线图**：`docs/待办清单.md`「多 Agent 协作」

## 1. 问题与目标

当前 Server 主路径是单次 `run_agent_turn`：session_mode →（Act 句启发式）→ ReAct 外循环。探索、改文件、跑测试都挤在
同一消息窗，长任务容易胀上下文、失败难隔离。分层 Manager/Operator 已归档并从树中移除，运行时没有子 Agent。

**本设计要实现**（照 #839）：

1. 父 Agent 可派**有界子任务**，子任务用**独立消息窗与预算**；
2. 子任务结束只回传**摘要 + 关键路径/结论**，不把工具日志灌回父上下文；
3. 子任务遵守既有工作区路径安全、工具白名单、`tool_approval`、墙钟预算；
4. 可配置：禁止嵌套，或限制深度为 1。

## 2. 现状（已核实的代码锚点）

| 事实 | 证据 |
| --- | --- |
| 子回合所需输入**全在环内** | `RunLoopCore` 有 `client` / `api_key` / `cfg` / `tools_defs` / `llm_backend`（`host/params.rs:44-52`） |
| 工具执行层**刻意不持有**凭据 | `src/agent/agent_turn/host/execute/**` 中 `api_key` 零命中（14 个文件） |
| 穿透链三层 | `RunLoopCore` → `ExecuteToolsCommonCtx`（`tools/mod.rs:147`）→ `SerialToolLoopState`（`serial/exec_serial.rs:45`）→ `CrabmateToolExecutionHost`（`tool_execution_host.rs:54`）→ `DispatchToolParams` |
| **已有「不进 dispatch」的先例** | `workflow_execute`：`tool_registry/execute/dispatch.rs:526-532` 明写「须由 agent 层调度」；特判点在 `tool_execution_host.rs:70` |
| **已有「能力注入」的先例** | `cm_tools/memory_tool_host.rs`：trait 由 crate 侧实现并注入，避免 `cm_tools → cm_memory` 环 |
| **已有「只读性」单一真源** | `tool_registry::is_readonly_tool(cfg, name)`（`serial/exec_serial.rs:199` 已在用） |
| 回合执行面可注入 | `TurnEngine` + `default_turn_engine()`（`turn_runner.rs:72-91`），默认转发 `run_agent_turn` |
| 公开面 golden 只记类型名 | `fixtures/public_api_golden.txt:24,28` 只有 `promised struct RunAgentTurnAttach` / `TurnRequest`，**不列字段** |

## 3. 关键约束：不做成 cm_tools 里的普通 runner

子回合必须调 LLM，而 `api_key` / `client` 在工具执行层**不存在**。若把 `delegate_task` 做成 `cm_tools` 的同步 runner，
只有两条路：

- 把 `api_key` 塞进 `DispatchToolParams` —— 等于让**所有**工具实现都能读到 LLM 凭据，破坏当前分层；
- 在 runner 里自建 HTTP client 与凭据来源 —— 又一处凭据装配点，与 `RunAgentTurnSharedInputs` 分叉。

因此：**`delegate_task` 走 agent 层调度（照 `workflow_execute`），能力用 trait 注入（照 `memory_tool_host`）。**

## 4. 设计总览

### 4.1 两层结构

```
cm_tools/delegation_host.rs        只放契约（cm_tools 不依赖 agent / llm）
    SubTaskRequest / SubTaskReport / SubTaskStatus / SubTaskStats
    #[async_trait] trait SubAgentDelegator { async fn run(&self, req) -> Result<SubTaskReport, ToolError>; }

agent/delegation.rs                crate 侧实现（能拿 client / api_key / cfg）
    struct CrabmateDelegator { client, api_key, cfg, tools_defs, caps }
    impl SubAgentDelegator → 装配子 TurnRequest → engine.run(params)   // engine: Arc<dyn TurnEngine>
```

**递归调用一律经 `Arc<dyn TurnEngine>`**（见 P0-4）：`async_trait` 生成的就是 boxed future，所以
「父回合 → 工具 → 子回合」这条递归**不会**触发 `recursion in an async fn requires boxing`。只有在绕过
trait object、直接调 `run_agent_turn(...)` 时才需手写 `Box::pin` —— 不建议，那等于把装箱责任散到各调用点。

### 4.2 数据契约

```rust
pub enum SubTaskStatus { Ok, BudgetExhausted, Cancelled, Denied, Failed }

pub struct SubTaskRequest {
    pub prompt: String,              // 唯一入口：任务描述；父上下文一概不传
    pub depth: u8,                   // 调用方透传的第几层（0 = 父回合直接派）
    pub allowed_tools: Option<Vec<String>>,  // None = 按只读性派生（见 §9 Q4）
    pub max_llm_calls: u32,
    pub max_tool_calls: u32,
    pub max_wall_secs: u64,
}

pub struct SubTaskReport {
    pub status: SubTaskStatus,
    pub summary: String,             // 截断到 max_report_chars
    pub key_paths: Vec<String>,      // 上限 20 条
    pub stats: SubTaskStats,         // llm_calls / tool_calls / elapsed_secs / estimated_tokens
    pub truncated: bool,
    pub error_code: Option<&'static str>,
}
```

**`depth` 进契约，且只是双保险的一半**（§9 Q2）：它是任务链上可观测的属性（日志、追踪、报表都要能说清
「这是第几层」）；另一半是「超限不注入 delegator」，让越权路径连入口都没有。两者分别服务「可观测」与
「不可达」，缺哪一半都不够。

## 5. 子回合装配（隔离语义的真正载体）

| 字段 | 取值 | 理由 |
| --- | --- | --- |
| `session.messages` | **全新 `Vec`**，只含收窄 system + 任务描述 | 隔离本体；绝不复制父 messages |
| `transport.out` / `no_stream` | `None` / `true` | 子回合**不得**写父 SSE 流 |
| `transport.cancel` | **共享父的 `Arc<AtomicBool>`** | 父被取消 → 子立即收手 |
| `transport.per_flight` | 新建 | 子有自己的截断信号，不污染父 |
| `session.effective_working_dir` / `workspace_is_set` | 继承父 | 否则路径安全（`PathAccess` / `openat2`）直接失效 |
| `attach.permission` | `min(父等级, cfg 上限)` | 子任务**不可提权**（AGENTS 4.6） |
| `attach.turn_allowed_tool_names` | 按只读性派生后**再收窄**（§9 Q4） | 第一刀只读；配置只能减不能加 |
| `attach.long_term_memory` / `scope_id` | 继承父 scope，但**不给写工具** | 记忆可读，写入仍归父回合 |
| `obs.process_handles` | 复用父的 `Arc` | `read_file_turn_cache` / 工具统计口径一致 |
| `obs.tool_job_registry` | **`None`** | 子任务不发起后台长任务 |
| `llm.llm_backend` | 继承（`shared.llm_backend`） | 也正因如此，测试可用 mock backend |

`llm` 其余覆盖项（temperature / model / executor_*）一律继承父回合，不新开口子。

## 6. 接线点（4 处）

1. `RunLoopCore` / `RunAgentTurnAttach` 增加 `delegation: Option<Arc<dyn SubAgentDelegator>>`
   —— **加的是能力对象，不是裸 `api_key`**，凭据不外扩。golden 不记字段，故不破 S6；但需同步所有
   `RunAgentTurnAttach` 构造点（web / CLI / bench / tests）。
2. `DispatchToolParams` 增加分组 `DispatchToolDelegation<'a> { delegator: Option<&'a dyn SubAgentDelegator> }`
   （`policy` / `obs` / `memory` 已有同类分组，照抄形状）。
3. `tool_dispatch.rs`：`HandlerId::Delegate` + 注册行 `(tool_names::DELEGATE_TASK, false, BlockingSync, Delegate)`；
   `dispatch.rs` 对该 handler 返回「须由 agent 层调度，不应进入 dispatch_tool」。
4. `tool_execution_host.rs`：`dispatch_tool_call` 里与 `HandlerId::Workflow` 并列特判 →
   取 `p.delegation.delegator`，`None` 时返回「本回合未启用子任务委托」。`CrabmateParallelToolDispatch`
   直接拒绝（并行只读批里跑子回合会打破「只读批无副作用」的前提）。

## 7. 预算与「掐断」

子回合自带 `TurnBudgetCounter`（从 `cfg` 取），**额外**叠三个硬上限：

| 上限 | 检查点 | 触发后 |
| --- | --- | --- |
| `max_llm_calls` | 每次 LLM 调用返回后 | 置 `cancel=true`，`status = BudgetExhausted` |
| `max_tool_calls` | 每次工具调用前 | 同上 |
| `max_wall_secs` | 每轮外循环入口 + 每次工具前后 | 同上 |

**父回合必须照常结束**（#839 的金样）：子任务被掐断只体现为一条 `status=budget_exhausted` 的 tool 结果，
`TurnOutcome` 仍正常返回。

## 8. 安全边界

- **双保险，而不是二选一**：`cfg.subagent.enabled = false`（默认）或 `depth >= max_depth` 时**不注入** delegator
  （越权无入口）；同时实现侧按 `req.depth` 再拒一次（越权有记录）。两条都要，理由见 §9 Q2。
- **不继承父上下文**：`prompt` 是唯一入口。父的历史、工具日志、system 全文都不进子回合。
- **能力按分类派生，不按名单枚举**：子任务的工具集 = `is_readonly_tool` 为真的那些（§9 Q4），并允许配置**收窄**；
  配置无法把写工具放进来 —— 否则等于绕过读/写分类这条硬边界。
- **路径与审批**：子回合走的是同一条 `dispatch_tool`，因此 `PathAccess` 与 `tool_approval` 自动沿用；
  但 `--yes` / 高权限等级**不随子任务放大**（`min` 链）。
- **回传有界**：`summary` 截断、`key_paths` ≤ 20、产物只回**引用**（§9 Q5），**绝不**回传子 messages 或工具原文。

## 9. 结论（5 个问题，按通用原则落定）

| # | 问题 | **结论** | 通用理由 |
| --- | --- | --- | --- |
| Q1 | 落点 | **内置工具 `delegate_task`** | 委托是「一次有界调用」：有输入、有界输出、何时调用由模型在运行中决定 —— 这就是工具的语义。workflow 节点要求调用方**事前**知道要探索什么，与「探索」这个用例直接冲突；而工具面是 Web / CLI / TUI 共用的最小公分母，不必新增对外契约形状。 |
| Q2 | `depth` 存哪儿 | **进 `SubTaskRequest`，再叠加「超限不注入」** | 递归上限是任务链的属性，必须能在日志/追踪里说清「第几层」——只在注入层隐式表达就不可观测；反之只靠运行时检查，越权路径始终可达。「可观测」与「不可达」是两个目标，各需要一半。 |
| Q3 | 子任务能否选 profile / 角色 | **禁止** | profile 描述的是**部署形状**（工作区白名单、命令白名单、权限等级、模型），属于「谁在什么环境里跑」；子任务是父回合内的一次派生工作，沿用父的部署形状即可。让它另选 profile 等于给被委托方一个独立部署身份，与「不可提权」直接冲突。将来要做角色化子任务，正确形态是「继承父 profile 的只读投影」，而不是新选一份。 |
| Q4 | 子任务工具集 | **按 `is_readonly_tool` 派生，配置只能收窄** | 手写白名单会与工具注册表漂移：新增只读工具默认进不来（能力白白丢失），新增写工具却没人拦（因为名单没变）。本仓已有「只读性」单一真源，直接派生即可；派生 + 只能收窄，同时满足「新工具自动可用」与「写工具无法被放进来」。 |
| Q5 | 产物是否落盘 | **不落盘，只回引用；落盘由父回合经既有写工具完成** | 子任务的全部价值建立在「无副作用地探索、失败可整体丢弃」上，自己写盘就破坏了 P1 的只读不变量。而返回**引用**（路径 / 将来的 artifact id）比返回文件内容更可演进：一旦引入 artifact store，引用可原地升级，路径字符串不行。 |

**这 5 条互相自洽**：工具落点（Q1）使「一次有界调用」成立 → 只读派生（Q4）+ 不落盘（Q5）让失败可丢弃 →
`depth` 双保险（Q2）+ 禁 profile（Q3）保证递归与提权两个方向都被封住。

## 10. 配置面（建议）

```toml
[subagent]
enabled = false              # 默认关；关 = 工具不可见
max_depth = 1                # 1 = 子任务不能再派
max_llm_calls = 8
max_tool_calls = 16
max_wall_secs = 300
max_report_chars = 4000
allowed_tools = []           # 空 = 按只读性派生全集；非空只能**收窄**（写工具写进来也无效）
```

需要同步：`config/default_config.toml`、`docs/配置说明.md`、`docs/工具说明.md`、`README.md`（用户可见面）。

## 11. 测试计划

| 层 | 用例 |
| --- | --- |
| 单测 | `SubTaskReport` 序列化有界（summary 截断 / key_paths ≤ 20）；三个上限各自触发 `BudgetExhausted`；`allowed_tools` 里的写工具被忽略 |
| golden | 子回合 prompt 装配结果：**只有** 收窄 system + 任务描述，不含父历史 |
| e2e | 用注入的 mock `llm_backend` 造「子任务第 2 次 LLM 调用即超预算」→ 断言子任务被掐断、**父回合 `TurnOutcome` 正常**、父 `messages` 里只有一条受限 report |
| 反向 | `enabled=false` 时 `delegate_task` 不可见 / 调用即拒；`depth >= max_depth` 时既无入口也被运行时拒；`allowed_tools` 无法放大到写工具 |
| 安全 | 子任务尝试写文件被拒；子任务 `permission` 不高于父；父取消 → 子 `Cancelled` |

## 12. 分期与非目标

### P0：先做 4 项前置重构（不实现本功能，但让 P1 变简单）

四项都满足同一条门槛：**不改对外行为、可独立合并、可独立验证**。每项都直接削掉 P1 的一处成本。

| # | 重构 | 现状问题（已核实） | 做法 | 对 P1 的收益 |
| --- | --- | --- | --- | --- |
| P0-1 | **只读性判定收成单一真源** | 有两份实现：`cm_tools/registry_policy.rs:132`（按 `tool_origin`）与 `cm_memory/memory/codebase_semantic_invalidation.rs:28`（按 `mcp__` / `tool_` 前缀）。后者第 21-24 行注释自己记录过上次双源漂移：名单停在 `apply_diff` / `write_file` / `mkdir`，还漏了 `structured` / `reminder` / `event` / `archive_*`，导致语义索引对真正写盘的工具不作废 | 只留一处实现，另一处改为调用；补 golden「全部工具名 → 只读 / 写」 | **Q4 的派生根源真的单一**；不先把这条修掉，子任务的只读集与失效决策会各判一套。新增工具只归类一次 |
| P0-2 | **环级不可变输入收口** | `client` / `api_key` / `cfg` / `tools_defs` 在 `RunLoopCore`，要到工具执行层需穿 `ExecuteToolsCommonCtx` → `SerialToolLoopState` → `CrabmateToolExecutionHost` → `DispatchToolParams` 四层，逐字段复制 | 把这些「整场不变」的输入收成一个窄结构（如 `&'a LoopEnv`），下游各层持**一个引用** | P1 只需在 `LoopEnv` 加 delegator **一处**；将来任何环级能力同理。否则每次加能力都要改 4 个结构体的字段 |
| P0-3 | **agent 层工具调度改为表驱动** | `tool_execution_host.rs:70` 硬编码 `if id_for(name) == HandlerId::Workflow`，`CrabmateParallelToolDispatch` 里再来一次；加 delegate 会变成第三、第四处 if | agent 层一个 `HandlerId → handler` 的小表，未登记回落 `InternalToolDispatch` | delegate **只注册一行**；「哪些工具必须 agent 层调度」变成可枚举清单（也便于写测试和文档） |
| P0-4 | **子回合构造器 + 统一经 `TurnEngine` 调用** | `TurnRequest` 是 6 组 20+ 字段的参数袋；直接 `run_agent_turn` 递归会让编译器要求装箱 | 提供 `SubTurnSpec → TurnRequest` 构造点；子回合统一经 `Arc<dyn TurnEngine>` 调用（`async_trait` 本已装箱） | P1 的 `delegation.rs` 只需填 spec；且**不必手写 `Box::pin` 绕递归类型**（比 §4.1 的原写法更干净） |

**P0 的边界（不做什么）**：不含拆 crate、不含砍 `TurnRequest` 字段的 SDK 化（那是 `docs/design/sdk_facade.md` 的独立项目）、
不改公开 API 形状、不动 `fixtures/public_api_golden.txt` 的承诺集。P0 膨胀成「大重构工程」就等于让 P1 无限延期。

### P1 / P2 / P3

- **P1**：只读派生子集（依赖 P0-1）+ `max_depth=1` + 三个硬上限 + §11 的金样与 e2e（#839 的「第一刀」）。
- **P2**：summary 质量（关键路径提取）、进度回传、`SubTaskReport` 进 SSE 事件。
- **P3**：嵌套、并行多子任务、写工具子任务（要重新过安全评审）、artifact store。

**非目标**（照 #839）：不复活 `hierarchy/` / `staged/` 入口或全局单表 FSM；不做多用户账号；不做 CrewAI 式消息总线。

## 13. 风险

| 风险 | 缓解 |
| --- | --- |
| async 递归导致栈/类型膨胀 | 一律 `Box::pin`；`delegate_task` 不进并行只读批 |
| 子任务与父竞争同一 workspace 写入 | P1 只读 + §8；写工具在 P3 单独评审 |
| 预算口径分裂（父 vs 子） | 子回合用**自己的** `TurnBudgetCounter`，父账本不并入子回合数字；report 里另列 |
| `RunAgentTurnAttach` 加字段波及构造点 | 一次性同步 web / CLI / bench / tests |
| 上下文节省被「summary 太长」抵消 | `max_report_chars` 默认 4000，截断时 `truncated=true` 让父回合知道 |
| 只读派生集过大（子任务仍可跑很多工具） | `allowed_tools` 可收窄；P2 可按任务类型给预设子集 |

## 14. 落地顺序

0. **P0 四项前置重构**（§12），建议顺序 **P0-1 → P0-3 → P0-2 → P0-4**：
   P0-1 是纯等价重构、风险最低，且 Q4 直接依赖它；P0-3 一行注册即可验证，改动面小；
   P0-2 触及环内四个结构体，改动面最大，放后面；P0-4 依赖 P0-2 的收口结果，最后做。
   每项独立 PR、独立验证，全部不改对外行为。
1. P0 四项各自独立 PR（独立合并、独立验证），全部落地后进入 P1；
2. `delegation_host.rs` 契约 + 单测；
3. `agent/delegation.rs` 装配 + mock backend e2e（**先把「超预算被掐断、父仍正常」跑通**——全流程最易错也最有价值的一步）；
4. 接线（P0-2 / P0-3 之后各只需改一处）+ 工具注册三步（`cm_names::DELEGATE_TASK` + `ALL` + `specs/*.inc.rs` + `param_types`）；
5. 配置面 + 文档 +（如变化）`fixtures/public_api_golden.txt`。

## 15. 相关文档

- `docs/design/turn_host_decouple.md` / `docs/design/turn_runtime_placement.md`：回合执行面为何可注入（P0-4 的前提）
- `docs/design/sdk_facade.md`：`TurnRequest` 参数袋的 SDK 化 —— P0 **刻意不含**，避免与本设计耦合
- `docs/design/background_tool_jobs_contract.md`：为什么子任务不给 `tool_job_registry`
- `docs/开发文档.md`：`agent_turn` / `cm_tools` 的模块边界与扩展点
- `AGENTS.md` §4.6：安全敏感面清单（本设计 §8 与之对齐）
