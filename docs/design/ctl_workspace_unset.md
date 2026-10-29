# 声明可省略 workspace：由同机 client 运行时设置

> **状态**：设计稿（跟踪 Issue #1183，**未实现**）。三条核心语义于 2026-10-29 拍板，实现不得回退。
> 相关：[命令行与路由](../命令行与路由.md)（`crabmatectl` 子命令与 socket 协议）、[permission_levels.md](permission_levels.md)（工具面门控）、[server_api_completeness.md](server_api_completeness.md)。

## 1. 目标

把「实例 ↔ 工作区」的绑定从**静态声明**改为**运行时状态**：

- `instances.toml` 只描述「这个实例叫什么、用什么 profile / 端口 / env」
- 工作区由 client 连上后通过 `POST /workspace` 设置
- `crabmatectl` 不再要求 `Declaration.workspace` 必填

**不改 HTTP 契约**：设置端点、三态语义、白名单校验、读回接口均已存在（§4）。

### 被否决的替代方案

| 方案 | 否决理由 |
|---|---|
| 只给 `workspace` 加 `#[serde(default)]` | 空串会先通过 TOML、再被 `validate` 拒（`bad_instances_entry`），症状相同但报错更晚、更难定位 |
| ctl 记住 client 设过的值并回填 | 见 §2 ②：把运行时状态伪装成声明状态，作者语义含混 |
| 不做「未设置即拒绝」 | 见 §2 ③：`serve` 会在 `run_command_working_dir` 上跑命令，用户以为没选目录、实际已经在一个目录里动手 |

## 2. 已定语义

### ① 一个实例只对应一个工作区 —— 服务端拦截

`workspace_override: Arc<RwLock<Option<String>>>` 是**进程级单值**。两个 client 各设一个目录，后到者覆盖前者，前者的下一次工具调用落在错误目录。因此不做「文档提醒」，而做服务端拒绝：

- `POST /workspace` 在「已设置且新值不同」时返回 **409 `WORKSPACE_ALREADY_SET`**，不覆盖
- 同值重复设置 → 幂等成功（客户端重连/重试不应报错）
- `None`（待绑定）与 `Some("")`（显式选默认目录）继续区分
- 换工作区 = **换实例**；原地换必须先 `POST /workspace {"path": ""}` 显式复位

### ② 接受无人值守退化 —— 不回填

`--restart on-failure` 或 ctl 重启后，实例回到待绑定态，等 client 重新设置。

- 不做「ctl 记录并回填」：那等于 client 发一次、ctl 永久记住，绕回「声明带工作区」，只是作者从人换成了 client
- 代价明示：崩溃恢复**不自愈**（实例能起来，但不能干活，直到 client 设置工作区）
- 将来要自愈，正解是 client 经 socket `add` 把工作区写进声明，而不是让 ctl 记住运行时值

### ③ 未设置时工具必须拒绝

现状是**静默回落**：`chat/stream.rs:238-243` 在 `eff_ws.is_empty()` 时用 `cfg.command_exec.run_command_working_dir`，而 `effective_workspace_path_from_override`（`app_state.rs:118`）对 `None` 返回空串——两者叠加即「从未设置」被当作「用默认目录」。改为：

- `workspace_is_set() == false`（override 为 `None`）⇒ **不 fallback**，工具面返回 **`WORKSPACE_NOT_SET`**
- `Some("")`（显式选默认）仍走 `run_command_working_dir`，不得误伤
- 所有 fallback 点逐个核对（§5）

### ④ 待绑定语义只对 ctl 起的实例生效（范围限定）

**`serve` 单独启动保持今天的行为**：未传 `--workspace` 等价于 `Some("")`，用 `run_command_working_dir`。

理由：CLI / headless 直启 `serve` 的用户没有 client 来设置工作区；若也要求设置，纯 CLI 用法会被打断，属本设计范围之外的破坏性变更。

**实现约束（重要）**：`--workspace` 本就是 `Option<String>`，「未传」这一个信号无法同时表达两种意图。因此 ctl spawn 时必须额外传一个**显式标志**（建议 `--workspace-unset`，或等价的受控环境变量），`serve` 据此把 override 初始化为 `None`；没有该标志时仍按旧逻辑取 `run_command_working_dir`。省略参数本身**不足以**表达「待绑定」。

该标志属对外 CLI 面变更，实现时须同步 `README.md` 与[命令行与路由](../命令行与路由.md)。

## 3. 状态机

### 3.1 工作区三态（服务端）

```
None ──POST /workspace {path:"/p"}──► Some("/p")        待绑定 → 已绑定
None ──POST /workspace {path:""}────► Some("")          显式选默认（合法终态）
Some(x) ──POST /workspace {path:x}──► Some(x)           幂等
Some(x) ──POST /workspace {path:y}──► 409 WORKSPACE_ALREADY_SET（原绑定不变）
Some(x) ──POST /workspace {path:""}─► Some("")          显式复位
```

工具面判定：

| override | `workspace_is_set()` | 工具执行 |
|---|---|---|
| `None` | false | **拒绝**，`WORKSPACE_NOT_SET` |
| `Some("")` | true | 用 `run_command_working_dir` |
| `Some(p)` | true | 用 `p`（已过白名单校验） |

### 3.2 实例生命周期（ctl）

```
声明（无 workspace）─► up / spawn（不传 --workspace，带 --workspace-unset）
                    ─► ready（工作区待绑定）
                          │
        client POST /workspace ─┘─► 已绑定（进程内状态，不写回声明）
                          │
进程崩溃 ─► restart on-failure ─► 重新 ready ─► 待绑定（不回填，等 client）
```

`InstanceRecord` 只增一个字段表达「启动时是否带工作区」，用于 `ls` / `status` / `--dry-run` 显示，**不作为恢复来源**。

## 4. 现状核对（基线 `main` @ `0b9081f2`）

| 能力 | 证据 |
|---|---|
| `serve --workspace` 可省略 | `src/cm_config/cli/definitions.rs:12-14`：`workspace: Option<String>`，省略则用配置的 `run_command_working_dir` |
| 设置端点已存在 | `src/web/routes/workspace/mod.rs:26-27` → `workspace_set_handler`（`POST /workspace`），支持 `path` 与 `project` 字段 |
| 读回端点已存在 | 同一路由 `get(workspace_handler)` |
| 白名单校验已存在 | `validate_workspace_set_path`（`src/web/workspace/handlers.rs:119`）：须存在、是目录、落在 `workspace_allowed_roots` 内、不命中敏感系统目录 |
| 三态语义已实现 | `set_workspace_from_raw_path`（`handlers.rs:172-187`） |
| 已设置判定已存在 | `workspace_is_set_from_override`（`app_state_facets.rs:396`）；`chat/stream.rs:237` 已在读 `state.workspace_is_set()` |
| **静默回落（要改）** | `chat/stream.rs:238-243`：`eff_ws.is_empty()` ⇒ `run_command_working_dir`；`app_state.rs:118`：`None` ⇒ 空串 |

## 5. 改动清单

### ctl 侧

1. `src/cm_ctl/instances.rs`：`Declaration.workspace: Option<String>`；`validate` 去掉「缺少 workspace」（`:206-211`）
2. `src/cm_ctl/ops.rs`：`prepare_up` 不再要求工作区（现在报 `bad_workspace`）
3. `src/cm_ctl/spawn.rs`：`build_args` 无工作区时不加 `--workspace`，改加 `--workspace-unset`（`:146-147`）
4. `src/cm_ctl/record.rs`：新增字段（只增）表达工作区来源
5. `control.rs::list`、`ops::status`、`supervise --dry-run`：待绑定显示为 `workspace: null`，不得显示成默认目录

### web / CLI 侧

6. `serve` CLI：新增 `--workspace-unset`（与 `--workspace` 互斥），初始 override 置 `None`
7. `workspace_set_handler` / `set_workspace_from_raw_path`：加冲突拦截，新增 `WORKSPACE_ALREADY_SET`
8. 未设置即拒绝，逐点核对：
   - `src/web/chat_handlers/chat/stream.rs:238-243`
   - `src/web/chat_handlers/chat/enqueue.rs:96`
   - `src/cm_internal/process_handles.rs:136`（先区分「进程 cwd」与「agent 工作区」，前者未必该拒）
   - `src/cli_run_serve.rs:142`、`:221`
   - `src/runtime/benchmark/runner.rs:57`
   - `src/runtime/cli_*` 的 `cli_effective_work_dir` 各处
9. 错误码 `WORKSPACE_NOT_SET` 落在共享常量层；若出现在 SSE / HTTP 错误体中，同步 [SSE 协议](../SSE协议.md)

## 6. 与既有模式的交互

| 场景 | 行为 |
|---|---|
| 纯对话（不触工具） | 允许；待绑定不影响对话 |
| 触发任一工具 | `WORKSPACE_NOT_SET`，提示 `POST /workspace` |
| `no_tools` | 无工具可触发，无需拒绝 |
| `ask` / `plan`（只读档） | 只读工具同属工具面 ⇒ 同样拒绝 |
| 工具调用后 client 才设置工作区 | 下一轮读到新值即可用，无需重开会话（override 是进程级） |

## 7. 兼容与迁移

- **旧声明（带 `workspace`）行为逐字不变**：仍传 `--workspace`，与今天一致
- **不写 `workspace` 是新增能力**，不是新默认
- `POST /workspace` 的 409 属**行为收紧**（原先覆盖）：CHANGELOG 记入 `Changed`，并提示 client 适配——收到 409 应提示「该实例已绑定其他工作区」而非静默重试

## 8. 验收

- 声明只有 `profile` / `port` 时 `supervise --dry-run` 接受，计划显示工作区待绑定
- 实例能起到 `ready`，`GET /workspace` 报未设置态
- client 设置工作区后工具作用域随之切换；越界路径 → 403/400（既有行为）
- 第二次设**不同**工作区 → 409 `WORKSPACE_ALREADY_SET`，原绑定不变
- 同值重复设 → 幂等成功
- **待绑定时任一工具返回 `WORKSPACE_NOT_SET`**，且 `Some("")` 不受影响（两条对照测试）
- `--restart on-failure` 重启后仍待绑定（断言不回填）
- **`serve` 直启（不带 `--workspace-unset`）行为不变**：不设置也能用 `run_command_working_dir`（对照测试，防 §2 ④ 被误伤）
- `cargo test` 全量 + `pre-commit run --all-files` 绿；文档同步 `README.md`、[命令行与路由](../命令行与路由.md)、`CHANGELOG.md`

## 9. 未决（实现前仍需定）

1. `cm_internal/process_handles.rs:136` 管的是子进程 cwd，与 agent 工作区可能不是同一概念；需先区分再决定是否拒绝
2. socket 控制面的 `add` 是否允许省略 `workspace`：建议**仍必填**（client 知道目录才 `add`），使「声明两种模式并存」且不影响既有调用方
3. 错误码命名与既有 `*_REQUIRED` 风格对齐（`WORKSPACE_NOT_SET` 还是 `WORKSPACE_REQUIRED`），需与 [SSE 协议](../SSE协议.md) 现有错误码一致
