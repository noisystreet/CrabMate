# CrabMate 改进建议

> 本文档基于源码分析、主流开源 Agent 方案对比与能力差距分析，对 CrabMate 未来改进方向进行优先级排序，供社区参考。

---

## 背景

CrabMate 是一个以 Rust 构建的本地优先编程开发向 Agent，支持 80+ 工具、多种 LLM 后端、本地向量记忆与 DAG 工作流。与主流开源方案（CrewAI、AutoGPT、Mastra、LangChain Agents 等）相比，在部分架构能力上存在差距。以下按优先级分 P0–P5 给出改进建议。

---

## P0 — 安全性（已有 TODOLIST 条目，直接可推进）

| 改进项 | 说明 | 实现难度 |
|--------|------|---------|
| **API 鉴权与多租户隔离** | `/chat`、`/chat/stream`、工作区 API 均未校验调用方身份；Bearer token 鉴权已具备框架，需在 `serve` 启动时强制开启并覆盖所有写操作接口 | 低（配置层面） |
| **工作区路径 TOCTOU** | `O_NOFOLLOW` / `openat2` 贯通文件打开路径；已有模块注释与风险说明 | 中（需改动路径操作核心模块） |

> **驱动因素**：安全问题是生产部署的前置条件，与其他改进项解耦，应优先完成。

---

## P1 — 架构层面（多 Agent 与自主 Agent）

| 改进项 | 说明 | 实现难度 |
|--------|------|---------|
| **多 Agent 协作框架** | 引入 Agent 实例抽象（各有模型/工具/系统提示配置）；通过共享消息总线或队列协作；可参考 CrewAI 的 Role-based Agent + Task 机制 | 高（需设计新架构层） |
| **主动型自主 Agent 模式** | 支持"目标 → 自主循环执行直到完成"，参考 AutoGPT 的 TaskComplete 判定逻辑；可与现有 `workflow_execute` DAG 结合 | 高（需新增主循环模式） |
| **MCP Server 暴露** | 将 CrabMate 自身工具以 MCP server 协议暴露；复用现有 `tool_registry` 导出能力 | 中（协议层已有参考实现） |

> **驱动因素**：这两个方向是 CrabMate 与主流方案拉开差距的关键；MCP server 实现相对独立，可作为多 Agent 架构的子集先行验证。

---

## P2 — 体验与可观测性（可逐步推进）

| 改进项 | 说明 | 实现难度 |
|--------|------|---------|
| **Benchmark 评测落地** | 在已有 `crabmate bench` 框架上，运行 SWE-bench / GAIA / HumanEval，建立能力基线与回归对照 | 中（需数据集与评测流程） |
| **可视化 Workflow 编辑** | Web UI 增加 DAG 可视化节点编辑器；复用现有 `workflow_execute` DAG schema | 高（前端工程量） |
| **多级自我修复/重规划** | 在现有 `per_coord` + `plan_rewrite` 基础上，增加逐工具失败重试 → 逐步骤回退 → 整规划层重写的多级恢复策略 | 中（逻辑扩展） |
| **Token / 费用估算** | 在 `llm` 模块的 `ChatRequest` 构造处粗算 token；追加 `usage_metadata` 字段到响应结构；与上下文预算联动 | 低（计算层改动） |

> **驱动因素**：Benchmark 和可观测性是工程成熟度的标志；可视化编排用户体验收益大但工程投入高，建议后期迭代。

---

## P3 — 生态与扩展性（中长期）

| 改进项 | 说明 | 实现难度 |
|--------|------|---------|
| **外部向量库支持** | Qdrant / pgvector adapter；解耦现有 `long_term_memory_store`，抽象为 trait；与 P0 多租户同向 | 中（需存储层抽象） |
| **外部可观测平台集成** | OpenTelemetry trace export；与现有 Chrome Trace 机制共用 span 模型 | 低（对接标准协议） |
| **云端扩缩容** | Redis/SQS 队列替代进程内 mpsc；会话共享需统一存储层 | 高（分布式系统改造） |

> **驱动因素**：这些是规模化和商业化部署的前置；当前单进程架构对多数场景仍够用，属于演进选项。

---

## 推进路线图

### 近期（1–2 个月）

聚焦 **P0 安全** + **Benchmark 评测落地**。

- 前者解锁生产部署，后者建立能力基线为后续迭代提供客观度量。
- 具体行动：
  1. 在 `config/default_config.toml` 中将 Bearer 鉴权设为默认开启，并审计所有写操作接口（`/chat/stream`、文件操作、Git 操作等）是否均已覆盖鉴权检查。
  2. 运行 `crabmate bench --suite=swe_bench`，产出基线分数并记录到 `docs/BENCHMARK.md`。

### 中期（3–6 个月）

推进 **MCP Server 暴露** + **多 Agent 协作框架**设计。

- MCP Server 可先作为独立模块验证，复用现有 `tool_registry` 导出能力。
- 多 Agent 协作可先做单进程内的逻辑角色分离扩展（如增加 `role` 字段到 Agent 配置），再演进为独立实例。
- 具体行动：
  1. 实现 `src/mcp/server.rs`，将 `tool_registry` 中所有工具按 MCP tool schema 暴露为 stdio server。
  2. 设计 Agent 实例抽象（`src/agent/core/agent.rs`），支持独立配置模型/工具/系统提示，引入共享消息队列供协作。

### 远期（6 个月+）

**可视化编排 UI** + **云端扩缩容**。

- 这些需要前端工程能力和分布式系统改造，建议在用户量增长驱动时再投入。
- 具体行动：
  1. 在 Leptos 前端中引入 DAG 可视化库（如 `vue-flow` 或自研 canvas），复用 `workflow_execute` 的 JSON schema。
  2. 评估 Redis pub/sub 或 SQS 替代进程内 mpsc，引入会话存储服务（PostgreSQL/SQLite）实现跨进程共享。

---

## 附录：与主流开源 Agent 能力对照

| 能力维度 | 主流方案代表 | CrabMate 现状 |
|----------|------------|--------------|
| 多 Agent 协作 | CrewAI、AutoGen | ❌ 仅单 Agent（logical_dual_agent 是逻辑角色分离，非独立协作） |
| 可视化编排 | Mastra、Flowise、LangFlow | ❌ 无 UI 编排器 |
| Benchmark 评测 | SWE-bench、GAIA、HumanEval | ⏳ 框架就绪，待落地 |
| 外部向量库 | Qdrant、pgvector、Pinecone | ⏳ 仅 fastembed 本地 |
| MCP 双向 | MCP 生态 | ⏳ 仅 client，server 待实现 |
| Token/费用估算 | LangChain Usage Tracking | ❌ 未实现 |
| 主动型自主 Agent | AutoGPT、BabyAGI | ❌ 响应式，工具按需调用 |
| 多级自我修复 | AutoGPT 内置重试、LangChain Plan-and-Execute | ⏳ 基础规划重写（plan_rewrite），待增强 |
| 云端扩缩容 | Mastra、CrewAI 云部署 | ❌ 单进程，分布式待实现 |
| 外部可观测平台 | LangSmith、OpenTelemetry | ⏳ 仅 Chrome Trace |
| API 鉴权 | — | ❌ TODOLIST P0 |

---

*文档生成时间：基于 CrabMate 源码（`src/`、`config/`、`docs/`）与主流 Agent 框架对比，时间 2026-01。*
