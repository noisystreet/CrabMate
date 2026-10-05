# 工具调用层：对标开源 Agent 的演进方向

**状态**：路线图 / 设计备忘（**未**承诺实现顺序与时间表）。**受众**：维护 **`src/cm_tools/`**、**`tool_registry`**、**`tool_approval`**、**`agent_turn::execute_tools`** 与 **SSE 工具事件** 的开发者。  
**语言**：中文。  
**关联**：

- 内置工具契约与信封：**`docs/工具说明.md`**
- 工具与分发：**`docs/工具说明.md`**、`tool_registry` / `tools/` 源码；架构入口见 **`docs/开发文档.md`**
- 待办跟踪：**`docs/待办清单.md`** → **`tools/` 与 `tool_registry.rs`** 小节（与本文件交叉维护：落地后删待办、本文件可增修订记录）
- 安全面：**`.cursor/rules/security-sensitive-surface.mdc`**、**`docs/配置说明.md`**（`allowed_commands`、`http_fetch_*`、沙盒等）

---

## 1. 当前结构化程度（基线）

| 层次 | 现状摘要 |
|------|-----------|
| **契约** | `ToolSpec` + JSON Schema（`tool_specs_registry`）；`workflow_tool_args_satisfy_required` 等与内置表对齐。 |
| **分发** | `HandlerId` + `tool_dispatch_registry!`；`dispatch_tool` / `run_tool`；`workflow_execute` 走 `agent::workflow_tool_dispatch`。 |
| **上下文** | `ToolContext` 聚合配置、工作目录、白名单、超时、变更集等。 |
| **策略** | `ToolCategory`、`dev_tag` 裁剪；`step_executor_policy` 与分阶段 / DAG `node_tool_role`；`is_readonly_tool` 与并行批。 |
| **薄弱带** | **`run_command`** 仍以字符串命令为主；**MCP** 动态工具语义弱于内置；各 `runner_*` 结构化程度依赖单工具实现。 |

以下方向按**开源 Agent / 编排产品**常见能力整理，**不**要求逐项实现；优先与上表薄弱带及 **`docs/待办清单.md`** 已有条目（MCP、审批、沙盒等）合并推进。

---

## 2. 调用形态与编排（LangGraph / AutoGen / OpenAI Agents 等）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **同轮并行与依赖** | 多 `tool_calls` 的并行、失败策略、依赖图 | 已有 **`workflow_execute` DAG**；可补：**非 DAG 的批策略在文档与提示词中显式化**（与 `parallel_readonly` 策略一致）。 |
| **工具结果部分接受** | 用户只批准部分 `tool_calls` 或跳过失败条 | 可增强 **Web 粒度审批** 与 **重试单条**（与 `tool_approval` 同向）。 |
| **强制结构化输出** | 某工具或某步要求 JSON schema 结果 | 与 **`agent_reply_plan`**、**`structured_payload`** 同谱系；可扩展：**特定工具注册「结果 JSON profile」** 供下游节点消费。 |
| **长任务进度事件** | 构建/索引等分阶段 SSE | 在 **`sse/`** 与 **`execute_tools`** 增加 **可订阅进度**（注意体积与脱敏）。落地切片（共享 Child 会话、杀进程组、chunk、按工具名墙钟）见 **`docs/design/long_running_tool_execution_todo.md`**。 |

---

## 3. 安全与沙箱（E2B、Sandbox Fusion、nsjail 类）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **命令默认沙箱化** | 危险命令自动走 Docker / 隔离 | 已有 **`sync_default_tool_sandbox_*`**；可演进：**策略表驱动「工具 → 沙箱档位」**。 |
| **策略即配置** | 路径前缀、HTTP 方法、命令类风险分级 | 与 **`write_effect_tools`**、**`http_fetch_allowed_prefixes`**、**`SensitiveCapability`** 演进同向。 |
| **每工具预算** | wall clock + token + 输出上限组合 | 与 **`parallel_wall_timeout_secs`**、按**工具名**的墙钟覆盖、`timeout_secs`、`tool_message_max_chars` 统一叙事（**不要**指望 `ToolExecutionClass` 区分同一 `run_command` 下的短命令与全量测试）。 |

---

## 4. 可扩展与生态（MCP、LangChain Toolkits）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **MCP 能力矩阵** | 按 server 标注风险、默认关闭写类 | **`docs/待办清单.md`** 已有 MCP 扩展项；本文件强调 **「声明能力 + 默认安全」** 产品叙事。 |
| **官方 Recipe / 模板** | 一键插入 CI / 审查工作流 | 与 **`workflow_template`**、`workflow_validate_only` → 规划绑定 **同向**；减少模型手写 DAG。 |
| **命令 → 工具映射** | 如窄匹配 `ls` → 列目录工具 | **可选**：在 `run_command` 入口做 **无 shell 元字符** 的别名展开；需 **文档开关** 防与真 shell 预期不一致（参见对话结论：窄匹配 + 配置关闭）。 |

---

## 5. 观测、调试、评测（LangSmith / OTel / SWE-agent）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **全链路 ID** | `request_id` / `job_id` / `tool_call_id` / `workflow_run_id` | 与 **`TracingChatTurn`**、**`crabmate_tool` 信封** 字段对齐；横切 **`docs/待办清单.md`** P5「日志关联」。 |
| **工具级指标** | 按工具名聚合错误码、耗时 | **不写敏感参数**；供 `/health` 或内部 metrics。 |
| **Replay 回归** | 失败用例生成 fixture | 已有 **`tool-replay`** 方向；可增强 **一键从会话导出 replay**。 |

---

## 6. 人机协同（Cursor / Copilot 类）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **写前 diff 预览** | Apply / Reject | 与 **`session_workspace_changelist`**、Web 侧展示 **深度绑定**。 |
| **参数可编辑再执行** | 尤其 HTTP、大块 patch | Web **`tool_call` 卡片** 扩展草稿态。 |
| **失败建议下一步** | 基于 `error_code` 的规则提示 | 减少盲目 **`run_command`** 重试。 |

---

## 7. 多 Agent / 角色（CrewAI、MetaGPT）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **角色绑定工具包** | 不仅分阶段步，还可会话级角色预设 | 与 **`agent_role`**、**`turn_allow`**、**`executor_kind`** 组合设计，**避免三套并行语义**（见 **`docs/工作流编排架构.md`** 分层用语）。 |

---

## 8. 实施优先级建议（非约束）

1. **观测链 + 工具错误聚合**（成本低、排障收益高）。  
2. **写类 Web diff 与粒度审批**（安全感知最强）。  
3. **Recipe / 模板与 validate_only 叙事产品化**（与现有 workflow 能力契合）。  
4. **MCP 能力矩阵与默认策略**（与待办 MCP 条合并）。  
5. **`run_command` 窄映射 / 收缩**（在提示词与可选运行时映射之间取舍）。

---

## 8.5 工具收敛：薄 CLI 封装 vs `run_command`（2026-10 分析）

**背景**：近一半内置工具只是把某个 CLI 包成独立 `ToolSpec` + `runner_*`。这些「薄封装」维护成本高于结构化收益时，可考虑收敛为 `run_command`。本节记录收敛的边界、约束与候选路线，供后续切片。

### 8.5.1 已落地

- **移除 6 个源码分析专用工具**（`shellcheck_check` / `cppcheck_analyze` / `semgrep_scan` / `hadolint_check` / `bandit_scan` / `lizard_complexity`）：统一改用 **`run_command`**（6 个 CLI 已进 `config/tools.toml` 白名单）；`/health` 不再探测 `dep_*`；缺失安装提示仍由 `error_output_playbook` 提供。详见 CHANGELOG `[Unreleased]` 与 **`docs/工具说明.md`**。
- **移除 14 个 JVM/容器 + Node/前端薄封装（A 档）**：`maven_compile` / `maven_test` / `gradle_compile` / `gradle_test` / `docker_build` / `docker_compose_ps` / `podman_images` / `npm_install` / `npm_run` / `npx_run` / `tsc_check` / `frontend_lint` / `frontend_build` / `frontend_test`，统一改用 **`run_command`**（相关 CLI 已在 `config/tools.toml` 白名单）。**实现函数与参数类型按需保留**：`quality_workspace` / `ci_pipeline_local` / `lint.rs` 仍**直接调用** `jvm_tools` / `container_tools` / `frontend_tools` 的实现，仅删除对外 `ToolSpec` + runner 注册，不影响这些组合工具的开关语义（如 `run_maven_*` / `run_gradle_*` / `run_frontend_build`）。
- **移除 17 个写侧 Git 工具（B 档）**：`git_stage_files` / `git_commit` / `git_checkout` / `git_branch_create` / `git_branch_delete` / `git_push` / `git_merge` / `git_rebase` / `git_stash` / `git_tag` / `git_reset` / `git_cherry_pick` / `git_revert` / `git_clone` / `git_remote_set_url` / `git_apply` / `git_fetch`，统一改用 **`run_command`** 调用 `git`（`git` 已在 `config/tools.toml` 白名单），能力零损失。**只读 Git 工具保留并做同构合并**：`git_diff` 吸收原 `git_diff_stat` / `git_diff_names` / `git_diff_base`（新增可选 `stat` / `name_only` / `base`），保留 `git_status` / `git_log` / `git_show` / `git_blame` / `git_file_history` / `git_branch_list` / `git_remote_status` / `git_remote_list` / `git_clean_check`。**为何不一并降级只读**：`run_command` 非只读，降级会连带失去 8.5.2 的四项只读能力（并行只读批 / 只读重试 / Plan·Ask 门控 / TTL 缓存）——这与 A 档「低频低价值只读」的取舍不同（Git 只读查询在代码审查工作流中使用频率高）。

### 8.5.2 核心约束（收敛前必须权衡）

`run_command` 在「按工具名」的只读判定中被**明确排除**——见 `tool_retry_policy` / `is_readonly_tool` 注释（同排除的还有 `terminal_session` / `http_request` / 写类 / MCP / 动态 / workflow）。因此任何一个**只读**专用工具改写为 `run_command`，都会**连带失去**：

- `parallel_readonly_batch` 的并行只读批；
- `tool_retry_policy` 的只读失败重试；
- `SessionMode::Ask` / `Plan` 的 `requires_readonly_tools()` 门控；
- 只读结果缓存 / 去重（`readonly_tool_ttl_cache_secs`）。

> **决策（2026-10）**：对**低频、低价值**的只读薄封装，**接受**上述能力损失，一并收敛进 `run_command`。

### 8.5.3 不可收敛的边界

- **白名单外 CLI**：如 `rust-analyzer` / `pytest` / `typos` / `codespell` / `ast-grep` 等——不在 `allowed_commands` 中，删掉专用工具等于**功能净丢失**，除非先扩白名单（见路线 C，风险最高）。**注（2026-10）**：`rustfmt` / `clang-format` / `ruff` / `mypy` / `uv` / `go` 系（`go` / `gofmt` / `golangci-lint`）等已随白名单扩充（见 §8.5.5）进入 `allowed_commands`，故不再属于本边界；但对应专用工具（`format_*` / `ruff_check` / `mypy_check` / `uv_*` / `go_*`）**仍保留**——它们是**只读**工具，降级 `run_command` 会失去 §8.5.2 的四项只读能力，与 A 档「低频低价值只读」的取舍不同。
- **纯 Rust 工具**：结构化输出 / 内部状态（记忆、schedule、后台任务、workflow、skill、self-config 等），无 CLI 对应，**保留**。
- **Agent / 编排类**：保留。
- **`cargo_test`** 等已接入**后台任务装配**（`background_job_async_tools` 默认 `["run_command"]`，`cargo_test` / `pytest_run` 须显式加入才走后台），收敛前须确认不破坏该链路。

### 8.5.4 候选路线（按风险 / 收益排序，非承诺顺序）

| 档位 | 做法 | 预期工具名变化 | 风险 |
|------|------|----------------|------|
| **A｜延续删除薄封装**（**已落地**） | 删除 JVM 容器 / npm 前端等低频封装（`maven_compile` / `maven_test` / `gradle_compile` / `gradle_test` / `docker_build` / `docker_compose_ps` / `podman_images` / `npm_install` / `npm_run` / `npx_run` / `tsc_check` / `frontend_lint` / `frontend_build` / `frontend_test`，共 14 个），改用 `run_command` | **-14** | 低（白名单已含相关 CLI；接受 8.5.2 能力损失） |
| **B｜合并式精简**（**写侧 + diff 合并已落地**） | 写侧按「全量降级 `run_command`」处理（17 个写 Git 工具）；只读侧按家族聚合并参数化（`git_diff` 吸收 `git_diff_stat` / `git_diff_names` / `git_diff_base`）。其余家族（`gh_*` / `cargo_*` 等）仍为候选 | 本轮 **-19**（-17 写 + diff 三合一） | 中（须重写 schema / 分发 / 提示词，并回归后台任务与只读语义；`rust_analyzer_*` 8 个不可替代需保留） |
| **C｜扩白名单再收敛** | 把 8.5.3 白名单外 CLI 也纳入再删封装 | 视范围 | **高**（扩大任意命令执行面，不建议；如做须走 `tool_approval` 审批） |

**推荐**：**A 已落地**（低风险、已通过 `cargo clippy --all-targets --all-features -- -D warnings` / `cargo test`）；**B 的写侧 + diff 合并已落地**（17 写工具降级 `run_command` + `git_diff` 三合一，能力零损失），其余家族合并（`gh_*` / `cargo_*` 等）待后续切片；**C** 暂不推进。

### 8.5.5 白名单扩充（2026-10，局部落地路线 C）

**背景**：此前存在「能力割裂」——多个内置工具（`go_*` / `ruff_check` / `mypy_check` / `uv_*` / `format_*` / `package_query` / `port_check` 等）内部直接 spawn 的 CLI **不在** `allowed_commands` 中，导致「走专用工具能跑、走 `run_command` 被拒」；且文档已声称 `npx` / `tsc` 在白名单，实际缺失。

**做法**：向 `config/tools.toml` 的 `allowed_commands` 新增 **21 项**（102 → 123）：

- **语言工具链（14）**：`npx` / `tsc` / `go` / `gofmt` / `golangci-lint` / `ruff` / `mypy` / `uv` / `rustfmt` / `clang-format` / `shfmt` / `xmllint` / `sqlfluff` / `pg_format`
- **归档（2）**：`7z` / `unrar`
- **系统与包查询（5）**：`dpkg-query` / `rpm` / `ss` / `lsof` / `bc`

**定位**：这是路线 **C（扩白名单）的局部落地**，但**仅扩白名单、不删对应专用工具**——与 C 档「扩白名单**再收敛**」不同。对应专用工具（`format_*` / `ruff_check` / `mypy_check` / `uv_*` / `go_*` / `package_query` / `port_check` / `process_list`）**保留**，因其为**只读**工具，降级会失去 §8.5.2 的四项只读能力。

**安全面**：白名单仍是硬闸门，仅扩充允许的命令集合；新增 CLI 均为常规开发工具，未引入任意命令执行面。

**后续**：可评估「工具 spawn 的 CLI ⊆ 白名单」的一致性测试，从机制上防止再次漂移。

---

## 9. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-05-01 | 初稿：对标开源 Agent 的工具调用演进维度、与现有模块映射、优先级建议。 |
| 2026-10-05 | 补 **§8.5 工具收敛分析**：已移除 6 个源码分析工具（改 `run_command`）；记录只读语义约束（`run_command` 非只读 → 失去并行只读批 / 重试 / Plan·Ask 门控）、不可收敛边界与 A/B/C 三档候选路线；决策接受低频只读封装的能力损失。 |
| 2026-10-05 | **落地 §8.5.4 A 档**：移除 14 个 JVM/容器 + Node/前端薄封装，改用 `run_command`；保留 `jvm_tools` / `container_tools` / `frontend_tools` 实现以支撑 `quality_workspace` / `ci_pipeline_local` / `lint.rs`。 |
| 2026-10-05 | **落地 §8.5.4 B 档（写侧 + diff 合并）**：移除 17 个写 Git 工具（降级 `run_command`，能力零损失）；只读 Git 工具保留，`git_diff` 吸收 `git_diff_stat` / `git_diff_names` / `git_diff_base`（新增 `stat` / `name_only` / `base`）。保留只读侧是为维持「按工具名」的只读语义（并行只读批 / 只读重试 / Plan·Ask 门控 / TTL 缓存）。 |
| 2026-10-05 | **白名单扩充（§8.5.5）**：向 `allowed_commands` 新增 21 个常用开发 CLI（语言工具链 / 归档 / 系统与包查询），修复「专用工具内部 spawn 的 CLI 不在白名单」的能力割裂与文档不一致；仅扩白名单、不删对应只读专用工具。 |
