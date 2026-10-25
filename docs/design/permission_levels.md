# 工具权限等级（Permission Levels）：设计草案

**状态**：设计草案（**未**实现）。默认等级取**现状**（`approved`），默认配置下**零行为变化**（等价性核对见 §5.2）。
**受众**：维护者、安全评审、Client（Web / `crabmate-tui`）维护者。
**语言**：中文。
**维护**：实现或废弃某级能力时同步修订本文、[`docs/design/README.md`](README.md) 索引与 [`docs/待办清单.md`](../待办清单.md)。

**关联文档**

- [`docs/配置说明.md`](../配置说明.md)：`allowed_commands` / `http_fetch_allowed_prefixes` / `workspace_allowed_roots` / `sync_default_tool_sandbox_mode` / `session_mode` 的现行逐项语义（本文是它们的**上位统一视图**，不替代各键说明）。
- [`docs/工具说明.md`](../工具说明.md)：受影响工具（`run_command` / `terminal_session` / `read_file` / `write` / `read_dir` / `http_fetch` / `http_request`）。
- [`docs/SSE协议.md`](../SSE协议.md) 与 Client `frontend/src/api/chat_stream/parser_v2.rs`：若新增控制面事件（§11）须两端同步。
- [`AGENTS.md`](../../AGENTS.md) → **Security-sensitive surfaces** / **Secrets & logging**：本文 §9 是其延伸，不得弱化。
- [`docs/design/tool_management_api.md`](tool_management_api.md)：`tool_overrides.json` 的单工具启停（与等级取交，见 §4.3）。

---

## 0. TL;DR

| 级 | 名称 | 一句话语义 |
|---|---|---|
| **L0** | `chat` | **只能对话**：不下发工具定义，纯文本应答 |
| **L1** | `read` | 只读探查：工作区内读 + 出站只读，写盘/命令全禁 |
| **L2** | `workspace` | 工作区**硬边界**：写与命令限工作区 + 白名单，越界**直接拒**，不打扰人 |
| **L3** | `approved`（**默认**） | **现状**：基线同 L2，但越界走人工审批（AllowOnce / AllowAlways） |
| **L4** | `unrestricted` | **不受工作区路径与命令白名单限制**：不问人，只审计 |

三条硬约束（详见 §9）：

1. **请求只能降级，不能提权**：`permission_level` 由客户端传入时须 ≤ 服务端上限，越界即钳制并记审计。
2. **上限不由客户端控制**：`permission_level_max` 只来自服务端（显式 TOML / `CM_*`，或**按部署形态推导**，见 §5.1），不经 `client_llm` 等客户端可控字段。
3. **L4 不等于免除不可逆操作确认**：删除/强推类确认仍在（§9 MB-4）。

---

## 1. 现状：权限已经存在，但散在 6 条轴

### 1.1 六条轴

当前分支（`refactor/agent-turn-merge`）**没有**任何 `permission_level` 概念；"权限"实际由六条互相独立的轴拼出，每条各自决定一件事：

| 轴 | 机制 | 锚点 |
|---|---|---|
| 工具可见性 | `SessionMode` Ask/Plan → `PlanStepExecutorKind::ReviewReadonly` | `src/agent/agent_turn/turn_loop/run_dispatch.rs:34`、`turn_loop/outer_loop.rs:101`（`build_planner_round_tools`） |
| 工具准入 | 子代理角色 / 角色 `allowed_tools` 的 early-deny | `src/agent/agent_turn/policy/tool_execution.rs:93`（`tool_policy_early_deny_message`）、`src/cm_agent/turn_tool_policy.rs:66` |
| 命令 | `allowed_commands` 白名单（`*` 哨兵已支持） | `src/cm_tools/tools/command_shell_script.rs:29`（`allowlist_allows_all`）、`src/cm_internal/tool_registry/execute/execute_run_command_approval.inc.rs:147`（`evaluate_policy`） |
| 路径 | 根内相对路径硬校验；绝对路径 / `..` 在文件工具直接拒 | `src/cm_tools/tools/file/path.rs:127`（`resolve_for_read`）、`:159`（`resolve_for_write`）、`src/cm_tools/workspace/path.rs` |
| 出站网络 | `http_fetch_allowed_prefixes`（`config/tools.toml` 默认 `["*"]`） | `src/cm_internal/tool_registry/execute/http_tool_exec.inc.rs:4`（`http_tool_url_allowed`） |
| 人工审批 / 隔离 | `SensitiveCapability`（5 类）+ Docker 沙盒 | `src/cm_approval/mod.rs:26`、`src/cm_internal/tool_approval/approval_specs.rs`、`src/cm_internal/tool_sandbox/mod.rs:82` |

### 1.2 三个真实缺口

这套分级不是"新加一个开关"，而是顺手修掉现状里的三处不一致：

1. **同一概念三种待遇**：「工作区外路径」在 `run_command` / `terminal_session` 可走审批放行（`execute/mod.rs:134`、`approve_external_run_command_paths_if_needed`），在 `read_file` / `write` 却是**永久硬拒**。用户无法表达"我就是要让 Agent 读隔壁仓"。
2. **无人值守场景没有"硬边界"档**：`bench`、`[[scheduled_agent_task]]`、运维 CLI 没有 SSE 审批通道，白名单未命中只能失败（`request_unknown_cmd_approval` 的 `ApprovalUnavailable` 分支）。这是**偶然行为**，不是**明确语义**。
3. **两条轴默认松紧相反**：命令走"长白名单 + 未命中审批"，出站网络却是 `http_fetch_allowed_prefixes = ["*"]`。运维无法用一句话回答"这台机器上 Agent 到底能干什么"。

### 1.3 关键事实：审批等于"人授予 host 能力"

这一点决定了 L3 与 L4 的边界怎么画，必须先说清：

- 未知命令 → `AllowAlways` 会把该命令名写入会话级 `persistent_allowlist_shared`（`execute_run_command_approval.inc.rs:277`），或直接扩展本轮 `allowed_commands`（`:316`）；
- 工作区外路径 → 审批通过后 `skip_arg_safety = true`（`:170`），本轮直接跳过 `..` / 绝对路径拒绝。

也就是说：**现状下"审批通过"= 该次调用拿到 host 级能力**。所以"最高等级"不是"比现状更强的能力"，而是"**同样的能力，但不问人**"。这直接决定了：

- L3 `approved` = **能力上限同 L4，但每次越界须人工确认**（这正是今天的行为）；
- L4 `unrestricted` = **同上的能力，不问人，只留审计**。

反过来，"更低等级"才需要新增——今天没有 L2 这种"越界即拒、不打扰人"的档位。

---

## 2. 设计目标与非目标

### 2.1 目标

1. **单一旋钮**：用一个 `PermissionLevel` 同时投影到工具可见性、外部工具、路径、命令、网络、审批**六轴**，取代"散读 6 个配置键"的心智负担。
2. **默认零变化**：默认配置 = 现状（§5.2 逐轴核对）。
3. **单调**：等级越高，允许面越大；任何组合规则只收紧不放宽（§6）。
4. **可降不可升**：请求侧永远无法获得高于服务端上限的能力（§9 MB-1）。
5. **可解释**：任一时刻能回答"本级为什么拒了这条调用"，且该回答能进入 SSE 时间线与 `tracing`。
6. **不破坏现有结构**：不引入新 crate、不改 SSE 线格式（除非 §11 的审计事件被采纳）、不让函数参数数量与 CCN 失控（§7.3）。

### 2.2 非目标

- **不做多租户身份体系**。本设计只解决"这个进程允许什么"，不解决"哪个用户允许什么"（见 [`docs/未来规划功能.md`](../未来规划功能.md)）。
- **不替代沙盒**。Docker 沙盒（`sync_default_tool_sandbox_mode`）是**执行隔离**，与等级正交；等级只负责"是否允许多做"（§6.4 有一条钳制例外）。
- **不替代 `session_mode`**。二者正交，见 §4.3。
- **不改变 `run_command` 参数安全规则本身的实现**（`is_arg_safe`、`allowlist_allows_all` 等保持原样），只改变**谁来驱动它们的入参**。
- **不在本设计里解决"审批通道缺失"**：无通道入口靠 §5.4 的**等级定档**绕开（L2 越界即拒而非弹窗），Web 审批链路本身不动。

---

## 3. 术语

| 术语 | 含义 |
|---|---|
| **等级（level）** | 本设计引入的 `PermissionLevel`，L0–L4。定义"默认允许面 + 是否保留人工例外通道"。 |
| **上限（ceiling）** | `permission_level_max`。服务端硬上限：显式配置 / 环境变量，或**未配置时按部署形态推导**（loopback → `unrestricted`，非 loopback → `approved`，见 §5.1）；请求侧与 HTTP 均不可写。 |
| **越界** | 调用超出当前等级的默认允许面。L2 下越界 = 拒绝；L3 下越界 = 审批；L4 下"越界"概念不存在。 |
| **投影（caps）** | `PermissionCaps`，由等级 + 配置算出的**纯值结构**，各执行点只消费它，不读等级本身。 |
| **`session_mode`** | 既有的 Ask / Plan / Act 会话工作模式，驱动 system 附录与编排姿态，**不是**安全边界。 |
| **审批通道** | Web SSE 的 `command_approval_request` 往返（`src/cm_approval/mod.rs`）；无通道时越界只能失败。 |

---

## 4. 等级定义

### 4.1 语义表

| 级 | 名称 | 工具可见性 | 路径范围 | 命令 | 出站网络 | 越界处理 |
|---|---|---|---|---|---|---|
| L0 | `chat` | **无**（不下发 `tools`） | 不适用 | 全禁 | 全禁 | 不适用 |
| L1 | `read` | 只读**正列表**（`permission_read_tools`，**非** `is_readonly_tool`） | 工作区内 | 全禁（硬拒；S2 即生效） | 配置前缀内 | **拒绝** |
| L2 | `workspace` | 全量**内置**工具（**不含** `mcp__*` / 动态工具，§7.1 `external_tools`） | 工作区内 | `allowed_commands` | 配置前缀内 | **拒绝** |
| L3 | `approved` | 全量 | 工作区内 | `allowed_commands` | 配置前缀内 | **人工审批** |
| L4 | `unrestricted` | 全量 | **任意** | **任意** | **任意** | 放行 + 审计 |

### 4.2 每级的用户可见承诺

- **L0 `chat`**：模型只能说话。`tools` 数组不下发或为空；**默认**连“澄清问卷”工具也不下发，符合“最低等级只允许对话”。若确实需要多轮澄清，可显式配置 `permission_chat_control_tools = ["present_clarification_questionnaire"]`（§10）。
  - 长期记忆**召回**（`ltm.prepare_messages` 的只读注入）保留；写入类工具（`long_term_remember` / `long_term_forget`）随“无工具”一并消失。
- **L1 `read`**：能看不能动。无写盘、无子进程、无 PTY；`http_fetch` 仍可用（属只读），`http_request` 被排除。
  - 判据是**独立正列表** `permission_read_tools`，**不用** `is_readonly_tool`：后者是并行只读批的**性能判据**，且其写集合可被 `tool_registry_write_effect_tools` **整体替换**（`registry_policy.rs:156`）——拿它当安全边界，运维为调并行漏配 `run_command` 就会让 L1 直接放行子进程。
- **L2 `workspace`**：能干活的**硬边界**档。越界不是“弹个窗”，而是明确报错并说明原因（文案须点出 `permission_level = workspace`），供无人值守场景安全使用。
  - **同时禁用 MCP 与动态工具**（`external_tools = Denied`）：`mcp__*` 与 `plugins/*.json` 的实际能力由**外部进程**决定，宿主侧的路径 / 命令 / 网络三轴**管不到**它们——放进来就等于没有硬边界。要 MCP 就用 L3（审批）或 L4（放行 + 审计），并接受它们不受 caps 约束。
- **L3 `approved`**：**默认档**，与今天完全一致。越界弹审批，`AllowAlways` 在会话内持续有效。
- **L4 `unrestricted`**：不受工作区路径与命令白名单限制。**仍需**不可逆操作确认（§9 MB-4），并写审计。

### 4.3 与既有四套机制的组合（只收紧）

| 机制 | 关系 | 组合规则 |
|---|---|---|
| `session_mode`（Ask/Plan/Act） | **正交**：它驱动 system 附录（`config/prompts/mode_*.md`）与 plan/act 编排姿态 | `ask` / `plan` → **钳到 ≤ L1**；`act` → 不钳 |
| `agent_roles[].allowed_tools` | 同一维度的**更细**约束 | 与 `tool_visibility` **取交**（等级先粗筛，角色再细筛） |
| `agent_roles[].default_session_mode` | 角色默认姿态 | 经 `session_mode` 间接生效，即 `ask`/`plan` 角色默认落在 ≤ L1 |
| `tool_overrides.json`（`PUT /tools/{name}/enabled`） | 单工具硬开关 | 与等级**取交**；L4 也不越过 `enabled = false` |
| `sync_default_tool_sandbox_mode = docker` | 执行隔离 | **只钳路径与网络两轴**（`path_scope` → `SessionRoot`，`net_scope` 按容器网络），**不动** `level` 与 `approval`（§6.4） |
| `workspace_allowed_roots` / `web_workspace_pool` | 工作区**选址**白名单 | 与等级链**独立**：等级管“能读工作区外吗”，它们管“工作区根能选在哪”。L4 不放宽选址约束 |

> 为什么不把 `session_mode` 折进等级：它两个职责都还在用（提示词附录 + 编排），合并会让“功能姿态”与“安全天花板”两个语义都不清楚；用 `min()` 组合是一行规则，比合并 enum 更便宜。

---

## 5. 默认值与兼容：默认取现状

### 5.1 默认值：上限按部署形态推导

```toml
# 会话默认等级（可被 /chat 请求与会话持久化覆盖，但只能降）
permission_level = "approved"        # CM_PERMISSION_LEVEL
# 服务端硬上限：**不写**则按部署形态推导（见下表）；写则覆盖推导
# permission_level_max = "approved"  # CM_PERMISSION_LEVEL_MAX
```

`permission_level_max` **默认不配置**，启动时按绑定地址推导——`serve` 已在同一处做过 loopback 判断（`cli_run.rs:534 validate_bind_auth`，非 loopback 无 token 直接拒启）：

| 部署形态 | 推导出的 ceiling | `ceiling_source` | 理由 |
|---|---|---|---|
| 绑 `127.0.0.1`（`serve` 默认） | **`unrestricted`** | `derived_loopback` | 本机单用户 = 主流本地 agent 模型：天花板是**操作系统**；本地任何进程本就拥有该用户的文件权限，给它 L4 不多给任何东西 |
| 绑非 loopback（`--host 0.0.0.0`） | `approved` | `derived_non_loopback` | 客户端可能来自远端，信任域不同；要 L4 须显式配置 + 开关（§5.3） |
| 显式写了 `permission_level_max` | 按配置 | `config` | 运维要更严（如 `workspace`）或明确放行，都以显式值为准 |

即：**默认等级 = `approved`（L3）= 现状**；**默认上限**在本机为 `unrestricted`、在远端为 `approved`。

> 为什么本机给 `unrestricted`：主流 agent 都允许客户端随时切到「不再询问」的模式，因为它们默认就是**本机单用户**运行、以当前 uid 操作当前用户的文件。CrabMate 在本机部署下是同一模型，不该因“也可能有远端客户端”而把本机一起锁死。**推导而非写死**，正是为了把这两种形态分开对待。

### 5.2 逐轴等价性核对（默认 `permission_level = approved`）

| 轴 | 现状行为 | L3 投影 | 等价 |
|---|---|---|---|
| 工具可见性 | 全量 `tools_defs`，受 `session_mode` / 角色 `allowed_tools` / `tool_overrides.json` 收窄 | `Full`（再由同一批机制收窄） | ✅ |
| 路径 | 工作区内；文件工具遇绝对路径 / `..` 硬拒；`run_command` / `read_dir` 走审批 | `SessionRoot` + `Gate` | ✅ |
| 命令 | 白名单命中→执行；未命中→工作区内可执行文件自动放行 / 会话 persistent 命中 / 否则审批 | `Allowlist` + `Gate` | ✅ |
| 出站网络 | `http_fetch_allowed_prefixes` 命中→执行；未命中→审批 | `Prefixes` + `Gate` | ✅ |
| 审批 | Web SSE，`AllowOnce` / `AllowAlways`，无通道则拒 | `ApprovalPolicy::Gate`（行为同现） | ✅ |
| 沙盒 | 独立开关，Docker 下挂 `/workspace:rw` | 不参与（`none` 时无钳制） | ✅ |
| MCP / 动态工具 | 一律放行（`step_executor_policy.rs:98` 对 MCP 直接 `return true`） | `external_tools = Allowed`（L3 与现状一致） | ✅ |

**结论**：不设任何新键时，**回合内的行为**与 `main`/当前分支一致（新追加的只有“可在请求中显式申请更高等级”这一项，不请求就不会发生）；S1–S3 切片可全程保持默认配置跑通全部现有测试。ceiling 的推导值**不改变任何单回合行为**，只决定“客户端能否申请到 L4”。

### 5.3 什么时候必须显式配置上限

| 场景 | 做法 |
|---|---|
| 本机使用（默认） | **不用配**。推导为 `unrestricted`，客户端可自由切到 L4（含会话首轮） |
| 远端部署、**不需要** L4 | **不用配**。推导为 `approved` |
| 远端部署、**需要** L4 | `permission_level_max = "unrestricted"` **且** `CM_ALLOW_UNRESTRICTED_PERMISSION_FOR_NON_LOOPBACK=1`，否则**启动失败** |
| 任何形态下想要**更严** | 显式写 `permission_level_max = "workspace"`（覆盖推导值） |

最后一条是这套设计的“硬化按钮”：不写配置 → 跟着部署形态走；写了 → 以运维为准（只会更严，或明确放行）。

### 5.4 各入口的默认等级（无人值守必须显式）

审批通道不是处处都有。没有 SSE 通道的入口若沿用 L3，越界只会变成“审批通道不可用”的失败——所以这些入口的默认等级必须**显式定档**，而不是撞到 `ApprovalUnavailable` 才发现：

| 入口 | 审批通道 | 默认等级 | 说明 |
|---|---|---|---|
| `serve` + Web / Desktop | SSE `command_approval_request` | 配置值（默认 `approved`） | 有通道 → 保留人工例外 |
| Client `crabmate-tui`（HTTP + SSE） | SSE | 同上 | 与 Web 同源，只是渲染在终端 |
| `bench`（批量测评） | **无** | **`workspace`** | 无人值守：越界即拒，比“卡在审批失败”可解释、可复现 |
| `[[scheduled_agent_task]]`（定时任务） | **无** | **`workspace`** | 同上；要放开须显式写 `permission_level` |
| 运维 CLI 中会实际执行工具的子命令 | **无** | **`workspace`** | 同规则；纯导出类（如 `save-session`）不跑回合，不挂 caps |
| `workflow_execute` 节点 | `WorkflowGate`（仅 Web 上下文） | **继承回合 `caps`** | MB-8：节点不得回读 `cfg` |

实现上：无通道入口由调用方在构造 `RunLoopAttach` 时直接传 `permission_level = workspace`，而不是依赖越界时才失败。


---

## 6. 解析与组合规则

### 6.1 min 链

```text
fallback  = min(cfg.permission_level, ceiling, role.max_permission_level)
requested = request.permission_level             // 可空
persisted = conversation.active_permission_level // 可空
chosen    = requested ?? persisted ?? fallback
effective = min(chosen, ceiling, role.max_permission_level)
```

- `min` 按枚举声明顺序（`Chat < Read < Workspace < Approved < Unrestricted`）。
- 非法字符串 / 大小写：与 `parse_session_mode` 同规（`trim` + 不区分大小写；非法 → 配置错误，启动期报错；请求期 → 400 而非静默回退，避免“以为降级了其实没有”）。
- 与 `session_mode` 的组合在**下一步**做：`session_mode ∈ {ask, plan}` → `effective = min(effective, Read)`。

### 6.2 持久化必须重算（fail-safe）

`active_permission_level` 像 `active_session_mode` 一样落库（`src/cm_internal/session_mode_turn.rs:86` 的同一模式），但**恢复时必须重新过一遍 §6.1**。理由：运维把 `permission_level_max` 调低后，旧会话若按库里的 `unrestricted` 续跑，等于降级失效。规则写死：

> **持久化只存“请求意图”，不存“生效结果”；生效结果每回合重算。**

热重载的**方向性**（降即时生效、升需重启）及其实现形态见 §9.1：`POST /config/reload` 调低 `permission_level_max` 须**立即**收紧进行中的会话（下一回合即生效）；调高则不追溯、且**不认**（ceiling 在启动时冻结）。

**落库形态**：会话库需加一列 `active_permission_level`。复用既有幂等加列助手 `ensure_text_column`（`src/conversation_store.rs:92`：`PRAGMA table_info` 检查 + `ALTER TABLE … ADD COLUMN … TEXT NOT NULL DEFAULT ''`），**不引入迁移框架**。因此“空串 = 未指定”——读回 `''` 时必须走 §6.1 的 `fallback`，不得当成 `approved` 落死，也不得拒绝加载旧会话。

### 6.3 审批结果的作用域：前缀键（已定）

现状的会话级 persistent allowlist 是**平铺命名空间**：`http_fetch:<url>`、`http_request:<METHOD>:<url>` 带前缀，但 `run_command` 的 `AllowAlways` 存的是**裸命令名**（`execute_run_command_approval.inc.rs:277`）。引入等级后必须防止“高等级批过的键在低等级继续放行”。

**决定：给键加等级前缀，存储类型不变**（仍是 `Arc<Mutex<HashSet<String>>>`）。这是第一优先级方案——不新增结构体、不改序列化、不动调用方签名；该集合是**内存态**（`stream_job_setup.rs:162` 以空集初始化），无磁盘 schema，故**零迁移成本**。

| 来源 | 现有键 | 加前缀后 |
|---|---|---|
| `run_command` / `terminal_session` | `git` | `l3\|git` |
| `http_fetch` | `http_fetch:https://…` | `l3\|http_fetch:https://…` |
| `http_request` | `http_request:POST:https://…` | `l3\|http_request:POST:https://…` |

规则：

1. **写入用 `effective` 等级**：键 = `format!("l{}|{}", effective as u8, key)`。
2. **命中要求等级严格相等**（不是 `>=`）：`stored_level == effective_level`。降级即失效、无需主动清理；用 `>=` 会让 L3 批过的键在 L2 继续命中，而 L2 的整个意义就是“人工例外通道关闭”（`http_tool_url_allowed` 在 L2 下也会查这个集合，正是泄漏点）。
3. **只有 `approval = Gate` 的等级才查这个集合**：`Deny`（L0–L2）直接拒、`AuditOnly`（L4）不需要（`command_scope = Any`）。
4. **写入前缀恒为 `effective`，与 ceiling 无关**：ceiling 只决定 `effective` 的上界（§6.1），所以不存在“批一个更高等级的键”这种操作——`AllowAlways` 最多写下当前 `effective` 的前缀（配合 MB-1）。

### 6.4 沙盒：只钳路径与网络，不降等级

`sync_default_tool_sandbox_mode = docker` 时**不**把等级钳到 L2——那样会把 L3 的「越界 → 审批」变成 L2 的「越界 → 直接拒」，即**为了更安全打开沙盒，结果白名单外命令从“弹审批”变成“报错”**（隔离机制误伤成权限降级）。

沙盒的问题不在“能不能多干”，而在**路径语义**：容器里只有 `/workspace`（`tool_sandbox/mod.rs:82`）。因此只钳两轴：

| 轴 | 沙盒下 | 理由 |
|---|---|---|
| `level` / `approval` | **不动** | L3 的审批弹窗在宿主侧 SSE，与执行位置无关，容器内同样有效 |
| `path_scope` | **无需钳**（L0–L3 本就是 `SessionRoot`） | 只有 L4 是 `Any`，而 L4 × 沙盒已在启动期**拒绝组合**；因此不存在“路径被钳”的情形 |
| `net_scope` | 按 `sync_default_tool_sandbox_docker_network` 收窄 | 这是沙盒**唯一**真正生效的钳制：未配网络名（默认）→ 容器无出网 → `None`；配了则沿用 `Prefixes` |

L4 + 沙盒说不通（要宿主级就别进容器），处理方式是**拒绝组合**（启动期直接报错，二选一），而不是偷偷把 L4 降成 L2。因此 `clamped_by` 里的 `sandbox` **只在 `net_scope` 被收窄时出现**（典型：L3 + Docker + 未配网络名），不静默。

---

## 7. `PermissionCaps` 投影

### 7.1 结构

```rust
// src/cm_types/permission_level.rs —— 等级本身（对齐 session_mode.rs 的形）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Chat,          // L0
    Read,          // L1
    Workspace,     // L2
    #[default]
    Approved,      // L3（= 现状）
    Unrestricted,  // L4
}

// src/cm_agent/permission.rs —— 纯投影（无 IO、无 cfg 之外依赖，可 golden）
pub enum ToolVisibility {
    None,
    /// 由**独立正列表** `permission_read_tools` 判定；**不读** `is_readonly_tool`
    /// （后者是并行只读批的性能判据，且其写集合可被配置整体替换，见 §7.2 注）。
    Readonly,
    Full,
}

/// 不受 `caps` 约束的**外部工具**：MCP 代理（`mcp__*`）与工作区动态工具（`plugins/*.json`）。
/// 它们的实际能力由外部进程决定，宿主侧的路径 / 命令 / 网络三轴**管不到**，
/// 所以不能用“L2 是硬边界”去掩盖对它们的放行——L0–L2 一律 `Denied`。
pub enum ExternalTools {
    Denied,
    Allowed,   // L3 / L4：由 MCP server 自身与审批策略约束（§8.1）
}

pub enum PathScope {
    /// 仅当前会话工作区根（现状；L0–L3）。
    SessionRoot,
    /// 会话根 + `workspace_allowed_roots`（S6）。
    AllowedRoots,
    /// 任意路径（L4）。
    Any,
}

pub enum CommandScope {
    None,                       // L0 / L1：命令全禁
    Allowlist(Arc<[String]>),   // 直接复用 AgentConfig 的 Arc，不复制
    Any,                        // L4
}

pub enum NetScope {
    None,                       // L0：出站全禁
    Prefixes(Arc<Vec<String>>),
    Any,                        // L4
}

pub enum ApprovalPolicy {
    Deny,       // L0–L2：越界即拒
    Gate,       // L3：SSE 人工审批（现状）
    AuditOnly,  // L4：放行 + 审计
}

/// 单回合一处的**唯一**能力快照；既有 `PolicyDecision` 改为**嵌入**本结构（§7.4）。
pub struct PermissionCaps {
    pub level: PermissionLevel,
    pub tool_visibility: ToolVisibility,
    pub path_scope: PathScope,
    pub command_scope: CommandScope,
    pub net_scope: NetScope,
    pub approval: ApprovalPolicy,
    /// 外部工具（MCP / 动态工具）是否可用；`Denied` 时 `mcp__*` 与 `plugins/*.json`
    /// 不进 `tools` 列表，且在 early-deny 处直接拒。
    pub external_tools: ExternalTools,
}
```

### 7.2 五级 × 六轴投影

| 级 | `tool_visibility` | `external_tools` | `path_scope` | `command_scope` | `net_scope` | `approval` |
|---|---|---|---|---|---|---|
| `chat` | `None` | `Denied` | —（不可达） | `None` | `None` | `Deny` |
| `read` | `Readonly` | `Denied` | `SessionRoot` | `None` | `Prefixes` | `Deny` |
| `workspace` | `Full` | **`Denied`** | `SessionRoot` | `Allowlist` | `Prefixes` | `Deny` |
| `approved` | `Full` | `Allowed` | `SessionRoot` | `Allowlist` | `Prefixes` | `Gate` |
| `unrestricted` | `Full` | `Allowed` | `Any` | `Any` | `Any` | `AuditOnly` |

注意 **L4 的“绕过”全是复用现有开关**，不需要新机制：

- `command_scope = Any` → 等价于 `allowed_commands = ["*"]`，`allowlist_allows_all` 已支持（`command_shell_script.rs:29`）；
- `path_scope = Any` → 等价于 `skip_arg_safety = true` + 文件工具走 `resolve_any`（§8.3）；
- `net_scope = Any` → 等价于 `http_fetch_allowed_prefixes = ["*"]`（默认已是此值）。

> `ToolVisibility::Readonly` 的判据是**独立正列表** `permission_read_tools`（默认 = `builtin_write_effect_tools()` 的补集，见 `registry_policy.rs:105`），**不受** `tool_registry_write_effect_tools` 覆盖影响。那个键是性能开关（整体替换式）；拿它当安全边界，运维为调并行漏配 `run_command` 就会让 L1 直接放行子进程。

### 7.3 为什么不写 `if level >= Approved`

仓库有硬门禁：**函数 CCN ≤ 10**（`scripts/lizard-rust.sh`）、**形参 ≤ 9**（`scripts/fn-param-ratchet.sh`）。若每个执行点都写 `match level { … }`，`evaluate_policy` 这类已有多分支函数会立刻顶破 CCN。

因此规定：

- **只有 `src/cm_agent/permission.rs` 允许 `match level`**（一处投影）；
- 所有执行点只读 `caps.*` 的**小枚举**分支（每个 `match` 2–3 臂）；
- 新增能力轴时，先在 `PermissionCaps` 加字段 + 补 golden，再改执行点——**禁止**在执行点里读 `cfg.permission_level`。

### 7.4 与既有能力快照合并（避免两套）

`execute_run_command_approval.inc.rs:134` 已有同形快照：

```rust
struct PolicyDecision {
    allowed_commands: Arc<[String]>,
    skip_arg_safety: bool,
    capabilities: PolicyCapabilities,   // { mode: ExecMode, has_approval_channel }
    approvals: Vec<PolicyApproval>,
}
```

**不新增第二套**。改为**嵌入**——`caps` 是唯一真源，其余全是派生：

```rust
struct PolicyDecision<'a> {
    caps: &'a PermissionCaps,   // 单一真源：白名单 / 路径 / 网络 / 审批策略
    skip_arg_safety: bool,      // 派生：path_scope 越界 + 审批结果
    approvals: Vec<PolicyApproval>,
    // ExecMode（Sync / Async / Docker / Pty）留在 PolicyCapabilities：
    // 它是**执行形态**，不是权限面；混进 caps 会让投影表多出一个与等级无关的维度。
}
```

这条同时消掉 `registry_policy.rs` / `step_executor_policy.rs` 里“再算一遍”的冲动：**任何能力判断都以 `caps` 为准**，`is_readonly_tool` 退回它原本的职责（并行批与只读 TTL 缓存）。

---

## 8. 执行点接线

### 8.1 改动表

| 层 | 文件 | 改动 |
|---|---|---|
| 类型 | 新建 `src/cm_types/permission_level.rs` | 枚举 + `ALL` / `as_str` / `FromStr` / `Display` / `Ord` |
| 配置 | `config/default_config.toml`、`cm_config/builder*`、`cm_config/env_overrides*.rs` | `permission_level`、`permission_level_max`、`CM_PERMISSION_LEVEL*` |
| 热更 | `src/cm_config/hot_reload.rs:20` | `permission` 显式登记进**逐字段**白名单 + 启动期 ceiling 冻结（§9.1）；不登记则连降级都不会热更 |
| 角色 | `config/agent_roles.toml` + `cm_config/agent_role_spec.rs` | 可选 `max_permission_level` |
| 解析 | 新建 `src/cm_agent/permission.rs` | `resolve_permission_level_for_turn` + `project_caps` |
| 装配 | `src/agent/agent_turn/host/params.rs:65`（`RunLoopAttach`） | 加 `pub permission: PermissionCaps` |
| 可见性 | `turn_loop/outer_loop.rs:101`（`build_planner_round_tools`） | 先按 `caps.tool_visibility` 过滤；`None` → 空表 |
| 准入 | `policy/tool_execution.rs:93`（`tool_policy_early_deny_message`） | 新增 permission 分支（`None` → 全拒；`Readonly` → 按**独立正列表** `permission_read_tools` 拒，**不读** `is_readonly_tool`） |
| 命令 | `execute/execute_run_command_approval.inc.rs:147`（`evaluate_policy`） | `PolicyDecision` 改为**嵌入** `caps`（§7.4）；`command_scope = None` 时**在 S2 即硬拒** `run_command` / `terminal_session` / `python_snippet_run` / `run_executable`（补上 `is_readonly_tool` 可能漏的口子） |
| PTY | `execute/execute_terminal_session.inc.rs` | 同上；`command_scope = None` 时整个工具不出现（已被可见性挡住，双保险） |
| 路径 | `tools/file/path.rs:127/159` | 加 `PathScope` 形参；`PathScope::Any` 走新 `resolve_any_path` |
| 路径 | `execute/mod.rs:134`（`approve_external_read_dir_if_needed`） | `path_scope = Any` → 跳过审批；`approval = Deny` → 直接拒（不再报“审批通道不可用”） |
| 网络 | `execute/http_tool_exec.inc.rs:56`（`http_tool_preflight`） | `net_scope` 与 `approval` 分支；`Any` → 不查前缀；`None` → 拒；`Prefixes` → 白名单 + `Gate`（§6.3 的等级前缀命中） |
| 审批 | `src/cm_approval/mod.rs:26` | 新增 `ApprovalPolicy` 消费点与 `SensitiveCapability` 无关；只在“是否需要弹窗”处判断 |
| 观测 | `self_config_info`、`GET /status` | 新增 `permission` 小节（已有 `sections` 机制） |
| Workflow | `cm_workflow/run.rs:228`（`WorkflowToolExecCtx`）+ `execute/node.rs:36` | 节点的 `allowed_commands` / 前缀 / 路径 scope 改由 **`caps` 派生**，不再从 `cfg` 直取；否则 L2 的硬边界会被 `WorkflowGate` 审批绕过 |
| MCP / 动态工具 | `cm_agent/step_executor_policy.rs:98`、`cm_tools/tool_naming.rs` | 现状：`mcp__*` **一律放行**（`step_executor_policy.rs:98` 直接 `return true`）且非只读。改为按 `caps.external_tools`：`Denied`（L0–L2）→ 不进 `tools` 且 early-deny；`Allowed`（L3/L4）→ 维持现状 |

> 接线原则：**所有改动都是“换掉入参来源”，不重写 gate 逻辑**。`evaluate_policy` 的三道 gate 顺序、弹窗次数、错误文案在 L3 下逐字不变（这也是 S3 的验收标准）。

### 8.2 L0 的编排侧特例（容易漏）

L0 没有工具，会让“要求工具证据”的编排逻辑反复重试。必须同时处理：

1. `run_dispatch.rs:34` 的只读挂载不必走（`tool_visibility = None` 已足够强）；
2. `cm_agent/acceptance/` 与 `final_plan_gate` 中**依赖工具证据**的校验降级为跳过，仅保留文本形态校验（`final_plan_requirement` 按 `none` 处理）；
3. `intent_at_turn_start` 的 Act 句启发式跳过（避免“以为要去改代码”而产出空工具调用循环）；
4. `per_plan_semantic_check`（无工具侧向调用）可保留——它不消耗工具能力。

实现上建议：在 `assess_turn_routing` 前加一个 `if caps.tool_visibility == None { … }` 的短路，而不是散改各 gate。

### 8.3 L4 的路径放行细节

`PathScope::Any` 不等于“无校验”。规则：

1. 新增 `resolve_any_path(base, sub)`：不做根内 `starts_with` 校验，但仍走 `canonicalize`（拒绝不存在的祖先链、解析 symlink 真实位置），并在日志中记录 `canonical` 结果。
2. **默认不设敏感前缀黑名单**（`permission_deny_system_paths = []`）：L4 的承诺是“不受工作区路径限制”，带默认黑名单的 `unrestricted` 名实不符。需要加固的人**显式**填写（推荐 `["/proc", "/sys", "/dev"]`）。`workspace_roots.rs:4` 的同款黑名单服务于工作区**选址**，语义不同，不要顺带复用。
3. 输出仍按“相对工作区路径优先”展示（`path_for_tool_display`），工作区外才回退绝对路径——避免把宿主绝对路径大量灌进模型上下文（也顺带减 token）。

---

## 9. 安全边界（硬规则）

| 编号 | 规则 |
|---|---|
| **MB-1** | 请求体 `permission_level` 只能是**降级**；> `min(ceiling, role cap)` 时钳制到上限，并在 SSE 时间线与 `tracing` **同时**记一条“请求等级被钳制”。禁止静默升级（本设计里不存在升级路径）。 |
| **MB-2** | `permission_level_max` 只来自服务端（显式 TOML / `CM_PERMISSION_LEVEL_MAX` / §5.1 的启动期推导）。**不得**加入 `/chat` 请求体、`client_llm`、`saved_model_*` 或任何客户端可控槽位。`POST /config/reload` 可**降**（即时生效）不可**升**（启动期冻结，实现形态见 §9.1）。 |
| **MB-3** | 非 loopback 绑定时 **ceiling 推导值固定为 `approved`**（不因“没配”而变 `unrestricted`）；若**显式**配置 `permission_level_max = "unrestricted"` 而缺少 `CM_ALLOW_UNRESTRICTED_PERMISSION_FOR_NON_LOOPBACK=1`，则**启动失败**。共享 Bearer 的 Web 部署不得默认拿到 L4。 |
| **MB-4** | **L4 不免除不可逆操作确认**：工具级默认见 `permission_confirm_tools`（`["delete_dir", "delete_files"]`）；`run_command` 的危险 argv（`git reset --hard` / `git push --force`）由 policy 单独判定。L4 只免除“路径与命令白名单”，不免除“删库确认”。 |
| **MB-5** | **审计必须脱敏**：L4 的 argv / URL 审计走 `src/cm_tools/redact.rs`，不得原样落日志（AGENTS.md → Secrets & logging 为硬约束）。 |
| **MB-6** | **沙盒钳制的两轴不可静默**：§6.4 被钳的 `path_scope` / `net_scope` 必须出现在启动日志与 `GET /permission` 的 `clamped_by`（含 `sandbox`）。 |
| **MB-7** | **L0 不得被绕过**：即便 `tools` 数组为空，执行侧仍有 early-deny（§8.1）；两层都生效，防止模型伪造 `tool_calls` 直穿。 |
| **MB-8** | **二级执行面不得回读 cfg**：`workflow_execute` 节点、MCP 代理、动态工具的能力参数必须来自 `caps`，禁止在 `cm_workflow` / `cm_mcp` 里重新从 `AgentConfig` 取 `allowed_commands` / `http_fetch_allowed_prefixes`——否则 L2 的“硬边界”只是第一层装饰。 |

### 9.1 ceiling 的可变更性（实现形态）

`permission_level_max` 的生命周期被刻意绑在“**进程物理在场**”上：调 HTTP 只要共享 Bearer（与普通聊天同一把钥匙），重启要 shell / systemd / 部署权限——**两者不是同一个能力**。如果 `/config/reload` 能抬上限，远端就等于能自我提权，MB-1/MB-3 全部失效。

实现形态照抄 `src/cm_config/hot_reload.rs:21` 已有的「全放行哨兵」先例——那里已用同样手法冻结 `serve --yes` 写入的 `allowed_commands = ["*"]`：

```rust
// 拟新增（与 hot_reload.rs:21-32 同形）
let ceiling_at_startup = dst.permission.permission_level_max;
dst.permission.clone_from(&src.permission);
// 只降不升：磁盘/环境变量把上限写高也不认，直到下次 serve 启动
dst.permission.permission_level_max =
    ceiling_at_startup.min(dst.permission.permission_level_max);
```

要点：

1. **`permission` 必须显式登记进 `apply_hot_reload_config_subset`**。该函数是**逐字段白名单**（不是整体替换），不登记则连“降级”都不会热更——与“默认热更”的直觉相反，容易踩。
2. **`CM_PERMISSION_LEVEL_MAX` 与 `API_KEY` 同规**：重载**不从环境变量重读**（`hot_reload.rs:8` 已确立），所以走环境变量抬上限天然是启动期生效。
3. **降级即时生效**：`POST /config/reload` 把上限降到 `workspace` 后，下一回合所有会话立即被 §6.1 的 `min()` 收紧——这是唯一允许的“运行时收紧”方向（fail-safe）。
4. **`permission_level`（默认等级）可自由热更**，不必冻结：它只是 fallback，且始终再受 ceiling 钳制。
5. **推导值同样在启动期冻结**：`ceiling_source = derived_*` 时，热重载不得把推导结果抬得更高；重载后若磁盘新增了显式 `permission_level_max`，仍须过上面同一个 `min()` 比较。

---

## 10. 配置与环境变量

| 键 | 环境变量 | 默认 | 说明 |
|---|---|---|---|
| `permission_level` | `CM_PERMISSION_LEVEL` | `approved` | 会话默认等级；可被请求降级 |
| `permission_level_max` | `CM_PERMISSION_LEVEL_MAX` | **推导**（loopback → `unrestricted`；非 loopback → `approved`） | 服务端上限；显式配置则覆盖推导（§5.1 / §5.3） |
| `permission_chat_control_tools` | `CM_PERMISSION_CHAT_CONTROL_TOOLS` | `[]` | L0 下允许的零副作用控制面工具（如澄清问卷），仅 `chat` 级生效 |
| `permission_read_tools` | `CM_PERMISSION_READ_TOOLS` | **推导**：`builtin_write_effect_tools()` 的补集（`registry_policy.rs:105`） | L1 的只读**正列表**；显式写出即覆盖推导。**不受** `tool_registry_write_effect_tools` 影响（§4.2 / §7.2） |
| `permission_deny_system_paths` | `CM_PERMISSION_DENY_SYSTEM_PATHS` | `[]` | 仅 L4 生效的**可选**敏感前缀加固（默认关，§8.3） |
| `permission_confirm_tools` | `CM_PERMISSION_CONFIRM_TOOLS` | `["delete_dir", "delete_files"]` | 任何等级都不豁免的不可逆操作确认；`run_command` 的危险 argv 另由 policy 判定（MB-4） |
| `allow_unrestricted_permission_for_non_loopback` | `CM_ALLOW_UNRESTRICTED_PERMISSION_FOR_NON_LOOPBACK` | `false` | 非 loopback 下显式允许把 ceiling 提到 `unrestricted`（§5.3 / MB-3）；与既有 `allow_insecure_no_auth_for_non_loopback` 同形 |
| `[[agent_roles.roles]].max_permission_level` | — | 省略 = 不额外限制 | 角色级上限（TOML only，角色是服务端资产） |

校验规则（`cm_config/validate.rs` 风格）：

- 非法等级串 → **启动期报错**（不是 warn + 回退），避免“以为被限制其实没限制”。
- `permission_level > permission_level_max` → warn **并按 max 生效**（可解释，不阻断启动）。
- `permission_chat_control_tools` 里的名字须能在 `tool_specs_registry` 找到且属零副作用类，否则报错。

---

## 11. 观测与对外契约

1. **`GET /permission` / `GET /status` / `self_config_info`**：等级与投影的统一出口。`GET /permission` 的响应形状见 §16.2；`self_config_info` 的 `permission` 小节与其**同源序列化**（同一个 `PermissionCaps`），避免两处漂移。密钥类字段一律不出现（沿用现有“只报是否设置”的规矩）。
2. **`tracing` 结构化字段**：每条被等级拦截或放行的关键调用记 `permission.level`、`permission.axis`（`path` / `command` / `net` / `external` / `tool`）、`permission.decision`（`allowed` / `denied` / `approved` / `clamped`）。
3. **SSE（可选，需 Client 同步）**：若采纳审计事件，建议复用现有 control-plane 而不是新增顶层键 —— 在 `tool_call` 时间线条目上附 `permissionLevel` 字段即可，**不改 SSE_PROTOCOL_VERSION**。若确需独立事件，须按 AGENTS.md 的 checklist 同步 `src/cm_sse_protocol/control_classify.rs`、`fixtures/sse_ag_ui_golden.jsonl`、`docs/SSE协议.md` 与 Client `parser_v2.rs` / `sse_dispatch/types.rs`，并跑双端 golden 测试。
4. **Client（另仓）**：`crabmate-client` 侧在 `session_mode` 选择器旁加等级选择（只列 ≤ ceiling 的项），L4 需二次确认文案。**本次不涉及本仓 `frontend/`（本仓没有前端源码）。**

---

## 12. 实施切片

| 切片 | 内容 | 行为变化 | 可独立合并 |
|---|---|---|---|
| **S1** | 类型 + 配置键 + **ceiling 按部署形态推导**（§5.1）+ **`agent_roles.max_permission_level` 解析**（§6.1 的 role cap 依赖它）+ `resolve_permission_level_for_turn` + `project_caps` + golden + **热重载登记与启动期 ceiling 冻结**（§9.1）（**不接线**） | **无** | ✅ |
| **S2** | `tool_visibility` + early-deny（L0 / L1 生效）+ **`command_scope = None` 硬拒**（`run_command` / `terminal_session` / `python_snippet_run` / `run_executable`） | 仅显式设 `chat` / `read` 时 | ✅ |
| **S3** | 命令白名单 / 路径 / 网络三轴接线（L2 硬边界、L4 放行）+ **`caps` 进 `WorkflowToolExecCtx`** + **`external_tools` 接线**（L0–L2 拒 `mcp__*` / 动态工具）+ L0 编排短路（§8.2） | 仅显式设 `workspace` / `unrestricted` 时 | ✅ |
| **S4** | `ApprovalPolicy::AuditOnly` + 审计事件 + allowlist 等级作用域（§6.3） | 仅 L4 时 | ✅ |
| **S5a** | `GET /permission` 只读投影（§16.2，含 `?agent_role=`）+ OpenAPI + `docs/命令行与路由.md` | 无 | ✅ |
| **S5b** | `POST /chat` 的 `permission_level`（含 `chat_keys` 字母序）+ `GET /conversation/messages` 的 `active` / `effective` 双字段（§16.1） | 客户端不传时无变化 | ✅ |
| **S5c** | `GET /status` 两个默认值 + `StatusShellView` golden 更新 + `GET /tools` 的 `available_at_level`（§16.4） | 无 | ✅ |
| **S5d** | 文档同步（§14）+ Client（另仓）等级选择器、`clamped_by` 文案与 L4 二次确认 | 无 | ✅ |
| **S6** | 可选 `PathScope::AllowedRoots`（见 §15 Q4） | 仅显式配置多根时 | ✅ |

排序理由：**R1–R3（§17）先于 S1**——它们是纯等价重构，把 S3 在文件类工具上的改动面从 20+ 调用点压到「1 个载体 + 2 个构造宏」，外加 file 模块外零星调用点（见 §17.5 实测表）；先做权限再重构等于同一批 call site 改两遍。S1 零行为变化，先把类型与投影固化并用 golden 钉住；S3 是唯一“可能悄悄改变现有行为”的切片（因为它改 `evaluate_policy` 入参），必须放在 S1/S2 之后并附 L3 逐字等价回归。S5a 先于 S5b：只读投影能让客户端在“会话级写入”上线前就渲染出选择器（选项集来自 `available`），不必等写路径。

---

## 13. 测试计划

| 层 | 用例 |
|---|---|
| 单元 | `resolve_permission_level_for_turn` 的 min 链矩阵：请求 > ceiling、请求 > role cap、`request=None` 走 `persisted`、`persisted` 高于新 ceiling 时被重算（§6.2）、非法串报错 |
| 单元 | `project_caps` 五级 golden（对 `PermissionCaps` 全字段快照） |
| 单元 | `parse_permission_level` 大小写 / 空串 / 未知值（对齐 `session_mode` 现有测试风格） |
| 集成 | `tool_policy_early_deny_message` 在 L0/L1/L3/L4 × `read_file`/`run_command`/`http_request` 的 12 组矩阵 |
| 集成 | `evaluate_policy` 在 L2（未命中→`Denied`，无审批弹窗）/ L3（逐字等价现状）/ L4（`Any`，永不弹窗）—— 直接扩 `execute_run_command_gate_tests.rs` |
| 集成 | `resolve_for_read` / `resolve_for_write` 在 `PathScope::Any` 下接受绝对路径与 `..`；L2 下仍拒（现有断言保留不动） |
| 安全 | `tests/permission_matrix.rs`：`ceiling` 钳制、非 loopback 启动校验（MB-3）、allowlist 等级作用域不跨级命中（§6.3）、L4 仍触发 MB-4 确认 |
| 单元 | **ceiling 推导三态**：`bind_ip.is_loopback()` → `unrestricted`；非 loopback → `approved`；显式配置时 → `config`（`ceiling_source` 随之变化） |
| 集成 | `GET /permission?agent_role=<id>` 的 `available` 随角色 cap 收窄；缺省用 `default_agent_role_id` |
| 安全 | **L1 不依赖 `is_readonly_tool`**：故意配置 `tool_registry_write_effect_tools` 漏掉 `run_command` 后，L1 仍须拒它（防性能开关变安全开关） |
| 安全 | **二级执行面**：L2 下 workflow 节点内带 `..` 的 `run_command` 须**拒**（不得弹审批放行）；L2 下 `mcp__*` 与动态工具被拒 |
| 集成 | 沙盒 + L3：`approval` 仍为 `Gate`（不被降成 `Deny`），`path_scope` 被钳到 `SessionRoot` |
| 回归 | 全量 `cargo test`（改了 `cm_config` 与跨模块公共类型，按 AGENTS.md 须全量） |

---

## 14. 文档同步清单（AGENTS.md 硬要求，实现时逐条勾）

- [ ] `README.md`：新增权限等级速查（默认值、如何开 L4、非 loopback 的额外开关）。
- [ ] `docs/配置说明.md`：新增「权限等级」小节，位置紧邻「会话工作模式（`session_mode`）」；逐键写默认值与生效顺序。
- [ ] `docs/工具说明.md`：`run_command` / `terminal_session` / `read_file` / `write` / `read_dir` / `http_fetch` / `http_request` 的“行为随等级变化”段落。
- [ ] `docs/开发文档.md`：新增 `src/cm_agent/permission.rs` 与 `cm_types/permission_level.rs` 触发「主要模块」表更新；若 `RunLoopAttach` 字段增加导致执行面数据流变化，同步「架构设计」小节。
- [ ] `docs/命令行与路由.md`：`POST /chat*` 新增 `permission_level`；`GET /conversation/messages` 新增 `active_permission_level` / `effective_permission_level`；新增 `GET /permission`；`GET /status?view=shell` 新增两个默认值字段。
- [ ] `src/web/openapi/openapi_paths.rs` + `fixtures/status_shell_view_golden.json`：新端点与 `StatusShellView` 新字段必须同步 OpenAPI 与 golden（该结构带 `deny_unknown_fields`，不改 golden 会直接红）。
- [ ] `docs/SSE协议.md`：仅当采纳独立审计事件时（§11）。
- [ ] `docs/待办清单.md`：与第 105 条「危险操作分级与确认」、第 141 条「多 profile」合并；实现完成后删除这两条（不要留 `- [x]`）。

---

## 15. 取舍记录（原「未决问题」，均已结案）

| # | 问题 | 建议 |
|---|---|---|
| **Q1** | 默认等级是否在**新装**环境从 `approved` 改为 `workspace`？ | **已结案**：保持 `approved`（= 现状）；“更安全”诉求由显式 `permission_level_max = "workspace"` 表达，不动默认等级 |
| **Q2** | L4 是否允许出现在 Web？ | **已结案**：loopback 默认允许（推导 `unrestricted`，客户端可自由切）；非 loopback 需显式配置 + `CM_ALLOW_…` 开关（§5.3 / MB-3） |
| **Q3** | `session_mode` 是否折进等级？ | **已结案：不折**（§4.3）。二者职责不同（一个决定提示词附录与编排姿态，一个决定能力天花板），`min()` 组合足够 |
| **Q4** | 是否实现 `PathScope::AllowedRoots`（S6）？ | **已结案：做**（S6）。能直接消掉 `../crabmate-client` 兄弟仓每次都要审批（AGENTS.md 明确要求本地同级仓）的摩擦 |
| **Q5** | L4 下是否保留 `permission_deny_system_paths`？ | **默认清空**（§8.3）：`unrestricted` 名实要相符；加固由运维显式填写 |
| **Q6** | `AllowAlways` 的等级作用域用前缀键还是结构化元组存储？ | **已结案：前缀键**（§6.3），作为**第一优先级方案**：沿用现有 `HashSet<String>`，不加新类型、不改序列化、不动调用方签名；集合是内存态，零迁移 |
| **Q7** | `GET /tools` 用每项 `available_at_level: bool`，还是 `?permission_level=` 过滤？ | **已结案：`available_at_level: bool`**（§16.4）：`GET /tools` 是“枚举 + 策略投影”，不是“本回合可用集” |
| **Q8** | L2（`workspace`）下是否禁用 `mcp__*` 与 `plugins/*.json` 动态工具？ | **已结案：禁用**（`external_tools = Denied`，§7.1）。理由：MCP 能任意读写宿主，与“硬边界”承诺直接矛盾。将来若要放行，应把外部工具拆为独立可选项并同步改 L2 的文案，而不是让 L2 名不副实 |
| **Q9** | `PathScope::Workspace` / `Unrestricted` 与 `PermissionLevel::Workspace` / `Unrestricted` 同名不同义，是否改名？ | **已结案：改名**：`PathScope::{SessionRoot, AllowedRoots, Any}`、`CommandScope::{None, Allowlist, Any}`、`NetScope::{None, Prefixes, Any}`（已在 §7.1 / §7.2 落地） |

---

## 16. 客户端管理面（等级的使用面）

「等级管理 API」不是一个接口，而是三件性质不同的事。照 `session_mode` 的现成先例办，**不新造一套管理方式**。

### 16.1 三段式：会话等级（≤ ceiling）

现状 `session_mode` 没有专用端点，靠三段式流转；`permission_level` 完全对齐（锚点为写作时快照）：

| 段 | 位置 | `permission_level` 的改动 |
|---|---|---|
| 写 | `POST /chat`（含 `/chat/stream`、`/chat/async`）顶层字段 | 加 `permission_level`，并登记进 `src/cm_api_contract/chat_keys.rs:6` 的**字母序**白名单（插在 `"message"` 与 `"readonly_tool_ttl_cache_secs"` 之间——该表用 `binary_search`，位置错会**静默失效**） |
| 持久化 | conversation store | 加 `active_permission_level`（**只存意图**，每回合按 §6.2 重算） |
| 读 | `GET /conversation/messages` | 加 **两个**字段：`active_permission_level`（意图）+ `effective_permission_level`（min 后生效值）；同时改 `src/cm_api_contract/chat.rs:194`（OpenAPI 形状）与 `:350`（实现体） |
| 默认 | `GET /status?view=shell` | 加 `permission_level_default` + `permission_level_max`（两个**标量**属壳层稳定子集，加且更新 golden；六轴投影不进，见 §16.2） |

**为什么必须回两个字段**：`session_mode` 没有 min 链，而等级会被 ceiling / role cap / `ask`·`plan` / 沙盒四重钳制。只回显一个值，就会出现“我明明切了 L4，怎么还是只读”，且无从排查。

**为什么不加 `POST /conversation/{id}/permission_level`**：`POST /workspace` 之所以是独立端点，是因为它切的是**执行上下文**（工作区根），与请求内容无关；而等级回答的是“这一轮允许什么”，与请求同生共死。且服务端已持久化 `active_*`，客户端不传即沿用上次——UI 的“顶部开关持续生效”在客户端即可实现（Client 现在正是这样处理 `session_mode` 的）。

### 16.2 只读投影：`GET /permission`（新增）

客户端要渲染“哪些等级可选 / 为什么不能选 L4”，需要一个策略投影端点：

```json
{
  "level": "approved",
  "level_source": "config",
  "ceiling": "unrestricted",
  "ceiling_source": "derived_loopback",
  "available": ["chat", "read", "workspace", "approved", "unrestricted"],
  "clamped_by": [],
  "caps": {
    "tool_visibility": "full",
    "external_tools": "allowed",
    "path_scope": "session_root",
    "command_scope": "allowlist",
    "net_scope": "prefixes",
    "approval": "gate"
  }
}
```

- `available` 直接是选择器选项集（`≤ ceiling`，并已扣除 role cap 与 `session_mode` 钳制）。
- `clamped_by` 取值 `ceiling` / `role` / `session_mode` / `sandbox`，空数组表示未被钳制；它是“为什么不能选 L4”的唯一答案。
- `ceiling_source` 取值：`config`（显式配置）/ `derived_loopback` / `derived_non_loopback`（§5.1）；客户端据此向用户解释“为什么本机能选 L4、远端不能”。
- **`?agent_role=` 参数（必须）**：`available` 依赖 role cap，而 `agent_role` 是 `/chat` 请求体字段。缺省时用 `cfg.roles_prompts.default_agent_role_id`；客户端在首轮之前必须带上即将使用的 `agent_role`，否则会拿到偏松的 `available` 再被服务端钳制——“先选错再纠正”是错的体验，也是客户端无法像主流 agent 那样一次性列出全部模式的根因。
- **`?conversation_id=` 参数（必须，除非只要静态投影）**：不带它就拿不到 `active_permission_level`，`level_source = persisted` 无法成立。带上时按 §6.1 的 min 链算出该会话的 `effective` 与真实 `caps`。
- `caps` 与 `self_config_info` 的 `permission` 小节**同源序列化**（同一个 `PermissionCaps`），避免两处漂移；但**两者语义上都是“静态投影”**：不含回合内 `AllowAlways` 已批准的键、不含 `run_command` 的 `skip_arg_safety` 等派生项（那些只在 `PolicyDecision` 内部）。
- **落点**：`src/web/routes/system/mod.rs:13` 已有 `/status`，毗邻新增（或独立 `src/web/routes/permission/mod.rs`），并登记到 `src/web/openapi/openapi_paths.rs:54` 一带。
- **不塞进 `StatusShellView`**（`src/cm_api_contract/status.rs:10`）的理由：它的定位是“Web 壳层稳定子集”，塞六轴投影会让它名不副实（加两个标量默认值仍可接受，见 §16.1，golden 相应更新）。

### 16.3 严禁：写 `ceiling` 的接口

**故意不提供**（不是“暂未实现”）：

> 只要存在能写 `permission_level_max` 的 HTTP 接口，§9 MB-3 的启动期门禁就形同虚设——远端先能把上限改到 `unrestricted`，再请求 L4，整套分级归零。

且 **API 层拦不住**这件事：Web 鉴权是共享 Bearer（`web_api_bearer_token`），没有“谁是谁”的语义。所以“谁能用 L4”当前**只能**由**启动期的 ceiling**（显式配置或 §5.1 的 `derived_*` 推导）+ 非 loopback 开关（MB-3）决定；等有了按用户身份的鉴权体系（`docs/未来规划功能.md`）才谈得上“给管理员开提权接口”。

同理，`POST /config/reload` 对 `permission_level_max` 只允许**降**（fail-safe），升需重启（MB-2）。

### 16.4 一致性：`GET /tools` 与等级

`GET /tools`（见 [`tool_management_api.md`](tool_management_api.md)）现在枚举三源全部工具。等级会改变可见性，若它仍列全部，客户端会展示一批实际调不动的工具，形成“命令行能看到、UI 能看到、实际被拒”的排查黑洞。

**已定（Q7）**：每项附 `available_at_level: bool`，保留全量枚举，**不**引入 `?permission_level=` 过滤——`GET /tools` 的定位是“枚举 + 策略投影”，不是“当前回合可用集”。

### 16.5 审批弹窗也要带等级

`command_approval_request` 的载荷（`src/cm_internal/tool_approval/approval_specs.rs`）应附当前 `level`，让用户在点 `AllowAlways` 前看到“这是 L3 会话在请求越界”，而不是只看到命令本身。实现上与 §11 第 3 条一致：**复用现有 control-plane 字段，不改 `SSE_PROTOCOL_VERSION`**。

---

## 17. 实施前置重构（R1–R3）

三处「切点分散」会让 S3 在同一批调用点上改两遍。先做三个**纯等价重构**（零行为变化、现有测试全绿），把「改 N 处」压成「改 1 处」。

### 17.1 R1 路径访问收口（✅ 已完成）

**落地结果**（与本节原估算的差异以实测为准）：

- 载体：`file::PathAccess`，仅持 `root`；`for_read` / `for_read_open` / `for_write` 委托现有自由函数，**零行为变化**。
- **16 个**文件类工具全部接入：`read_file` `read_dir` `glob_files` `list_tree` `read_binary_meta` `hash_file` `chmod_file` `delete_dir` `create_dir` `delete_files` `append_file` `search_replace` `create_file` `copy_file` `move_file` `modify_file`。
- 构造点收在 `runners.rs` 两个宏：`define_runners_paths_try!` / `define_runners_paths_ctx_try!`（后者带 `ctx`）。
- 展示类 helper（`*_display` / `*_body`）仍收 `&Path`，由调用方传 `access.root()`——它们不解析路径，换成载体只增遮蔽风险。
- 实测踩到的两个点：`mutate.rs` 已有 `paths: &[String]` 形参（访问器必须叫 `access`，否则静默遮蔽）；`read_file` 起初漏转（`read_tool.rs` 直调 `resolve_for_read_open`），第二轮补上。

**问题**（历史记录）：`resolve_for_read` / `resolve_for_write` 原有 **18+ 个调用点**（`file/mutate.rs` ×6、`file/write_ops.rs` ×6、`structured_data.rs` ×4、`tree_glob.rs` ×2、`file/directory.rs` / `file/inspect.rs` / `file/perm.rs` / `text_diff.rs` 各 1）。S3 加 `PathScope` 形参要改 20 处，S6 加 `AllowedRoots` 时**再改一遍**。

**改法**：

```rust
pub struct PathAccess<'a> {
    root: &'a Path,
    scope: PathScope,   // 初始只有 SessionRoot，行为等价
}

impl PathAccess<'_> {
    pub fn for_read(&self, sub: &str) -> Result<PathBuf, WorkspacePathError>;
    pub fn for_write(&self, sub: &str) -> Result<PathBuf, WorkspacePathError>;
}
```

**等价验收**：

- `tools/file/path.rs` 现有单测原样通过，含 `openat2` / `RESOLVE_IN_ROOT` 相关用例（`tests_typed_readonly.rs`）；
- 18 个调用点只换调用形态，**不引入任何新的路径判定分支**（diff 里不应出现 `if scope == …`）；
- 全量 `cargo test` 绿。

**收益**：S3 的 `Any`、S6 的 `AllowedRoots` 都只改 `path.rs` 一处；`openat2` 的 TOCTOU 语义集中在一处，不散到 18 个调用点（AGENTS.md → **Security-sensitive surfaces** 明确不得弱化）。

### 17.2 R2 外部工具来源收口（✅ 已完成）

**落地结果**（实测 10 个判定点，与下方原估算基本一致）：

- 载体：`cm_tools::tool_naming::{ToolOrigin, tool_origin}`；判定规则原样搬入，**未新增任何分支**。
- 已转换：`step_executor_policy.rs`×2、`registry_policy.rs`×3（`is_readonly_tool` 的两条早退合并为 `!= Builtin` 一条）、`web/tools_handlers.rs`×1、`execute_dispatch_body.inc.rs`×2、`tool_call_explain.rs`×2，以及 `tool_naming.rs` 自身 1 处。
- **故意没转** `dynamic_tools.rs:56`：它校验的是「插件 spec 的 `name` 必须以 `dyn__` 开头」，属**规格校验**而非来源分派；写成 `tool_origin != Dynamic` 只会更难读。
- 附带发现（已随 R2 后续清理）：转换后 `cm_mcp/stub.rs` / `cm_mcp/mcp_impl.rs` / `cm_internal/mcp/mod.rs` 对 `is_mcp_proxy_tool` 的 `pub use` 转发链已无消费者，**三处转发已删除**；`is_mcp_proxy_tool` 只保留在 `cm_tools/tool_naming.rs` 内部供 `tool_origin` 使用。
- 清理时的额外发现（**未处理**，属更大范围的死分支）：`cm_mcp` 只在 `feature = "server"` 下 `mod`（`src/lib_server.rs:23`），而 `server` 强制含 `mcp`（`Cargo.toml`）——所以 `#[cfg(not(feature = "mcp"))] mod stub` 的整个分支在当前 feature 图下**不可达**。删桩本体会牵动 `Cargo.toml` feature 图，留待单独评估。

**问题**（历史记录）：`is_mcp_proxy_tool` / `is_dynamic_tool_name` 原散在 **10+ 处**（`step_executor_policy.rs:65/98`、`registry_policy.rs:150/153/263`、`web/tools_handlers.rs:358`、`execute_dispatch_body.inc.rs:8/57`、`tool_call_explain.rs:28/46`、`dynamic_tools.rs:56`）。`external_tools = Denied` 若逐点判断，必漏。

**改法**：

```rust
pub enum ToolOrigin { Builtin, Mcp, Dynamic }
pub fn tool_origin(name: &str) -> ToolOrigin;
```

所有现有判定改成读 `tool_origin`，**判定规则本身不变**。

**等价验收**（均已完成）：

- ✅ `tool_origin` 的单测覆盖三类输入（`MCP_PROXY_PREFIX` / `DYNAMIC_TOOL_PREFIX` / 两者都不匹配），含「只在开头匹配」与空串边界；
- ✅ 原 10 个调用点的布尔结果**逐点不变**：`tool_origin_matches_legacy_predicates` 用同一批工具名对三条谓词做表驱动断言；
- **已知分歧只记录、不在 R2 修**：`tool_call_explain.rs:28` 把 MCP 与只读工具并列处理，与 L1「只读 = 独立正列表」冲突——R2 只统一**来源判定**，这条策略留到 S2 由 `caps.tool_visibility` 收口。

**收益**：S3 的 `external_tools` 变成一个谓词（`tool_origin(n) != Builtin && caps.external_tools == Denied`），MB-8 才可验证。

### 17.3 R3 `PolicyDecision` 借用能力快照

**问题**：`PolicyDecision` 里的 `allowed_commands: Arc<[String]>` 是从 `cfg` **复制**进来的，与未来的 `caps.command_scope` 重复。先实现权限再改，等于在同一文件里改两遍。

**改法**：`allowed_commands` / `skip_arg_safety` 改为从**借用的快照**读取，结构与 §7.4 的最终形态一致；快照暂时仍由 `cfg` 构造，语义不变。

**等价验收**：

- `execute_run_command_gate_tests.rs` 逐字通过——三道 gate 顺序、弹窗次数、错误文案全部不变（这正是 S3 的验收标准）；
- `PolicyDecision` 不再持有 `Arc<[String]>` 的副本（编译期可验证）。

### 17.4 明确不做

| 不做 | 理由 |
|---|---|
| 抽象通用 policy engine（6 轴变 plugin） | 过度设计；§7.3 已定「一处 `match level` + 执行点读小枚举」 |
| 动 `turn_loop` / `assess_turn_routing` | S2 只换入参来源，动它就是夹带行为变化 |
| 改 SSE 协议 | §11 已定复用 control-plane，不改 `SSE_PROTOCOL_VERSION` |
| 改名 `PathScope::*` 等变体 | Q9 是**文档**里的类型设计，代码里尚无这些类型 → 零改名成本，无需前置 |
| 拆 `finalize.rs` / `openapi_paths.rs` | 属文件尺寸话题，S1 与 S5a 落地时顺手拆即可 |

### 17.5 完成判据

**不要用「只改 N 个文件」这类估算**——R1 落地后的实测口径如下（R1 已完成，两批提交）。

S3 的必需改动点：

| 范围 | 具体位置 | 说明 |
|---|---|---|
| 路径载体 | `cm_tools/tools/file/path.rs` | `PathAccess` 加 `PathScope` 字段；`for_read` / `for_read_open` / `for_write` 内做 scope 判定（**唯一判定点**） |
| 文件类工具构造点 | `runners.rs` 的 `define_runners_paths_try!` 与 `define_runners_paths_ctx_try!` | 两个宏各一行 `PathAccess::new(ctx.working_dir)`；16 个文件类工具全部经此构造 |
| 命令执行 | `execute_run_command_approval.inc.rs` | `cwd` 越界判定 |
| HTTP | `http_tool_exec.inc.rs` | 出站前缀判定 |

不属 file 模块、但同样直调 `resolve_for_*` 的调用点（S3 需决定「换 `PathAccess`」还是「显式传 `None` scope + 注释说明」）：`text_diff.rs`×1、`structured_data.rs`×4、`table_text.rs`×1、`structured_data_patch/execute.rs`×1、`tools/mod.rs::resolve_workspace_path_for_read`×1、`cm_llm/outbound_workspace_images.rs`×1。

**可验证的停止条件**（比「改了几个文件」可靠）：

```bash
grep -rn "resolve_for_\(read\|read_open\|write\)(" src/cm_tools/tools/file/ | grep -v "path.rs"
```

预期只剩 `tests.rs` 里直接测自由函数的两行；若出现**非测试**文件命中，说明该工具尚未接入载体，先补 R1 再往下写。file 模块之外的上表调用点则逐个确认意图已写入 `PathScope`。

**R4**（allowlist 键构造收口，为 §6.3 的 `l{n}|` 前缀准备）与 **R5**（工具可见性三处判定合一）**不前置**：S3/S4 本来就要改那几个函数，届时顺手做。

---

## 18. 附录：现状锚点索引

便于实现时定位（行号为写作时快照，实现前请复核）：

| 概念 | 位置 |
|---|---|
| `SessionMode` 定义与 `requires_readonly_tools` | `src/cm_types/session_mode.rs` |
| `session_mode` 回合解析（含持久化） | `src/cm_internal/session_mode_turn.rs:11`、`:86` |
| Ask/Plan 挂只读约束 | `src/agent/agent_turn/turn_loop/run_dispatch.rs:34` |
| 单回合工具定义过滤 | `src/agent/agent_turn/turn_loop/outer_loop.rs:101` |
| 子代理角色工具表 | `src/cm_agent/step_executor_policy.rs:91` |
| early-deny | `src/agent/agent_turn/policy/tool_execution.rs:93` |
| 角色 `allowed_tools` 解析 | `src/cm_config/agent_roles.rs:111`、`src/cm_agent/turn_tool_policy.rs:44` |
| 只读/写工具分类 | `src/cm_tools/registry_policy.rs:105`、`:149` |
| `run_command` 策略评估 | `src/cm_internal/tool_registry/execute/execute_run_command_approval.inc.rs:147` |
| `PolicyDecision` 结构 | 同上 `:134` |
| 白名单 `*` 哨兵 | `src/cm_tools/tools/command_shell_script.rs:29` |
| 文件路径解析/写入 | `src/cm_tools/tools/file/path.rs:127`、`:159` |
| 工作区路径与敏感前缀 | `src/cm_tools/workspace/path.rs`、`src/cm_config/workspace_roots.rs:4` |
| `read_dir` 工作区外审批 | `src/cm_internal/tool_registry/execute/mod.rs:134` |
| http 前缀 + 审批 preflight | `src/cm_internal/tool_registry/execute/http_tool_exec.inc.rs:4`、`:56` |
| 审批能力域 | `src/cm_approval/mod.rs:26` |
| 审批载荷构造 | `src/cm_internal/tool_approval/approval_specs.rs` |
| 执行附件（`RunLoopAttach`） | `src/agent/agent_turn/host/params.rs:65` |
| Docker 沙盒挂载 | `src/cm_internal/tool_sandbox/mod.rs:82` |
| ceiling 推导（loopback 判断） | `src/cli_run.rs:534`（`validate_bind_auth`） |
| 会话级 allowlist（内存态） | `src/chat_job_queue/worker/stream_job_setup.rs:162` |
| workflow 节点能力上下文 | `src/cm_workflow/run.rs:228`、`src/cm_workflow/execute/node.rs:36` |
| MCP 一律放行（现状） | `src/cm_agent/step_executor_policy.rs:98` |
| MCP / 动态工具判定 | `src/cm_tools/tool_naming.rs` |
| 只读/写分类的可替换点 | `src/cm_tools/registry_policy.rs:156` |
| 热重载逐字段白名单 | `src/cm_config/hot_reload.rs:20` |
| 会话库加列（幂等） | `src/conversation_store.rs:92`（`ensure_text_column`） |
| 默认配置 | `config/default_config.toml:98`、`config/tools.toml:203` |
