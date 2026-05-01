# 工具调用层：对标开源 Agent 的演进方向

**状态**：路线图 / 设计备忘（**未**承诺实现顺序与时间表）。**受众**：维护 **`src/tools/`**、**`tool_registry`**、**`tool_approval`**、**`agent_turn::execute_tools`** 与 **SSE 工具事件** 的开发者。  
**语言**：中文。  
**关联**：

- 内置工具契约与信封：**`docs/工具说明.md`**
- 模块索引与分发：**`docs/开发文档.md`**（`tool_registry`、`tools/`）
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
| **长任务进度事件** | 构建/索引等分阶段 SSE | 在 **`sse/`** 与 **`execute_tools`** 增加 **可订阅进度**（注意体积与脱敏）。 |

---

## 3. 安全与沙箱（E2B、Sandbox Fusion、nsjail 类）

| 方向 | 说明 | 与仓库关系 |
|------|------|------------|
| **命令默认沙箱化** | 危险命令自动走 Docker / 隔离 | 已有 **`sync_default_tool_sandbox_*`**；可演进：**策略表驱动「工具 → 沙箱档位」**。 |
| **策略即配置** | 路径前缀、HTTP 方法、命令类风险分级 | 与 **`write_effect_tools`**、**`http_fetch_allowed_prefixes`**、**`SensitiveCapability`** 演进同向。 |
| **每工具预算** | wall clock + token + 输出上限组合 | 与 **`parallel_wall_timeout_secs`**、`tool_message_max_chars` 等统一叙事。 |

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

## 9. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-05-01 | 初稿：对标开源 Agent 的工具调用演进维度、与现有模块映射、优先级建议。 |
