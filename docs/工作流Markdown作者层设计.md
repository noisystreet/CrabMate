# 工作流 Markdown 作者层：两种路径对比与演进设计

**状态**：设计稿（**未**承诺实现时间表）。**受众**：维护者、产品与协议设计者。  
**语言**：中文（暂无独立英文全译本）。  
**关联**：

- 轮内 DAG 能力边界、FSM/分支/循环原则 → **`docs/工作流编排架构.md`**
- 运行时 JSON 契约、`workflow_execute` / `validate_only` → **`docs/工具说明.md`**
- 源码：`src/cm_workflow/`（**`parse_workflow_spec`**、模板、调度）

---

## 1. 背景与动机

当前 CrabMate 已支持：

| 能力 | 入口 | 说明 |
|------|------|------|
| 手写 DAG | `workflow_execute` 的 `workflow.nodes` + `deps` | 解析见 **`parse.rs`**，规格见 **`model.rs`** |
| 内置模板 | `workflow.workflow_template` | `rust_ci_light`、`code_review`、`refactor_precheck` 等 |
| 仅校验 | `workflow.validate_only: true` | 返回拓扑层、节点表，不执行工具 |
| 占位符 | `{{node_id.output}}` 等 | 节点间注入（见工具说明） |

用户希望用**接近自然语言**的方式描述流程（含分支、循环叙事），同时仍走现有 **DAG 执行器**（审批、补偿、trace、`workflow_node_id` 对齐）。

本文定义 **「作者层（Author Layer）」**：把 Markdown（或对话中的 MD 片段）变成 **`WorkflowSpec` 可消费的中间表示**，再交给既有 **`workflow_execute`**。作者层 **不** 替代调度器，也 **不** 在单轮 DAG 内引入无界环（与 **`工作流编排架构.md` §4.3** 一致）。

---

## 2. 分层模型（统一用语）

```mermaid
flowchart TB
  subgraph author [作者层 — 本文范围]
    MD[Markdown / 对话文本]
    A[路径 A: 确定性提取 + 编译]
    B[路径 B: LLM 结构化]
    MD --> A
    MD --> B
    A --> SPEC[workflow_spec v2 或 nodes JSON]
    B --> SPEC
  end
  subgraph runtime [运行时 — 已实现]
    VAL[validate_only / schema 校验]
    PARSE[parse_workflow_spec]
    EXEC[workflow_execute 调度]
    SPEC --> VAL
    VAL --> PARSE
    PARSE --> EXEC
  end
  subgraph session [会话级 — 可选外环]
    TURN[多轮 agent_turn / plan_rewrite]
    EXEC --> TURN
    TURN --> MD
  end
```

| 层 | 职责 | 确定性 |
|----|------|--------|
| **作者层** | 可读、可版本管理、可 review | 路径 A 高；路径 B 依赖模型 |
| **编译层**（建议新增） | `workflow_spec` → `workflow.nodes`；`when` / `for_each` 展开 | 必须 100% 可复现 |
| **运行时** | 拓扑、并行、审批、补偿、trace | 已实现 |

**原则**：无论路径 A 还是 B，**执行前**必须经过 **`parse_workflow_spec` + `workflow_tool_args_satisfy_required`**（及未来的 spec schema）；**禁止**「模型直接输出可执行 DAG 且跳过校验」作为唯一路径。

---

## 3. 路径 A：Markdown + 确定性代码块（编译器）

### 3.1 形态

工作区或仓库内文件，例如 `examples/workflows/ci_from_markdown.md`（或工作区 `.crabmate/workflows/ci.md`）：

```markdown
# Rust 轻量 CI

提交前在仓库根跑 fmt → check → clippy → test。

```crabmate-workflow
version: 2
workflow:
  fail_fast: true
  steps:
    - id: fmt
      label: 格式化检查
      tool: cargo_fmt
      args: { check: true }
    - id: clippy
      label: Clippy
      tool: cargo_clippy
      after: [fmt]
      args: { all_targets: true }
```
```

**围栏标识**：`crabmate-workflow`（info string 精确匹配；大小写敏感，与 CommonMark 惯例一致）。

**块内语法**：首版推荐 **YAML**（人类友好）；可选接受 **JSON**（`version` 字段区分）。块内根对象对齐 **`workflow_execute` 的 `workflow` 键** 或独立的 **`workflow_spec`** 包装（见 §6）。

### 3.2 处理流水线

1. **`extract_md_workflow_blocks(path | text)`**  
   - 扫描 fenced code block；仅处理 `crabmate-workflow`。  
   - 同一文件多块时：默认 **按出现顺序** 视为独立工作流；或要求 front matter 指定 `workflow_id`（Phase 2）。

2. **`parse_workflow_spec_yaml(bytes)`** → `serde_json::Value`（与现有 JSON 入口汇合）。

3. **`compile_workflow_spec(spec)`**（新模块，建议 `src/cm_workflow/compile_spec.rs`）  
   - 将 `steps[]` 的 `after` / `when` / `for_each` 编译为 **`nodes[]` + `deps`**。  
   - 编译期强制 **`max_items` / `max_iterations`** 上限。  
   - 输出与手写 DAG **同形**，供 **`parse_workflow_spec`** 消费。

4. **`workflow.validate_only`**（可选）→ 展示 `execution_layers` 给用户或 CI。

5. **`workflow_execute`**（实际执行）。

### 3.3 优点

| 点 | 说明 |
|----|------|
| **可复现** | 同文件、同版本编译器 → 同一 DAG；适合 CI、`crabmate workflow validate` |
| **可 diff** | Git review 聚焦代码块，而非整段 prose |
| **安全** | 无任意代码执行；仅声明工具名与 args |
| **离线** | 不消耗 LLM；适合 air-gapped |
| **与现网一致** | 编译产物即今日 `workflow.nodes` 语义 |

### 3.4 缺点与缓解

| 缺点 | 缓解 |
|------|------|
| 作者仍需学习块内 YAML | 提供 snippet、LSP/JSON Schema 校验、`doctor` 子命令 |
| 纯 prose 段落不参与执行 | 正文仅文档；或 Phase 2 用 LLM **仅生成块**（见 §5） |
| 动态分支需 `when` 或 choice 节点 | 与 **`工作流编排架构.md` Phase 2** 对齐；MVP 可静态展开 |

### 3.5 建议入口（实现时）

| 入口 | 行为 |
|------|------|
| CLI | `crabmate workflow validate path.md` / `run path.md --dry-run` |
| 工具 | `workflow_from_file`：`{ "path": "...", "validate_only": true }` |
| Web | 工作区浏览器打开 `.md` → 预览编译后的层图（只读） |

三端共用 **`compile_spec` + `parse_workflow_spec`**（符合 **CLI/TUI/Web 共享逻辑** 规则）。

---

## 4. 路径 B：Markdown + 大模型解析

### 4.1 形态

**纯自然语言** MD（可无代码块），或 **正文 + 由模型补全的代码块**：

```markdown
# 发布前检查

1. 先看 git diff（含 staged）
2. 跑 cargo clippy（all targets）
3. 若 clippy 失败，再 cargo test
4. 对改动列表里最多 10 个 .rs 文件各跑 rust_file_outline
```

模型输出（强制 JSON 围栏或 tool result）：

```json
{
  "workflow_spec": {
    "version": 2,
    "workflow": { "fail_fast": false },
    "steps": [ "..."]
  }
}
```

或（仅简单串行、无 `when` 时）直接：

```json
{
  "workflow": {
    "nodes": [ "..."]
  }
}
```

**推荐**：优先让模型产出 **`workflow_spec` v2**，由 **同一编译器** 生成 `nodes`，避免模型手写 `deps` 出错。

### 4.2 处理流水线

1. **输入**：文件路径、粘贴文本、或对话 turn 中的用户消息。  
2. **LLM 调用**（专用短请求或 Agent 子步）：  
   - System：工具白名单、禁止密钥、`max_items` 规则、输出 schema。  
   - User：Markdown 全文 + 可选工作区上下文（**不**含 API key）。  
   - 参数：**低温**、**`max_tokens` 上限**、优先 **JSON mode**（若后端支持）。  
3. **提取**：从回复中 parse JSON / 写入 `` ```crabmate-workflow ``。  
4. **与路径 A 汇合**：`compile_workflow_spec` → `validate_only` → `workflow_execute`。  
5. **可选落盘**：`--freeze` 将生成块写回 MD，下次走路径 A。

### 4.3 优点

| 点 | 说明 |
|----|------|
| **作者体验** | 产品、测试、运维可用母语写流程 |
| **从 prose 到结构** | 适合探索期、一次性流水线 |
| **与 Agent 一体** | 对话中「帮我按这个 md 跑一遍」无需手写 JSON |
| **分支叙事** | 模型擅长把「如果…则…」译为 `when` 或展开节点 |

### 4.4 缺点与缓解

| 缺点 | 缓解 |
|------|------|
| **非确定性** | 默认 `validate_only` + 用户确认；`--freeze` 落盘块 |
| **幻觉工具名** | `workflow_tool_args_satisfy_required` + 未知工具硬失败 |
| **成本与延迟** | 缓存编译结果；仅无块时调 LLM |
| **安全** | 不把 MD 全文打进 info 日志；侧向请求 fail-closed 可选（配置） |
| **审计难** | 记录 `workflow_run_id` + 输入 hash + 模型 id，不记录密钥 |

### 4.5 与现有 Agent 能力的关系

| 模式 | 说明 |
|------|------|
| **隐式（今日可做）** | 用户贴 MD，模型直接 `workflow_execute` 手写 `nodes` | 缺统一 schema，易漂移 |
| **显式（推荐）** | 新工具 `workflow_from_markdown` 或编排提示词要求先出 `workflow_spec` 再执行 | 可测、可 validate |
| **外环循环** | 「直到测试通过」用 **多轮 `agent_turn`** + 每轮小 DAG，而非单次解析无限循环 | 与 P-E-V 一致 |

**`final_plan_semantic_check`** 类侧向 LLM 仅用于**规划一致性**，不替代 workflow 编译；若对「解析结果 vs 用户 MD」做语义检查，应单独开关并 **fail-open/fail-closed** 在配置中写明（默认建议 **fail-closed 仅阻止执行、不静默改 DAG**）。

---

## 5. 推荐：混合策略（生产默认）

```mermaid
flowchart TD
  IN[输入 Markdown]
  IN --> Q{存在 crabmate-workflow 块?}
  Q -->|是| A[路径 A: 提取 + 编译]
  Q -->|否| CFG{配置允许 NL 解析?}
  CFG -->|否| ERR[错误: 须补充代码块或模板]
  CFG -->|是| B[路径 B: LLM → spec]
  B --> W{写入块? freeze}
  W -->|是| DISK[更新 .md 块]
  W -->|否| A
  DISK --> A
  A --> V[validate_only 可选]
  V --> E[workflow_execute]
```

| 配置键（建议） | 含义 |
|----------------|------|
| `workflow_author_mode` | `deterministic_only` \| `llm_fallback` \| `llm_always` |
| `workflow_llm_freeze_on_success` | 解析成功后是否写回围栏块 |
| `workflow_spec_max_steps` | 编译后节点数硬顶 |
| `workflow_for_each_max_items` | `for_each` 默认与上限 |

**严格环境**（CI、受信工作区）：`deterministic_only`。  
**交互环境**（Web / Client 终端）：`llm_fallback`（无块才调模型）。

---

## 6. 中间表示：`workflow_spec` v2（编译契约）

**语法与字段的权威说明（使用者视角）** 见 **`docs/工作流编写教程.md`**：`steps` 常用能力见其 §5、`nodes` 模式见 §6、字段速查见 §7。本节只保留 **作者层 → 编译层** 的契约与设计约束，不重复 YAML 示例。

- **编译后仅保留 `nodes`**，与现有 **`workflow.nodes`（对象/数组）** 同形，交 **`parse_workflow_spec`** 消费。
- **必需保证**：`for_each.max_items` / `repeat.count` 硬上限；块内**禁止** shell、任意表达式语言与无界 `while`。
- **透传**：`label`（trace `display_name` 扩展）、`requires_approval` / `node_tool_role` 原样进入 **`WorkflowNodeSpec`**。

**编译契约（概念）**：

| 作者层写法 | 编译产物 |
|------------|----------|
| `after: [id]` | `deps: [id]` |
| `when.branch` / `when.match` / `kind: choice` | 互斥 `steps` 展开 + trace `skipped` |
| `for_each`（须 `max_items`） | `id_0 … id_{n-1}`，无回边 |
| `repeat.count` | `id_1 … id_{count}` 链式 `deps` |
| 无界 `while` | **拒绝编译** |

**示例夹具**：`fixtures/workflows/`（`01_serial_after` … `09_fenced_in_markdown`，各配 `*.expected.json` 或为 `.md`）；`compile_spec` 落地后对照同名 `*.expected.json` 跑金样测试（见 §11）。无界 `for_each` 的**建议**错误码为 **`WORKFLOW_COMPILE_FOR_EACH_UNBOUND`**。

## 7. 两种路径对比总表

| 维度 | 路径 A：确定性块 | 路径 B：LLM 解析 |
|------|------------------|------------------|
| **输入** | `` ```crabmate-workflow `` | Prose MD 或 A+B 混合 |
| **确定性** | 高 | 低（可 freeze 后变高） |
| **实现核心** | 提取器 + `compile_spec` | Prompt/schema + 同上编译器 |
| **CI 友好** | 是 | 仅 freeze 后 |
| **分支/循环** | 声明式 `when` / `for_each` | 模型翻译为同上；须编译器落地 |
| **失败模式** | YAML/编译错误，信息 локаль | 模型拒答、JSON 破损、工具幻觉 |
| **密钥风险** | 低（静态文件） | 中（勿把 .env 贴进 MD） |
| **与模板关系** | 块内可写 `workflow_template: rust_ci_light` + overlay | 模型可选用模板名减少 token |
| **观测** | `workflow_run_id` + 源文件 path:line | 外加 `author_source=llm`、input_hash |

---

## 8. 分支、循环与「直到通过」（摘要）

YAML 写法详见 **`docs/工作流编写教程.md` §5**（`steps`：`after` / `when` / `for_each` / `repeat` / `kind: choice`）与 **`fixtures/workflows/`**。两种作者路径**共享** **`docs/工作流编排架构.md`** 的边界：

| 用户叙事 | 作者层表达 | 执行层 |
|----------|------------|--------|
| 「clippy 不过就跑 test」 | `when: { from: clippy, branch: failure }` | choice 剪枝；未选分支 trace 记 `skipped` |
| 「每个改动文件跑 outline」 | `for_each` + `max_items: 10` | 展开为 N 个节点 |
| 「直到 CI 全绿」 | **不**编译为单 DAG 环 | 多轮 Agent + 每轮 `workflow_execute` 或 `rust_ci_light` 模板 |

LLM 路径的额外风险：模型生成**无界** `for_each` → 编译器 **拒绝**，错误码建议 **`WORKFLOW_COMPILE_FOR_EACH_UNBOUND`**。

---

## 9. 安全、审批与可观测性

| 主题 | 要求 |
|------|------|
| **工具白名单** | 编译后仍走现有 registry；`run_command` 等受审批与 explain 约束 |
| **路径** | `workflow_from_file` 仅允许工作区内相对路径（与 `read_file` 同类规则） |
| **日志** | 记录块序号、spec version、编译后节点数；**不**记录完整 MD 若含用户机密 |
| **审批** | 动态 `when` 引入后须 **惰性审批**（见编排架构 §6） |
| **trace** | 编译阶段事件：`workflow_compile_start/end`；skipped 分支须可见 |
| **SSE** | 若 Web 展示「编译预览」，用控制面事件，**不**污染正文 delta（见 api-sse 规则） |

---

## 10. 演进阶段（建议）

| 阶段 | 内容 | 路径 |
|------|------|------|
| **P0** | 本文 + `workflow_spec` v2 示例 fixture；`extract_md_workflow_blocks` 单测 | A |
| **P1** | `compile_spec`：仅 `after` 串行 + 模板 overlay；CLI `workflow validate` | A |
| **P2** | `when` + trace skipped；`for_each` 有界展开 | A |
| **P3** | `workflow_from_markdown` 工具 + `llm_fallback` 配置 | B |
| **P4** | freeze 写回 MD；Web 预览层图 | A+B |
| **P5** | 工作区 `.crabmate/workflows/*.md` 发现与 `doctor` 检查 | A |

**非目标**：Markdown 内嵌任意 Rust/Python；单轮 DAG 内 `while(true)`；用 LLM **直接调度**工具而不经 `workflow_execute`。

---

## 11. 测试策略

| 类型 | 内容 |
|------|------|
| **单元** | 提取围栏、YAML 错误行号、`compile_spec` 展开与上限 |
| **金样** | `fixtures/workflows/*.yaml` + `*.expected.json`（`compile_spec` 落地后 `cargo test workflow_compile_golden`） |
| **集成** | `validate_only` 对编译产物通过；未知工具失败 |
| **LLM** | 可选 mock：固定 prose → 期望 spec 快照（不默认跑 live API） |

---

## 12. 文档与代码同步义务

实现时须更新：

- **`docs/工具说明.md`**：新工具、`workflow_spec` 字段、错误码  
- **`docs/工作流编排架构.md`**：choice / `for_each` 与编译器关系  
- **`docs/开发文档.md`**：架构概要（workflow 接合见「主要模块」/`agent`）  
- **`docs/命令行与路由.md`**（若有 CLI 子命令）  
- **`.cursor/rules/api-sse-chat-protocol.mdc`**（若 Web 增加编译预览事件）

---

## 13. 相关源码索引

| 区域 | 路径 |
|------|------|
| DAG 规格 | `src/cm_workflow/model.rs` |
| 解析 | `src/cm_workflow/parse.rs` |
| 模板 | `src/cm_workflow/workflow_templates.rs` |
| 调度 | `src/cm_workflow/execute/` |
| 工具分发 | `src/agent/workflow_tool_dispatch.rs` |
| 规划对齐 | `src/cm_agent/plan_artifact/` |

**建议新增**：`src/cm_workflow/compile_spec.rs`、`src/cm_workflow/md_extract.rs`、`runtime/cli_workflow.rs`（或 `cli` 子模块）。

---

## 14. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-05-16 | 初稿：路径 A（确定性围栏 + 编译器）与路径 B（LLM → spec）对比、混合策略、`workflow_spec` v2 草案、安全/测试/演进阶段。 |
| 2026-05-16 | 增补 §6.1–§6.3 分支/循环 YAML 专节；`fixtures/workflows/` 示例与 `01_serial_after.expected.json`。 |
| 2026-10-07 | 语法下沉：删除 §6.1–§6.3 的 YAML 语法/字段速查，§6 收敛为「编译契约」，使用者语法权威改指 **`docs/工作流编写教程.md` §5–§7**。 |
