**语言 / Languages:** 中文（本页）· [English](en/TODOLIST.md)

# 后续修复与完善清单

本文仅保留**未完成**待办。**某项完成后必须从本文件删除该条目**（勿长期保留 `[x]`）；空的小节可删掉标题。追溯完成时间请查 Git。维护约定见 `docs/DEVELOPMENT.md`「TODOLIST 与功能文档约定」。

**结构**：

- **全局优先级（跨模块）**：按 **P0–P5** 共识项（安全、架构、测试、运维等）；与模块章可能交叉，实现后删除即可。
- **按模块分章**：每个功能域单独一章（`agent/`、`llm/`、`tools/` 等），章首**职责摘要**便于定位；条目前可含与全局段的交叉引用。

---

## 全局优先级（跨模块）

### P0 — 安全（非本机部署前建议处理）

- [ ] **HTTP 无鉴权**：`/chat`、`/chat/stream`、工作区、文件、上传、任务等均未校验调用方身份；`API_KEY` 仅用于调模型，不能防止他人滥用接口与配额。
- [ ] **多角色与人设切换**：支持创建/配置多种**角色**（各角色可绑定不同系统提示、工具可见性、温度等策略）；**CLI**（如 `repl` 的 `/` 命令）与 **Web** 提供**内建命令或等价控件**切换当前会话生效角色，且与会话持久化、导出、`POST /config/reload` 可重载字段的边界在 `README` / `docs/DEVELOPMENT.md` 中写清；若对多租户开放须与上条鉴权同盘。

### P4 — 测试与质量

- [ ] **生产路径 unwrap/expect 审计**：梳理非测试代码中的 `unwrap` / `expect`（如 `per_coord`、`conversation_store`、命令退出码处理），改为显式传播或带业务上下文的 `expect`，降低低概率 panic 与排障成本。
- [ ] **集成/契约测试**：在 `lib_smoke` 与 **`tests/cli_contract.rs`**（`parse_args_from_argv`、`normalize_legacy_argv` fixture、`classify_model_error_message` / `EXIT_*`）之外，可为 `plan_artifact` 边界、`classify_agent_sse_line` 协议行、`workflow_reflection_controller` 状态迁移增加 fixture 或快照用例。
- [ ] **`stream_chat` 非流式**：可选 wiremock / 静态 JSON fixture 测 `ChatResponse` 解析。
- [ ] **Agent Benchmark 测评与基线**：在主流 agent benchmark（SWE-bench、HumanEval、GAIA 等）上对 CrabMate 做系统性评估，建立能力基线与回归对照，覆盖工具调用、多步推理、代码生成等；批量测评框架已具备（`--benchmark` + `--batch`，支持 SWE-bench / GAIA / HumanEval / Generic），后续在实际数据集上跑通完整流程并记录基线分数、持续追踪迭代。

### P5 — 运维与体验

- [ ] **跨进程 / 多副本队列**：当前为**单进程**内 `mpsc` + `Semaphore`；水平扩展需 Redis/SQS 等外部代理与持久化，本仓库未实现。
- [ ] **限流 / 配额**：对 `/chat`、`/chat/stream` 按 IP 或 token 限流（常与 P0 鉴权一起做）。
- [ ] **日志关联**：多轮会话落地后，可统一 `request_id` / `conversation_id`（依赖 P1 会话模型）。

---

## 路线图参考（对标主流开源 Agent）

以下由「与 AutoGen / CrewAI / LangGraph / Open Interpreter 等对照」整理为**方向性**待办，可与上文章节交叉推进；实现后按惯例删除条目。

- [ ] **多 Agent 协作**：多角色实例、消息路由与监督式编排（对标 AutoGen / CrewAI / MetaGPT）；与当前单 `agent_turn`、会话与配额模型如何共存须在设计与 `docs/DEVELOPMENT.md` 中预案。
- [x] **结构化规划—执行—验证闭环**：已实现显式子任务拆解、执行结果校验与反思/重试策略，降低对单次模型输出的隐式依赖（对标 AutoGPT / SWE-agent 类循环）。具体实现：
  - **扩展 `SubGoal`**：添加 `acceptance` 验收条件和 `max_retries` 最大重试次数字段
  - **实现 `GoalVerifier`**：验证器组件支持文件存在性、输出内容匹配、退出码验证、验证命令执行
  - **集成到 `HierarchicalExecutor`**：在每个子目标执行后添加验证阶段，形成「执行 → 验证 → 反思/重试」闭环
  - **实现反思与重规划**：`ManagerAgent.reflect_and_replan()` 在验证失败时分析原因并生成修复策略
  - **可观测性增强**：添加 `verification_passed/failed/escalated`、`reflection_started/finished` SSE 事件
- [ ] **交互式代码执行与受控 REPL**：在 `run_command` 白名单与沙盒策略之上，评估解释执行、会话级依赖安装与输出校验（对标 Open Interpreter）；安全面与 `tool_approval`、Docker 沙盒文档对齐。
- [ ] **工作流编排产品化**：在已有 `workflow_execute`（DAG）等能力上，补齐更接近 LangGraph 的**默认入口**——状态机式配置、条件/循环的可读表达、运行态可视化与排障故事；主聊天回合仍为线性时，在文档中写清「对话流 vs 工作流」边界。**架构设计见 `docs/WORKFLOW_ORCHESTRATION_ARCHITECTURE.md`（DAG/FSM 边界）与 `docs/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`（规划—执行—验证闭环与 `plan_rewrite` 正交）；实现前以该文为共识并随代码迭代修订。**
- [ ] **工具与连接器生态**：在 MCP 与 `dev_tag` 分栈工具之外，系统化高频集成（DB、云 API、办公等）或维护「推荐连接器」清单与接入模板，降低重复造轮子（与 `tools/` 章 MCP 扩展条同向）。
- [ ] **短期会话与主题轨迹**：多轮主题一致性、轮次级摘要与注入策略（与 `context_window`、长期记忆「检索质量」条目协同），缩小与主流「时序记忆 + 压缩」体验的差距。
- [ ] **可观测与执行轨迹**：在 tracing 之上，为 Web/CLI 提供回合内工具时间线、失败重试与推理/思维过程的统一可视化（与 `sse/` 调试类事件、横切 `request_id` 同向）。
- [ ] **文档、示例与社区引导**：Cookbook、典型场景模板、第三方工具/MCP 贡献说明，便于对标主流社区的扩展路径。

---

## `agent/`（回合编排、上下文、PER、工作流）

**职责摘要**：`agent_turn` 主循环；`context_window` 裁剪/摘要；`reflection/plan_rewrite` 终答规划重写与历史扫描；`per_coord` / `plan_artifact` / `workflow_reflection_controller`；`workflow` DAG 执行。

- [ ] **规划器/执行器阶段 2（模型与预算解耦）**：在阶段 1 逻辑双 agent 基础上，为 planner / executor 提供独立模型、温度、max_tokens 与上下文预算，建立成本/时延对照基线。
- [ ] **规划器/执行器阶段 3（物理拆分可选）**：评估是否拆分为独立进程/服务（队列、会话与重试语义一致），目标是故障隔离与独立扩缩容；若收益不足则保留同进程架构。
- [ ] **规划与反思策略可插拔**：在现有 `FinalPlanRequirementMode` 与已落地的 `final_plan_require_*` / `final_plan_semantic_check_*` 之上，继续按场景细化（如按工具类型门控、非 workflow 路径策略等）。
- [ ] **测试与回归基线**：对 `plan_artifact` 解析边界、`run_staged_plan_then_execute_steps` 与 `context_window` 组合增加 fixture/快照测试（与 P4 可合并实现）。
- [ ] **长期记忆：外部向量库与多租户**：当前已支持会话级 SQLite + 可选本地 **fastembed** 检索；后续可接 **Qdrant** / **pgvector**、跨会话租户键、与 P0 鉴权强绑定及更细 `redact` 策略。
- [ ] **长期记忆：检索质量**：混合检索（如 SQLite **FTS5** 与向量分数加权）、多轮上下文或会话主题感知的 **query** 构造，降低仅依赖末条 `user` 的漂移与漏召。
- [ ] **长期记忆：条目生命周期与去重**：**TTL**、访问频率或显式 **pin**；语义分块与近重复合并/淘汰策略，避免 `max_entries` 仅按时间删旧导致有效事实被挤出。
- [ ] **长期记忆：写入与索引策略**：选择性索引（跳过过短轮次、纯工具调试等）、同步索引与失败重试/补偿可配置；索引耗时与错误在可观测面汇总。
- [ ] **长期记忆：运营与合规 API**：在 **P0 鉴权**落地前提下，提供只读列表/按 scope 删除等管理接口（排障与「被遗忘权」类需求）；`/status` 可暴露条目规模、embed 降级次数等（不脱敏全文）。
- [ ] **长期记忆：Web 与文档体验**：侧栏或面板展示「本轮注入摘要」、单条忽略/置顶；在 UI/README 中区分**长期记忆**、**工作区备忘文件**与**项目画像**的职责边界。
- [ ] **分层 Agent：Operator 执行指导扩展**：当前已实现编译/构建类、文件操作类任务的步骤指导（`build_execution_guide`）；后续可扩展更多任务类型：
  - **测试类任务**：检测测试框架 → 运行测试 → 收集结果 → 生成报告
  - **调试类任务**：定位问题 → 分析原因 → 实施修复 → 验证修复
  - **部署类任务**：打包 → 上传 → 验证部署 → 健康检查
  - **代码审查类任务**：静态分析 → 风格检查 → 安全扫描 → 生成报告
  - **依赖管理类任务**：检测依赖文件 → 分析冲突 → 更新/安装 → 验证
- [ ] **分层 Agent：执行步骤持久化与恢复**：复杂任务（如编译大型项目）可能耗时较长，支持将 Operator 的 ReAct 状态（迭代次数、消息历史、观察记录）持久化，允许中断后恢复执行。
- [x] **分层 Agent：结构化规划-执行-验证闭环（质量保障）**：已实现完整的 P-E-V 闭环：
  - **规划阶段**：Manager 分解任务时可为子目标定义 `acceptance` 验收条件
  - **执行阶段**：Operator 执行子目标
  - **验证阶段**：`GoalVerifier` 根据 `acceptance` 验证执行结果（文件存在、输出匹配、退出码、验证命令）
  - **反思/重规划阶段**：验证失败时，`ManagerAgent.reflect_and_replan()` 分析失败原因并生成修复策略
  - **重试阶段**：使用修复后的子目标重新执行，最多 `max_retries` 次
  - **可观测性**：通过 SSE 事件实时展示验证和反思状态
- [x] **分层 Agent：工具审批流程集成**：已修复并行执行（Parallel/Hybrid）中未传递审批上下文的问题，现在顺序和并行执行都支持 Web 审批对话框。
- [x] **分层 Agent：动态子目标分解**：已实现 `DynamicDecomposer`，Operator 执行过程中检测到目标过于复杂时（复杂度评分 >= 阈值），自动触发 Manager 反思和重新规划。

### 分层 Agent 架构改进（`hierarchy/` 模块）

**职责摘要**：Manager + Operator 分层架构；Router 路由决策；任务分解与执行；ArtifactStore 产物管理；BuildState 构建状态；工具执行与审批。

- [x] **路由层智能化（高优先级）**：已实现 `SmartRouter`，支持四种路由策略：
  - **RuleBased**：基于关键词匹配的快速路由（默认，无 LLM 开销）
  - **LlmBased**：使用 LLM 分析任务语义进行智能路由决策
  - **HistoryBased**：基于历史执行数据推荐执行模式
  - **UserOverride**：用户显式指定执行模式
  - 配置项 `enable_llm_routing` 控制是否启用 LLM 路由（默认关闭）
- [x] **真正的并行执行（高优先级）**：已实现基于 `tokio::spawn` 的真正并发执行：
  - 使用 `tokio::sync::Semaphore` 控制并发度（`max_parallel`）
  - 每个子目标在独立的 `tokio::spawn` 任务中执行
  - 使用 `Arc` 共享配置数据，避免克隆开销
  - 执行完成后自动合并产物到主 `artifact_store`
  - 并行执行已支持 SSE 事件流和工具审批
- [x] **失败恢复与自我修复（高优先级）**：已实现错误分类器，支持以下错误类型：
  - `OpenMPError`：OpenMP 并行区域错误
  - `CompilerVersionError`：编译器版本不兼容
  - `MissingDependency`：缺少依赖库
  - `ConfigError`：配置错误（如 arch 参数）
  - `WorkingDirectoryError`：工作目录错误
  - `SyntaxError`：语法错误
  - `LinkError`：链接错误
  - 每种错误类型都有对应的修复建议和重试策略
- [ ] **Manager 任务分解质量优化（中优先级）**：优化分解 Prompt，增加 Few-shot 示例引导；引入子目标粒度评估机制；支持基于代码结构的分解（按模块/函数）；添加子目标依赖自动检测。
- [ ] **产物管理增强（中优先级）**：`artifact_store.rs` 添加产物版本历史、内容哈希去重、元数据搜索、依赖图谱可视化。
- [ ] **构建状态扩展（中优先级）**：`build_state.rs` 扩展支持更多构建系统（Maven、Gradle、Webpack 等）；实现更精细的依赖变更检测；添加构建缓存共享机制。
- [x] **Operator ReAct 循环优化（中优先级）**：已完成以下优化：
  - 添加连续失败检测机制（3次连续失败自动终止）
  - 增强工作目录管理（系统提示中明确显示当前工作目录）
  - 添加详细的编译任务执行指导（步骤0-5的完整流程）
  - 支持错误类型跟踪和重复错误检测
  - 待完成：执行检查点（Checkpoint）机制、执行轨迹回放
- [ ] **可观测性增强（中优先级）**：`events.rs` 添加更细粒度事件（工具调用耗时、LLM 调用耗时等）；实现执行 DAG 可视化输出；添加性能分析（各阶段耗时统计）；支持执行计划预览（dry-run 模式）。
- [ ] **工具执行器增强（中优先级）**：`tool_executor.rs` 增强工具执行结果分析；实现工具调用链追踪；添加工具执行沙箱；支持工具执行超时和取消。
- [ ] **任务模型扩展（低优先级）**：`task.rs` 添加资源需求声明（CPU、内存、IO）；支持优先级动态调整；实现子目标模板系统；添加任务预估耗时。

### 分层 Agent 深度改进（`hierarchy/` 模块，超出原有 TODO 范围）

**职责摘要**：针对规划-执行-反思闭环的深层次能力缺口，涵盖上下文管理、预算控制、用户体验、知识迁移等维度。

- [ ] **上下文窗口管理 — ReAct 循环消息裁剪（高优先级）**：当前 `ReactState.messages` 只增不减，长任务（15 轮迭代）的消息列表可能超出 LLM 上下文窗口。需要实现：
  - 当消息总 token 估算超过阈值时，对早期消息进行摘要压缩
  - 保留策略：系统提示 + 最近 N 轮完整消息 + 早期消息摘要
  - 与现有 `context_window` 模块集成（复用其 token 估算与裁剪逻辑）
  - 压缩时机：每轮迭代结束后检查，超出阈值时触发压缩
- [ ] **执行预算控制 — Token/时间/成本上限（高优先级）**：当前只有 `max_iterations` 作为唯一限制，缺少细粒度预算控制。需要：
  - Token 消耗预算（单次 ReAct 循环和总任务），超限时优雅终止而非截断
  - 执行时间上限（wall-clock timeout），超时返回已完成的中间结果
  - LLM 调用次数限制，防止失控循环
  - 预算耗尽前的降级策略（如简化后续子目标、跳过非关键验证）
- [ ] **渐进式结果汇报 — Operator 中间状态可见（高优先级）**：当前 Operator 只在子目标完成时返回结果，长时间执行的用户看不到任何进度。需要：
  - 在 ReAct 循环中定期发送进度事件（当前迭代/总迭代、工具调用次数）
  - 关键里程碑事件（如编译开始、测试运行、文件生成）
  - 预估剩余时间（基于历史迭代耗时中位数）
  - 通过 SSE 事件流实时推送到 Web UI
- [ ] **跨层级依赖传递增强（中优先级）**：当前 `ArtifactStore.get_dependencies` 只取每个依赖目标的第一个产物，但实际场景中：
  - 编译目标可能产生多个产物（可执行文件 + 配置文件 + 日志）
  - 后续目标可能需要特定类型产物而非全部
  - 需要支持按产物类型（`ArtifactKind`）筛选依赖
  - 需要支持依赖别名（如 `goal_5` 需要 `goal_1` 的 `executable` 产物）
- [ ] **自适应迭代次数（中优先级）**：当前 `max_iterations` 是固定值 15，但简单任务（如检查文件存在）只需 2-3 轮，复杂任务可能需要更多。需要：
  - 基于目标描述和任务类型自动调整迭代上限
  - 任务类型 → 推荐迭代次数映射（检查类:3、编译类:10、调试类:15、分析类:8）
  - 与 `build_execution_guide` 的任务类型识别联动
- [ ] **工具调用结果缓存（中优先级）**：同一工作区中反复执行相同命令（如 `ls`、`which gcc`）浪费 LLM 调用。需要：
  - 对只读工具的调用结果进行短时缓存
  - 缓存 key = (tool_name, arguments, working_dir)
  - 写操作自动失效相关缓存
  - TTL 机制防止过期数据（默认 30s）
- [ ] **执行轨迹回放与调试（中优先级）**：当前缺少执行后的调试能力。需要：
  - 记录每轮 ReAct 的完整状态（消息、工具调用、结果、耗时）到结构化日志
  - 支持从任意检查点重放（用于调试和分析）
  - 生成可读的执行报告（类似 chat export 但结构化，含 token 消耗统计）
- [ ] **子目标优先级调度（中优先级）**：当前同一层级的子目标按顺序执行，缺少优先级感知。需要：
  - 关键路径上的目标优先执行
  - 失败高风险的目标尽早执行（fail fast 原则）
  - 用户关心的目标优先返回结果
  - 与 DAG 拓扑排序结合，在同级目标中按优先级排序
- [ ] **智能重试策略（中优先级）**：当前重试只是简单重新执行相同目标，缺少策略性。需要：
  - 根据错误类型选择重试策略（换工具/换参数/降低目标/换工作目录）
  - 重试间注入额外上下文（"上次失败是因为 X，请尝试 Y"）
  - 指数退避重试（避免频繁重试相同错误）
  - 最大重试次数与错误严重程度挂钩
- [ ] **跨会话知识迁移（低优先级）**：当前 `SessionStateManager` 仅在单会话内有效。需要：
  - 将执行经验持久化到磁盘（哪些命令成功/失败、什么配置有效）
  - 跨会话复用执行策略（如同项目再次编译时跳过已验证步骤）
  - 类似"项目记忆"的概念，按 workspace 路径索引
- [ ] **LLM 调用去重与批处理（低优先级）**：多个并行任务可能同时调用 LLM 做类似分析。可以：
  - 合并相似的 LLM 请求（如多个子目标同时需要检测构建系统）
  - 批量处理工具调用结果分析
  - 减少 API 调用次数和 token 消耗
- [ ] **子目标模板库（低优先级）**：为常见任务模式提供预定义模板：
  - "编译 C 项目" → 标准子目标序列（检查→解压→阅读文档→检查工具→编译→验证）
  - "Python 项目测试" → 检测框架→运行→报告
  - 模板可由用户自定义和扩展（TOML 配置）
  - Manager 分解时优先匹配模板，减少 LLM 调用
- [ ] **执行结果评分机制（低优先级）**：不仅判断成功/失败，还对执行质量打分：
  - 执行效率（是否有多余步骤、重复命令）
  - 资源使用（工具调用次数、token 消耗、总耗时）
  - 结果完整性（是否满足所有隐含需求）
  - 评分作为历史数据供 Router 和 Manager 参考
---

## `llm/` 与 `http_client.rs`（模型请求、重试、流式解析）

**职责摘要**：`ChatRequest` 构造、`complete_chat_retrying`；`api` 中 SSE/JSON 解析；共享 `reqwest::Client` 连接池与超时。

- [ ] **LLM 上游指标（可选）**：按 HTTP 状态/可重试维度暴露计数或对接外部 metrics（当前错误日志已含 `http_status` / `retryable` 字段）。
- [ ] **Token / 费用预估（可选）**：调用前按消息粗算 token，与 `context_window` 预算联动。
- [ ] **非流式与流式一致性测试**：为 `stream: false` 路径补充契约测试（与 P4 同向）。
- [ ] **连接与 TLS 可观测**：可选 debug 级别记录连接复用、首字节延迟（不含敏感 URL 全量）。

---

## `tools/` 与 `tool_registry.rs`（工具实现与分发策略）

**职责摘要**：表驱动 `ToolSpec`、`run_tool`；`tool_registry` 中 Workflow / 阻塞超时 / 搜索等策略。

- [ ] **危险操作分级与确认（续）**：写盘类等工具若需审批或细粒度策略，扩展 [`tool_approval::SensitiveCapability`] 与配置项（当前 `run_command` / `http_fetch` / `http_request` / 工作流审批已统一经 **`tool_approval`**）。
- [ ] **新栈工具按需扩展**：在 `dev_tag` 体系下按需增加其它语言栈标签与最小工具集（保持白名单与路径安全）。Go 已有 `go_build`/`go_test`/`go_vet`/`go_mod_tidy`/`go_fmt_check`/`golangci_lint`；JVM 已有 `maven_*`/`gradle_*`；容器已有 `docker_*`/`podman_images`；Node.js 已有 `npm_install`/`npm_run`/`npx_run`/`tsc_check`。
- [ ] **MCP 扩展（续）**：客户端支持 Streamable HTTP / SSE、多 server 与鉴权；可选将本 agent 以 **HTTP**（streamable）暴露并与 Web 鉴权策略对齐；stdio server 已提供 **`crabmate mcp serve`**（见 **`docs/CLI.md`**）；与 `run_command` / 工作区策略的边界在文档中继续细化。

---

## `sse/`（协议与行分类）

**职责摘要**：`protocol` 编码控制面 JSON；`line` 供 Rust 侧行分类（与 `frontend-leptos/src/sse_dispatch.rs` / `api.rs` 消费语义对齐）。

- [ ] **调试/运维事件**：不脱敏前提下可关闭的 `debug` 类 payload（阶段名、耗时等），仅开发模式启用。

---

## `lib.rs` 路由、`chat_job_queue.rs`、`web/`（HTTP 接入与工作区 API）

**职责摘要**：Axum `Router`、`AppState`；对话队列；`web/workspace`、`web/task` 等。

- [ ] **鉴权与多租户隔离**：API Key / Bearer / 反向代理信任头（与 P0 同向）。
- [ ] **会话与消息 API**：`messages` 或 `conversation_id` + 存储，与 `run_agent_turn` 对齐（与 P1 同向）。
- [ ] **上传配额与清理策略**：`/upload` 大小、类型、保留时间、按用户或 IP 限额。

---

## `config/`（配置加载与 CLI）

**职责摘要**：嵌入/文件 TOML、环境变量、`cli` 参数合并为 `AgentConfig`。

- [ ] **热重载（可选）**：仅对安全子集（工具开关、日志级别等）支持 SIGHUP 或文件 watch。
- [ ] **多 profile**：`dev` / `prod` 预设（工具白名单、审批模式、`http_fetch` 前缀等）。
- [ ] **密钥外置**：与密钥管理（vault、文件权限）集成，文档化兼容路径。

---

## `runtime/`（CLI、会话与导出）

**职责摘要**：`cli`；`workspace_session`、`chat_export`、`message_display`、终端着色与无 SSE 回显。

- [ ] **（未来）全屏终端 UI**：若重新引入 TUI，需恢复消息列表滚动/缓存与 SSE 行分类消费路径。
- [ ] **CLI 历史与脚本**：可选持久化输入历史、从文件批量注入用户消息。
- [ ] **导出格式版本号**：`chat_export` 与前端导出 JSON 带 schema 版本。
- [ ] **无障碍与终端兼容**：弱终端、配色盲、宽字符与剪贴板失败时的降级提示。

---

## `frontend-leptos/`（Web UI，Leptos CSR + WASM）

**职责摘要**：根入口 `frontend-leptos/src/lib.rs`；HTTP/SSE 与本地存储见 `api.rs`、`sse_dispatch.rs`、`storage.rs`、`app_prefs.rs`；主界面 `app/mod.rs`（`chat_column`、`chat_composer`、`message_row` / `message_group_views`、`sidebar_nav`、`side_column`、`workspace_panel`、各 `*_modal` 等）；会话与导出见 `session_ops.rs`、`session_export.rs`、`session_search.rs`；Markdown 与展示见 `markdown.rs`、`assistant_body.rs`、`message_format.rs`；样式与打包见 `frontend-leptos/styles/*.css`、`index.html`、`Trunk.toml`。

- [ ] **浏览器侧多轮状态（续）**：可选**加密**本地缓存；与 P1 会话模型其它项同向。已实现：`ChatSession` 持久化 **`server_conversation_id` / `server_revision`**，流式回合写入后触发 **`GET /conversation/messages`** 水合；标签页内仍用 **`frontend-leptos/src/session_sync.rs`** 的 **`SessionSyncState`**。
- [ ] **聊天列表虚拟化**：极长对话下减少 DOM 与重渲染。
- [ ] **国际化与可访问性**：已集中 **`frontend-leptos/src/i18n/`**（按域拆分子模块，设置内语言切换）与 **`a11y.rs`**（主要模态焦点 + Tab 陷阱、全局 Esc 关闭弹层）；本轮已完成 6 个组件（`settings_modal`、`session_list_modal`、`approval_bar`、`changelist_modal`、`chat_column`、`chat_composer`）的 i18n 审计，确认无硬编码中文；**剩余文件**：`app/mod.rs` 子组件（约 20+ 个）、`timeline.rs`、`find_bar.rs`、各 `*_modal.rs`（`model_switcher_modal`、`session_search_modal` 等）、`workspace_panel.rs`、`session_message_search.rs`；预估工作量：约 5～8 小时（逐文件提取硬编码字符串→`i18n` 补充中英→替换调用）。
- [ ] **语音交互（未来）**：浏览器侧麦克风采集、STT（可对接云端或本地引擎）、TTS 播放；与现有聊天/SSE 流衔接；权限、隐私与错误降级文案；若走后端代理需在 `web/` 增加路由并与鉴权（P0）同盘。

---

## 横切（`types`、`tool_result`、`health`、`redact`、`text_sanitize`）

**职责摘要**：OpenAI 兼容类型；工具结构化结果；`/health`；日志脱敏；展示层清洗。

- [ ] **请求关联 ID**：自 `lib` 入口生成 `request_id`，贯穿日志与 SSE（与 P5「日志关联」同向）。
- [ ] **健康与容量维度扩展**：在现有 **`GET /health`**（含可选 **`llm_models_endpoint`** / **`health_llm_models_probe`**）基础上，可选补充磁盘、队列深度等（实现时可合并为少数指标项）。
- [ ] **敏感信息规则库**：`redact` 与工具输出截断策略集中维护，新增工具时 checklist。

---

*说明：已完成工作不再写入本文件；必要时查 Git 提交记录。调整 `src/` 模块边界时同步更新 `docs/DEVELOPMENT.md`（见 `.cursor/rules/architecture-docs-sync.mdc`）。安全敏感面见 `.cursor/rules/security-sensitive-surface.mdc`。*
