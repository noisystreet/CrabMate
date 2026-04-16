# 结构化规划—执行—验证（P-E-V）闭环：架构与设计

**状态**：设计稿（**未**承诺实现时间表）。**受众**：维护者、产品与协议设计者。  
**语言**：中文。  
**关联文档**：**`docs/WORKFLOW_ORCHESTRATION_ARCHITECTURE.md`**（轮内 DAG / FSM 扩展边界）、**`docs/DEVELOPMENT.md`**（**`agent_turn` / `per_coord` / `plan_artifact` / 分阶段规划**）、**`docs/TOOLS.md`**（**`workflow_execute` / `agent_reply_plan`** 字段约定）、**`docs/CONFIGURATION.md`**（**`final_plan_*` / `staged_plan_*` / `reflection_*`**）。

---

## 1. 目标与对标

在**不否定模型创造性**的前提下，降低对**单次模型输出**的隐式依赖，形成可观测、可上限、可测试的闭环：

| 能力 | 含义 |
|------|------|
| **显式子任务拆解** | 结构化步骤列表、稳定 **`id`**、可选与 **DAG 节点** / **工具角色**绑定。 |
| **执行** | 分阶段 **E**、**`executor_kind`** 收窄工具；可选 **同轮 `workflow_execute` DAG**。 |
| **验证** | 对**执行结果**的**确定性或可重复**判定（退出码、结构化输出、diff 指纹等），而非仅「再问一次模型好不好」。 |
| **反思 / 重试** | **有界**：区分「规划形式错误」与「世界未满足验收」；与 **SSE / trace** 对齐。 |

对标 **AutoGPT / SWE-agent** 类外环时须注意：本仓库**已有**多轮 **`agent_turn`**、**终答规划校验与重写**、**工作流反思**与 **可选侧向 LLM 一致性检查**；本文定义**缺口**与**推荐增量**，避免与现有 **`plan_rewrite`**、**`workflow_reflection_controller`** 语义重叠或 silent 分叉。

---

## 2. 现状能力谱系（实现已存在）

### 2.1 结构化规划（Plan）

- **`agent_reply_plan` v1**（**`src/agent/plan_artifact.rs`**）：**`type` / `version` / `steps[]`**；每步 **`id`**、**`description`**；可选 **`workflow_node_id`**（与 **`workflow_execute` / `workflow_validate_only`** 结果中 **`nodes[].id`** 对齐）；可选 **`executor_kind`**（**`review_readonly` / `patch_write` / `test_runner`**）收窄该步 **E** 可见工具。  
- **`workflow_validate_only`**：只生成 **`workflow_validate_result`**，用于「先校验 DAG 再让模型写规划」及 **validate-only 逐步绑定**规则（见 **`plan_artifact`** 错误 **`ValidateOnlyPlanNodeBindingMismatch`** 等）。

### 2.2 执行（Execute）

- **分阶段规划**（**`agent_turn::staged`**）：无工具规划轮 → 将 **`steps`** 注入为多条 **user** → 每步跑完整工具子循环（**E**），并由 **`sub_agent_policy`** 按 **`executor_kind`** 拒绝越权 **`tool_calls`**。  
- **`workflow_execute`**：同轮 **DAG**；与「分阶段单步」正交，可由规划 **`workflow_node_id`** 对齐责任。

### 2.3 验证（Verify）— **已实现**

- **确定性验证闸门**（**`step_verifier.rs`**）：支持以下验收规则：
  - **`expect_exit_code`**：退出码验证（如 `cargo test` → 0）
  - **`expect_stdout_contains`**：stdout 是否包含指定字符串
  - **`expect_stderr_contains`**：stderr 是否包含指定字符串
  - **`expect_file_exists`**：文件是否存在
  - **`expect_json_path_equals`**：JSON path 验证（支持 `$.field.nested` 和 `$[0].field` 格式）
  - **`expect_http_status`**：HTTP 状态码验证（对 `http_request`/`http_fetch` 类工具生效）
- **隐式验证**：工具返回的 **`error_code` / 退出码 / 结构化 `crabmate_tool` 信封** 已构成事实；模型在后续轮读取。  
- **`final_plan_semantic_check_enabled`**（**`per_plan_semantic_check`**）：在静态 **PER** 规则通过后，可选 **再调一次无工具 LLM**，判断「终答规划 vs 最近工具摘要」是否明显矛盾（**fail-open** 等语义以配置为准）——偏 **「规划一致性」**，**不是**通用「验收测试通过」闸门。

### 2.4 反思与重试（Reflect / Retry）— 当前「部分存在」

- **`final_plan_requirement` + `plan_rewrite_max_attempts`**（**`per_coord::after_final_assistant`** + **`reflection/plan_rewrite`**）：终答缺合格 **`agent_reply_plan`**、或与 **workflow 节点覆盖 / validate-only 绑定**不符时，追加重写 **user** 再进入下一轮 **P**。  
- **`WorkflowReflectionController`**（**`src/agent/workflow_reflection_controller.rs`**）：控制 **是否再执行 `workflow_execute`**、是否注入 **`workflow_reflection_plan_next`**、是否对参数打 **`validate_only`** 补丁等（**`WorkflowReflectionDecision`**）。  
- **分阶段失败策略**：**`staged_plan_feedback_mode`**（**`fail_fast` / `patch_planner`**）等（见 **`docs/CONFIGURATION.md`**）。

---

## 3. 缺口与设计原则

### 3.1 缺口

1. **缺少「步级验收」一阶对象**：验收条件多散落在 **`description` 自然语言**或模型自行理解中，难以 **确定性重放**。  
2. **`plan_rewrite` 与「执行失败」未统一**：**`plan_rewrite`** 主要解决 **规划 JSON / 绑定契约**；**测试失败 / 业务未达成** 不自动等价于「应触发同一条重写路径」。  
3. **外环有界性**：多轮依赖模型自省时，须有与 **`plan_rewrite_max_attempts` / `reflection_default_max_rounds`** 同哲学的 **全局步级/验证级上限**，并在 **SSE** 用 **`reason_code`** 区分失败类别。

### 3.2 设计原则

| 原则 | 说明 |
|------|------|
| **单一结构化计划源** | 继续以 **`plan_artifact`** 为规划真源；扩展字段须 **serde 向后兼容**、文档与 OpenAPI（若有）同步。 |
| **验证优先确定性** | 能解析 **`tool` 输出 / 退出码 / 文件指纹**的不要用 LLM；LLM 仅用于**解释**或**草案 replan**（可选）。 |
| **与 `plan_rewrite` 正交** | **形式与绑定** → **`plan_rewrite`**；**世界未满足 acceptance** → **Verifier 闸门**（新语义，新配置与 SSE 码）。 |
| **有界循环** | 任意外环须 **三重上限**（步内重试次数、重规划次数、墙钟/token），默认保守。 |
| **可观测** | 验证结果进入 **`messages`**（如 **`user.name=crabmate_step_verify`** 占位约定）或 **trace** 扩展事件；禁止仅 stderr 黑箱。 |

---

## 4. 推荐架构：三层 + 闸门

```mermaid
flowchart TB
  subgraph plan [Plan 声明层]
    ARP["agent_reply_plan v1\n(plan_artifact)"]
    WV["workflow_validate_only\n可选"]
  end
  subgraph exec [Execute 执行层]
    ST["staged: 每步 E"]
    WE["workflow_execute DAG\n同轮可选"]
  end
  subgraph verify [Verify 闸门层 设计增量]
    V["StepVerifier\n确定性规则"]
  end
  subgraph retry [Reflect / Replan 有界]
    PR["plan_rewrite\n形式与绑定"]
    RP["replan_on_verify\n设计增量"]
    WRC["WorkflowReflectionController\nDAG 反思路径"]
  end
  ARP --> ST
  WV --> ARP
  ST --> WE
  ST --> V
  V -->|Fail| RP
  V -->|Pass| ST
  RP --> ARP
  ARP --> PR
  ST --> WRC
```

**注**：**Verify → Pass → ST** 为抽象画法，表示「本步验收通过后，由**分阶段编排**进入**下一步 E** 或进入 **PER / 终答**」；**不**表示在同一轮 `tool_calls` 内无意义地回到同一执行入口。

**说明**：

- **Plan**：模型或模板产出 **`agent_reply_plan`**；可选先 **`workflow_validate_only`** 再要求 **`workflow_node_id`** 全绑定（已有规则）。  
- **Execute**：**分阶段 E** 为主；**`workflow_execute`** 承载同轮并行/流水线。  
- **Verify（建议新增）**：在每步 **E** 结束（或整轮终答前）运行 **Verifier**：输入为本步相关 **`tool` 消息**（及配置中的 **`acceptance`** / 工具名白名单），输出 **`Pass | Fail(reason) | EscalateHuman`**。  
- **Reflect / Replan**：**`Fail`** 且未超限时 → **追加结构化 user**（含 verifier 摘要、**`step_id`**、**`workflow_run_id`** 指针）→ 下一轮 **P**（**replan**）或 **仅重跑本步 E**（**局部 retry**）；与 **`plan_rewrite`** 并行时须 **文档定义优先级**（建议：**先**解决形式规划 **`plan_rewrite`**，**再**进入 **verify** 闸门）。

---

## 5. `PlanStepV1` 扩展方向（可选字段，设计级）

以下字段名为**设计提案**，实现前须走 serde 与 **`docs/TOOLS.md`** 契约评审：

| 字段（提案） | 类型 | 用途 |
|--------------|------|------|
| **`step_kind`** | 枚举可选 | **`implement` / `verify` / `gate`**：指导 UI 与 Verifier 是否在本步强制跑验收。 |
| **`acceptance`** | 对象可选 | 如 **`{ "tool": "cargo_test", "expect_exit_code": 0 }`** 或 **`{ "json_path": "…", "equals": … }`**——须限制表达能力，避免 Turing-complete 配置。 |
| **`max_step_retries`** | 整数可选 | 本步 **E** 内或跨轮 **局部**上限；与全局上限组合取 min。 |

**兼容**：全部可选；省略时行为与当前一致（仅靠模型与 **`executor_kind`**）。

---

## 6. 与现有模块的衔接清单

| 模块 | 衔接方式 |
|------|----------|
| **`plan_artifact`** | 解析/校验扩展字段；错误枚举新增须 **不破坏**现有 **`PlanArtifactError`** 处理路径。 |
| **`agent_turn::staged`** | 每步 **E** 结束后调用 **Verifier**（若启用）；决定 **继续下一步 / 重跑本步 / 中止并 SSE**。 |
| **`per_coord`** | **终答后** **`plan_rewrite`** 逻辑保持不变；**Verifier 触发的 replan** 建议走 **独立计数器**（如 **`verify_replan_attempts`**），避免与 **`plan_rewrite_attempts`** 混用导致排障困难。 |
| **`workflow_reflection_controller`** | **DAG 反思路径**不变；若 **分阶段步级验证失败** 需注入与 **`INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`** 类似指令，应 **复用或并列新常量**，并在 **`DEVELOPMENT.md`** 说明语义差异。 |
| **`final_plan_semantic_check`** | 保持 **「规划 vs 工具摘要」**；**步级 Verifier** 不应用侧向 LLM 替代确定性验收。 |
| **SSE** | 新增控制面事件或 **`reason_code`** 须在 **`docs/SSE_PROTOCOL.md`**、**`crabmate-sse-protocol`**、**`frontend-leptos`** 同步（见仓库 **api-sse-chat-protocol** 规则）。 |

---

## 7. 非目标与风险（当前共识）

- **不在首版**引入任意表达式语言作为 **`acceptance`** 的通用求值器（安全与测试成本）。  
- **不把** **`workflow_execute` DAG** 改成通用「验证图灵机」；验证外环仍以 **会话级 `agent_turn` + `plan_artifact`** 为主（与 **`WORKFLOW_ORCHESTRATION_ARCHITECTURE.md`** 一致）。  
- **默认不**无界 **「直到模型满意」** 循环；所有 **replan / retry** 须有配置上限与可观测 **耗尽** 事件。

---

## 8. 演进阶段（建议）

| 阶段 | 状态 | 内容 | 产出 |
|------|------|------|------|
| **P0** | ✅ 完成 | 文档化「推荐验收模式」 | **`docs/TOOLS.md` + 本稿附录示例** |
| **P1** | ✅ 完成 | **Verifier** 支持 **退出码 + JSON 路径 + HTTP 状态** | **Rust 闸门 + 单测** |
| **P2** | 🔄 进行中 | **`PlanStepV1`** 可选字段 **`step_kind` / `acceptance` / `max_step_retries`** | **serde 迁移 + 前端展示（若有）** |
| **P3** | ⏳ 待办 | 与 **Chrome trace / `/status`** 对齐的 **verify 事件** | **观测性完备** |

---

## 9. 相关源码索引

| 主题 | 路径 |
|------|------|
| 规划 JSON | `src/agent/plan_artifact.rs` |
| 分阶段执行 | `src/agent/agent_turn/staged.rs`、`sub_agent_policy` |
| **步级验收闸门** | `src/agent/step_verifier.rs` |
| PER / 重写 | `src/agent/per_coord.rs`、`src/agent/reflection/plan_rewrite.rs` |
| 工作流反思 | `src/agent/workflow_reflection_controller.rs` |
| 侧向规划检查 | `src/agent/per_plan_semantic_check.rs` |
| DAG 执行 | `src/agent/workflow/`、`src/agent/workflow_tool_dispatch.rs` |
| validate-only | `src/agent/workflow/run.rs` |

---

## 10. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-04-12 | 初稿：P-E-V 分层、与现有能力映射、Verifier 闸门与 `plan_rewrite` 正交、演进阶段与非目标。 |
| 2026-04-16 | P1 完成：`step_verifier.rs` 支持 JSON path 和 HTTP 状态验证；`PlanStepAcceptance` 扩展字段。 |

---

## 附录：`acceptance` 使用示例

### 基础退出码验证

```json
{
  "type": "agent_reply_plan",
  "version": 1,
  "steps": [
    {
      "id": "run-tests",
      "description": "运行项目测试",
      "executor_kind": "test_runner",
      "acceptance": {
        "expect_exit_code": 0
      }
    }
  ]
}
```

### 多规则组合验证

```json
{
  "steps": [
    {
      "id": "build",
      "description": "编译项目",
      "executor_kind": "patch_write",
      "acceptance": {
        "expect_exit_code": 0,
        "expect_stderr_contains": "Finished"
      }
    },
    {
      "id": "verify-build-artifact",
      "description": "验证构建产物",
      "executor_kind": "review_readonly",
      "acceptance": {
        "expect_file_exists": "target/release/myapp"
      }
    }
  ]
}
```

### JSON Path 验证

```json
{
  "steps": [
    {
      "id": "api-check",
      "description": "调用健康检查 API",
      "acceptance": {
        "expect_exit_code": 0,
        "expect_json_path_equals": {
          "path": "$.status",
          "value": "healthy"
        },
        "expect_http_status": 200
      }
    }
  ]
}
```

### 带重试与控制流

```json
{
  "steps": [
    {
      "id": "apply-fix",
      "description": "应用修复补丁",
      "executor_kind": "patch_write",
      "max_step_retries": 3,
      "transitions": [
        {
          "condition": "on_verify_fail",
          "target_step_id": "apply-fix",
          "max_loops": 3
        }
      ]
    },
    {
      "id": "verify",
      "description": "运行测试验证",
      "executor_kind": "test_runner",
      "step_kind": "verify",
      "acceptance": {
        "expect_exit_code": 0,
        "expect_stdout_contains": "all tests passed"
      }
    }
  ]
}
```
