# 分层多 Agent 协作架构：Manager + Operator + 多 Agent 群体

**状态**：设计稿（**未**承诺实现时间表）。**受众**：维护者、产品与协议设计者。  
**语言**：中文。  
**关联文档**：**`docs/PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`**（现有 P-E-V 架构）、**`docs/REACT_ARCHITECTURE.md`**（ReAct 模式）、**`docs/DEVELOPMENT.md`**（模块索引）、**`docs/CONFIGURATION.md`**（配置项）。

---

## 1. 目标与对标

### 1.1 背景

当前项目（CrabMate）的 `agent_turn` 本质上是单一 Agent 循环，对于简单任务足够，但面对复杂任务时：
- 单 Agent 容易陷入"一步错，步步错"的困境
- 缺乏高层规划与底层执行的分离
- 无法有效处理可并行的子任务

### 1.2 目标

引入**分层多 Agent 协作**架构，实现：

| 能力 | 含义 |
|------|------|
| **规划-执行分离** | Manager 负责高层规划，Operator 负责底层执行 |
| **多 Agent 协作** | 主 Agent 分解任务，子 Agent 并行执行 |
| **自主错误恢复** | Operator 失败后 Manager 决定重试或调整 |
| **与现有架构共存** | 可选模式，与结构化规划、ReAct 并存 |
| **优雅降级** | 分层架构失败时自动降级到简单模式 |

### 1.3 对标主流方案

| 方案 | 核心思想 | 适用场景 | 代表项目 |
|------|---------|---------|---------|
| **Manager + Operator** | 高层规划 + 低层执行分离 | 复杂多步骤任务 | Mobile-Agent-E |
| **多 Agent 群体** | 主 Agent 分解 + 子 Agent 并行执行 | 可并行的子任务 | OpenClaw |
| **层次化 Agent** | 多层抽象，每层专注不同粒度 | 超长周期任务 | AutoGPT |

---

## 2. 架构设计

### 2.1 核心架构（修订版）

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           分层多 Agent 协作架构                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                        ROUTER LAYER (路由层)                             │ │
│  │  - 任务复杂度评估                                                        │ │
│  │  - 模式选择（Single/ReAct/Hierarchical/MultiAgent）                      │ │
│  │  - 执行上限设置                                                          │ │
│  │  ⚠️ 注：简单路由逻辑，无需 LLM 调用                                      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                    │                                          │
│                                    ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                        MANAGER LAYER (管理层)                            │ │
│  │                                                                          │ │
│  │    ┌─────────────────────────────────────────────────────────────────┐ │ │
│  │    │                     Manager Agent                                 │ │ │
│  │    │  - 理解高层任务目标                                              │ │ │
│  │    │  - 分解为可执行的子目标 (Sub-goals)                             │ │ │
│  │    │  - 确定执行策略（Sequential/Hybrid/Parallel）                    │ │ │
│  │    │  - 协调子目标执行顺序                                            │ │ │
│  │    │  - 处理子目标级别的失败（重试/跳过/重规划）                      │ │ │
│  │    │  - 汇总结果，生成最终回答                                        │ │ │
│  │    └─────────────────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                    │                                          │
│              ┌─────────────────────┼─────────────────────┐                  │
│              │                     │                     │                  │
│              ▼                     ▼                     ▼                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                        OPERATOR LAYER (操作层)                          │ │
│  │                                                                          │ │
│  │   ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │ │
│  │   │  Operator A │    │  Operator B │    │  Operator C │              │ │
│  │   │  (工具子集A) │    │  (工具子集B) │    │  (工具子集C) │              │ │
│  │   │              │    │              │    │              │              │ │
│  │   │  ReAct 循环  │    │  ReAct 循环  │    │  ReAct 循环  │              │ │
│  │   └──────┬──────┘    └──────┬──────┘    └──────┬──────┘              │ │
│  │          │                   │                   │                      │ │
│  │          └───────────────────┼───────────────────┘                      │ │
│  │                              │                                              │ │
│  │                              ▼                                              │ │
│  │                    ┌─────────────────┐                                     │ │
│  │                    │   Tool Registry │                                     │ │
│  │                    │   (工具执行层)   │                                     │ │
│  │                    └─────────────────┘                                     │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 三层职责分离（修订版）

| 层级 | 组件 | 职责 | LLM 调用 |
|------|------|------|----------|
| **路由层** | Router | 复杂度评估、模式选择、执行上限 | 无（简单 if-else） |
| **管理层** | Manager Agent | 子目标分解、策略确定、层级间协调、失败处理 | 有 |
| **操作层** | Operator Agent(s) | 工具调用级重试、子目标执行（ReAct） | 有 |
| **工具层** | Tool Registry | 实际工具执行 | 无 |

**关键改进**：
- 移除"元认知层"，降级为简单路由逻辑
- 明确 Manager 负责**子目标级**的失败处理
- 明确 Operator 负责**工具调用级**的重试

### 2.3 多 Agent 群体扩展

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           多 Agent 群体协作                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│                        ┌─────────────────┐                                   │
│                        │    Orchestrator   │                                   │
│                        │  (任务分解协调)   │                                   │
│                        └────────┬────────┘                                   │
│                                 │ 分解任务                                    │
│         ┌───────────────────────┼───────────────────────┐                   │
│         │                       │                       │                   │
│         ▼                       ▼                       ▼                   │
│  ┌─────────────┐        ┌─────────────┐        ┌─────────────┐            │
│  │  子 Agent 1  │        │  子 Agent 2  │        │  子 Agent 3  │            │
│  │  (工具子集A)  │        │  (工具子集B)  │        │  (工具子集C)  │            │
│  │              │        │              │        │              │            │
│  │  ReAct 循环  │        │  ReAct 循环   │        │  ReAct 循环   │            │
│  └──────┬──────┘        └──────┬──────┘        └──────┬──────┘            │
│         │                       │                       │                   │
│         └───────────────────────┼───────────────────────┘                   │
│                                 ▼                                           │
│                        ┌─────────────────┐                                  │
│                        │  Artifact Store │                                  │
│                        │  (全局存储)      │                                  │
│                        └─────────────────┘                                  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. 核心数据结构

### 3.1 任务与子目标

```rust
// src/agent/hierarchy/task.rs

/// 任务状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed { reason: String },
    Skipped { reason: String },
}

/// 任务结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub artifacts: Vec<Artifact>,
    pub duration_ms: u64,
}

/// 产物/制品
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub content: Option<String>,
    pub metadata: Value,
    pub produced_by: String,        // 产生该 artifact 的 goal_id
    pub consumed_by: Vec<String>,   // 消费该 artifact 的 goal_ids
}

/// 子目标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubGoal {
    pub goal_id: String,
    pub description: String,
    pub priority: u32,
    /// 依赖的 goal_ids（这些必须先完成）
    pub depends_on: Vec<String>,
    /// 该子目标需要的工具类型
    pub required_capabilities: Vec<Capability>,
    /// 状态
    pub status: TaskStatus,
    /// 结果
    pub result: Option<TaskResult>,
}

/// 能力/技能（替代 OperatorType）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    FileRead,
    FileWrite,
    CommandExecution,
    NetworkRequest,
    WebSearch,
    // 未来可扩展
}
```

### 3.2 Artifact Store（全局产物存储）

```rust
// src/agent/hierarchy/artifact_store.rs

/// 全局产物存储
#[derive(Debug, Clone, Default)]
pub struct ArtifactStore {
    artifacts: HashMap<String, Artifact>,
    /// goal_id → 产生的 artifact_ids
    produced_by: HashMap<String, Vec<String>>,
    /// goal_id → 消费的 artifact_ids
    consumed_by: HashMap<String, Vec<String>>,
}

impl ArtifactStore {
    /// 存储 artifact
    pub fn put(&mut self, artifact: Artifact) {
        let id = artifact.id.clone();
        let produced_by = artifact.produced_by.clone();
        self.artifacts.insert(id.clone(), artifact);
        self.produced_by.entry(produced_by.clone())
            .or_insert_with(Vec::new)
            .push(id.clone());
        self.consumed_by.entry(id)
            .or_insert_with(Vec::new);
    }

    /// 获取 artifact
    pub fn get(&self, id: &str) -> Option<&Artifact> {
        self.artifacts.get(id)
    }

    /// 获取某个 goal 产生的所有 artifacts
    pub fn get_produced_by(&self, goal_id: &str) -> Vec<&Artifact> {
        self.produced_by.get(goal_id)
            .map(|ids| ids.iter().filter_map(|id| self.artifacts.get(id)).collect())
            .unwrap_or_default()
    }

    /// 获取某个 goal 消费的所有 artifacts
    pub fn get_consumed_by(&self, goal_id: &str) -> Vec<&Artifact> {
        self.consumed_by.get(goal_id)
            .map(|ids| ids.iter().filter_map(|id| self.artifacts.get(id)).collect())
            .unwrap_or_default()
    }

    /// 标记某个 goal 消费了某个 artifact
    pub fn mark_consumed(&mut self, goal_id: &str, artifact_id: &str) {
        self.consumed_by.entry(artifact_id.to_string())
            .or_insert_with(Vec::new);
        // 更新 artifact 的 consumed_by
        if let Some(artifact) = self.artifacts.get_mut(artifact_id) {
            if !artifact.consumed_by.contains(&goal_id.to_string()) {
                artifact.consumed_by.push(goal_id.to_string());
            }
        }
    }

    /// 获取某个 goal 的依赖 artifacts
    pub fn get_dependencies(&self, goal_id: &str, depends_on: &[String]) -> Vec<&Artifact> {
        depends_on.iter()
            .filter_map(|dep_id| {
                // 找到依赖 goal 产生的 artifact
                self.produced_by.get(dep_id)
                    .and_then(|ids| ids.first())
                    .and_then(|id| self.artifacts.get(id))
            })
            .collect()
    }
}
```

### 3.3 Manager Agent 消息

```rust
// src/agent/hierarchy/manager_messages.rs

/// Manager 的输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerInput {
    /// 用户高层任务
    pub user_task: String,
    /// 当前全局状态
    pub global_state: GlobalState,
    /// 已完成的子目标结果
    pub completed_results: Vec<TaskResult>,
    /// 失败的子目标
    pub failed_tasks: Vec<FailedTaskInfo>,
}

/// Manager 的输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerOutput {
    /// 分解的子目标列表
    pub sub_goals: Vec<SubGoal>,
    /// 执行策略（顺序/并行/混合）
    pub execution_strategy: ExecutionStrategy,
    /// 给用户的结果摘要
    pub summary: String,
    /// 失败处理决策
    pub failure_decision: FailureDecision,
}

/// 失败处理决策
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureDecision {
    /// 继续执行
    Continue,
    /// 重试失败的子目标
    Retry { goal_id: String, max_retries: u32 },
    /// 跳过失败的子目标
    Skip { goal_id: String, reason: String },
    /// 需要重规划
    Replan { reason: String },
    /// 终止
    Abort { reason: String },
}

/// 执行策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStrategy {
    /// 顺序执行
    Sequential,
    /// 完全并行
    Parallel,
    /// 依赖感知的混合执行
    Hybrid,
}

/// 全局状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalState {
    pub completed_goals: Vec<String>,
    pub in_progress_goals: Vec<String>,
    pub context_summary: String,
    /// 允许的操作能力
    pub available_capabilities: Vec<Capability>,
}
```

### 3.4 Operator Agent 消息

```rust
// src/agent/hierarchy/operator_messages.rs

/// Operator 配置（按能力分配工具）
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// Operator ID
    pub operator_id: String,
    /// 该 Operator 拥有的能力
    pub capabilities: Vec<Capability>,
    /// 该 Operator 可用的工具白名单
    pub allowed_tools: Vec<String>,
    /// 最大 ReAct 迭代次数
    pub max_iterations: usize,
}

/// Operator 的输入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorInput {
    /// 要执行的子目标
    pub sub_goal: SubGoal,
    /// 该 Operator 的配置
    pub config: OperatorConfig,
    /// 相关的上下文（包含依赖的 artifacts）
    pub context: OperatorContext,
    /// 可用的工具描述（基于 allowed_tools 生成）
    pub available_tools: Vec<ToolDescriptor>,
}

/// Operator 的输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorOutput {
    /// 执行结果
    pub result: TaskResult,
    /// 生成的产物
    pub artifacts: Vec<Artifact>,
    /// 执行的 ReAct cycles
    pub react_cycles: Vec<ReActCycle>,
}

/// 操作上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorContext {
    /// 相关的 artifacts（来自依赖子目标）
    pub related_artifacts: Vec<Artifact>,
    /// 之前的操作历史
    pub operation_history: Vec<OperationRecord>,
}
```

### 3.5 工具能力映射

```rust
// src/agent/hierarchy/capability_mapping.rs

/// 能力到工具的映射
pub fn get_tools_for_capabilities(capabilities: &[Capability]) -> Vec<&'static ToolRegistryEntry> {
    let mut tools = Vec::new();
    for cap in capabilities {
        match cap {
            Capability::FileRead => {
                tools.push(tool_registry::get("read_file"));
                tools.push(tool_registry::get("glob"));
                tools.push(tool_registry::get("grep"));
            }
            Capability::FileWrite => {
                tools.push(tool_registry::get("write_file"));
                tools.push(tool_registry::get("create_dir"));
            }
            Capability::CommandExecution => {
                tools.push(tool_registry::get("run_command"));
                tools.push(tool_registry::get("calc"));
            }
            Capability::NetworkRequest => {
                tools.push(tool_registry::get("http_fetch"));
                tools.push(tool_registry::get("http_request"));
            }
            Capability::WebSearch => {
                tools.push(tool_registry::get("web_search"));
            }
        }
    }
    tools
}

/// 根据需要的 capabilities 选择合适的 Operator
pub fn select_operators_for_goal(
    goal: &SubGoal,
    available_operators: &[OperatorConfig],
) -> Option<OperatorConfig> {
    // 选择 capabilities 超集覆盖 goal.required_capabilities 的 Operator
    available_operators.iter()
        .find(|op| {
            goal.required_capabilities.iter().all(|cap| {
                op.capabilities.contains(cap)
            })
        })
        .cloned()
}
```

---

## 4. 执行流程

### 4.1 整体流程（修订版）

```
User Task
    │
    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 1. ROUTER LAYER: 复杂度评估                                            │
│                                                                          │
│    - 分析任务复杂度（基于关键词/预估步数/工具需求）                        │
│    - 选择执行模式                                                       │
│    - 设置执行上限                                                       │
│                                                                          │
│    Output: AgentMode + ExecutionConfig                                  │
└──────────────────────────────────────────────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 2. MANAGER LAYER: 任务分解                                             │
│                                                                          │
│    Manager LLM (使用 Function Calling):                                 │
│    - 理解高层任务目标                                                   │
│    - 分解为 Sub-goals（带 depends_on）                                  │
│    - 确定执行策略                                                       │
│    - 分配 required_capabilities                                        │
│                                                                          │
│    Output: Vec<SubGoal> + ExecutionStrategy                           │
└──────────────────────────────────────────────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 3. OPERATOR LAYER: 子目标执行（阻塞式层级执行）                          │
│                                                                          │
│    按依赖层级分组执行（同一层可并行）：                                    │
│                                                                          │
│    For each level in topological_order:                                 │
│    ┌────────────────────────────────────────────────────────────────┐   │
│    │ 1. 获取依赖的 artifacts（从 ArtifactStore）                      │   │
│    │ 2. 为每个子目标选择合适的 Operator                               │   │
│    │ 3. 并行执行该层所有子目标                                        │   │
│    │ 4. 收集结果，更新 ArtifactStore                                  │   │
│    │ 5. Manager 处理失败（重试/跳过/重规划）                           │   │
│    └────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│    Operator LLM (使用 Function Calling):                                 │
│    - THOUGHT: 理解子目标，决定下一步操作                                 │
│    - ACTION: 选择工具（仅限 allowed_tools）                              │
│    - OBSERVATION: 获取结果                                               │
│    - 重复直到完成                                                       │
└──────────────────────────────────────────────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 4. RESULT AGGREGATION: 结果汇总                                         │
│                                                                          │
│    - 收集所有 Operator 的 artifacts                                     │
│    - 验证最终结果完整性                                                  │
│    - 生成最终回答                                                        │
└──────────────────────────────────────────────────────────────────────────┘
    │
    ▼
Final Response
```

### 4.2 路由层实现

```rust
// src/agent/hierarchy/router.rs

/// 任务复杂度估算
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    Simple,      // 1-2 步
    Medium,      // 3-5 步
    Complex,     // 6-20 步
    VeryComplex, // 20+ 步
}

/// 路由决策
pub struct RouterOutput {
    pub mode: AgentMode,
    pub max_iterations: usize,
    pub max_sub_goals: usize,
}

impl RouterOutput {
    pub fn route(task: &str) -> Self {
        let complexity = estimate_complexity(task);

        match complexity {
            TaskComplexity::Simple => RouterOutput {
                mode: AgentMode::Single,
                max_iterations: 5,
                max_sub_goals: 3,
            },
            TaskComplexity::Medium => RouterOutput {
                mode: AgentMode::ReAct,
                max_iterations: 10,
                max_sub_goals: 5,
            },
            TaskComplexity::Complex => RouterOutput {
                mode: AgentMode::Hierarchical,
                max_iterations: 30,
                max_sub_goals: 20,
            },
            TaskComplexity::VeryComplex => RouterOutput {
                mode: AgentMode::MultiAgent,
                max_iterations: 50,
                max_sub_goals: 50,
            },
        }
    }

    fn estimate_complexity(task: &str) -> TaskComplexity {
        // 基于关键词和预估步数
        let task_lower = task.to_lowercase();

        let mut score = 0;

        // 关键词评估
        if task_lower.contains("分析") || task_lower.contains("比较") {
            score += 2;
        }
        if task_lower.contains("多个") || task_lower.contains("并行") {
            score += 3;
        }
        if task_lower.contains("测试") && task_lower.contains("修改") {
            score += 4;
        }
        if task_lower.contains("重构") || task_lower.contains("迁移") {
            score += 5;
        }

        // 工具需求预估
        let tool_keywords = ["文件", "代码", "测试", "编译", "部署", "API", "数据库"];
        for kw in tool_keywords {
            if task_lower.contains(kw) {
                score += 1;
            }
        }

        match score {
            0..=2 => TaskComplexity::Simple,
            3..=5 => TaskComplexity::Medium,
            6..=10 => TaskComplexity::Complex,
            _ => TaskComplexity::VeryComplex,
        }
    }
}
```

### 4.3 Manager Agent 执行（修订版）

```rust
// src/agent/hierarchy/manager.rs

/// Manager Agent 主循环
pub async fn run_manager(
    params: &mut ManagerParams<'_>,
) -> Result<ManagerOutput, ManagerError> {
    // Phase 1: 分解任务（使用 Function Calling）
    let sub_goals = decompose_task(params).await?;

    // Phase 2: 确定执行策略
    let strategy = determine_execution_strategy(&sub_goals)?;

    // Phase 3: 按层级执行
    let mut artifact_store = ArtifactStore::new();
    let results = execute_by_levels(&sub_goals, &strategy, params, &mut artifact_store).await?;

    // Phase 4: 处理失败
    let failure_decision = handle_failures(&results)?;

    // Phase 5: 汇总结果
    let summary = summarize_results(&results)?;

    Ok(ManagerOutput {
        sub_goals,
        execution_strategy: strategy,
        summary,
        failure_decision,
    })
}

/// 按依赖层级执行（阻塞式）
async fn execute_by_levels(
    sub_goals: &[SubGoal],
    strategy: &ExecutionStrategy,
    params: &ManagerParams<'_>,
    artifact_store: &mut ArtifactStore,
) -> Result<Vec<TaskResult>, ManagerError> {
    // 1. 构建 DAG，获取拓扑层级
    let dag = build_dag(sub_goals)?;
    let levels = dag.topological_levels();

    let mut results = Vec::new();

    for level in levels {
        // 2. 获取该层所有子目标（可并行）
        let level_tasks: Vec<_> = level.iter()
            .filter_map(|id| sub_goals.iter().find(|g| &g.goal_id == id))
            .collect();

        // 3. 并行执行该层
        let level_results = if matches!(strategy, ExecutionStrategy::Parallel) {
            execute_parallel(&level_tasks, params, artifact_store).await?
        } else {
            execute_sequential(&level_tasks, params, artifact_store).await?
        };

        // 4. 更新 artifact store
        for (goal, result) in level.iter().zip(level_results.iter()) {
            if result.status == TaskStatus::Completed {
                // 从 result 中提取 artifacts 并存入
                for artifact in &result.artifacts {
                    artifact_store.put(artifact.clone());
                }
            }
        }

        results.extend(level_results);

        // 5. 检查是否需要终止
        if has_critical_failures(&results) {
            break;
        }
    }

    Ok(results)
}

/// 获取依赖的 artifacts 并构建 OperatorContext
fn build_context_for_goal(
    goal: &SubGoal,
    artifact_store: &ArtifactStore,
) -> OperatorContext {
    let related_artifacts = artifact_store.get_dependencies(&goal.goal_id, &goal.depends_on);

    // 标记消费关系
    for artifact in &related_artifacts {
        artifact_store.mark_consumed(&goal.goal_id, &artifact.id);
    }

    OperatorContext {
        related_artifacts,
        operation_history: Vec::new(),
    }
}
```

### 4.4 Operator Agent 执行（修订版，使用 Function Calling）

```rust
// src/agent/hierarchy/operator.rs

/// Operator Agent 主循环（基于 ReAct + Function Calling）
pub async fn run_operator(
    params: &OperatorParams<'_>,
) -> Result<OperatorOutput, OperatorError> {
    let sub_goal = &params.sub_goal;
    let mut cycles = Vec::new();
    let max_iterations = params.config.max_iterations;

    // 构建 Function Calling 格式的工具列表
    let tools = build_function_tools(&params.input.available_tools);

    loop {
        if cycles.len() >= max_iterations {
            break;
        }

        // ReAct Cycle
        let cycle = think_and_act_function_calling(
            &params.input.sub_goal.description,
            &params.input.context,
            &tools,
            cycles.len() as u64,
        ).await?;

        cycles.push(cycle.clone());

        // 检查是否完成
        if cycle.is_finish() {
            break;
        }
    }

    let result = build_task_result(&cycles, sub_goal)?;
    let artifacts = extract_artifacts(&cycles, &sub_goal.goal_id)?;

    Ok(OperatorOutput {
        result,
        artifacts,
        react_cycles: cycles,
    })
}

/// 构建 Function Calling 格式的工具
fn build_function_tools(available_tools: &[ToolDescriptor]) -> Vec<FunctionDefinition> {
    available_tools.iter().map(|tool| {
        FunctionDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: build_parameters_schema(&tool.parameters),
        }
    }).collect()
}
```

### 4.5 降级机制

```rust
// src/agent/hierarchy/fallback.rs

/// 带降级的执行入口
pub async fn run_with_fallback(
    task: &str,
    params: &mut RunLoopParams<'_>,
) -> Result<ExecutionResult, RunAgentTurnError> {
    // 1. 路由决策
    let router_output = RouterOutput::route(task);

    // 2. 根据模式执行
    let result = match router_output.mode {
        AgentMode::Hierarchical => {
            run_hierarchical_with_fallback(task, params, &router_output).await
        }
        AgentMode::MultiAgent => {
            run_multi_agent_with_fallback(task, params, &router_output).await
        }
        AgentMode::ReAct => {
            run_react_mode(params).await
        }
        AgentMode::Single | _ => {
            run_single_agent_mode(params).await
        }
    };

    // 3. 如果失败且可降级，尝试降级
    if let Err(e) = &result {
        if e.is_recoverable() && router_output.mode != AgentMode::Single {
            warn!("Hierarchical failed: {}, falling back to ReAct", e);
            return run_react_mode(params).await;
        }
    }

    result
}

/// 分层架构执行（带内部降级）
async fn run_hierarchical_with_fallback(
    task: &str,
    params: &mut RunLoopParams<'_>,
    config: &RouterOutput,
) -> Result<ExecutionResult, RunAgentTurnError> {
    // 尝试分层执行
    match run_hierarchical(task, params, config).await {
        Ok(r) => Ok(r),
        Err(e) if is_decomposition_error(&e) => {
            // 分解失败，降级到 ReAct
            warn!("Decomposition failed, falling back to ReAct");
            run_react_mode(params).await
        }
        Err(e) if is_execution_error(&e) => {
            // 执行失败，尝试减少并行度
            warn!("Execution failed, retrying with sequential");
            let mut sequential_config = config.clone();
            run_hierarchical_sequential(task, params, &sequential_config).await
        }
        Err(e) => Err(e),
    }
}
```

---

## 5. 多 Agent 群体协作

### 5.1 Orchestrator 实现

```rust
// src/agent/multi_agent/orchestrator.rs

/// 主 Agent（任务分解协调者）
pub struct Orchestrator {
    pub config: OrchestratorConfig,
    /// 可用的 Operator 配置
    pub operators: Vec<OperatorConfig>,
}

impl Orchestrator {
    /// 分解任务并协调执行
    pub async fn run(
        &self,
        task: &str,
    ) -> Result<MultiAgentResult, OrchestratorError> {
        // 1. 理解任务，确定需要的 capabilities
        let required_caps = self.identify_capabilities(task).await?;

        // 2. 分解任务
        let sub_goals = self.decompose_task(task, &required_caps).await?;

        // 3. 为每个子目标分配 Operator
        let assignments = self.assign_operators(&sub_goals)?;

        // 4. 构建 ArtifactStore 并执行
        let mut artifact_store = ArtifactStore::new();
        let results = self.execute(assignments, &mut artifact_store).await?;

        // 5. 汇总结果
        let aggregated = self.aggregate(results, &artifact_store)?;

        Ok(MultiAgentResult {
            sub_goals,
            results: aggregated,
            artifacts: artifact_store.into_inner(),
        })
    }

    /// 确定任务需要的 capabilities
    async fn identify_capabilities(
        &self,
        task: &str,
    ) -> Result<Vec<Capability>, OrchestratorError> {
        // 使用 Function Calling 判断
        let prompt = format!(
            r#"## 任务
{}

## 可用能力
- FileRead: 文件读取、搜索
- FileWrite: 文件写入、创建
- CommandExecution: 命令执行
- NetworkRequest: HTTP 请求
- WebSearch: 网页搜索

## 输出
确定完成该任务需要哪些能力，用 JSON 数组格式输出。
"#
        );

        let response = llm::chat_simple(&prompt).await?;
        parse_capabilities(&response)
    }
}
```

---

## 6. 与现有架构的集成

### 6.1 新增配置

```toml
# config/hierarchy.toml

[hierarchy]
# 启用分层架构
enabled = false

# 启用多 Agent 协作
multi_agent_enabled = false

# 路由配置
[hierarchy.router]
complexity_threshold_simple = 2
complexity_threshold_medium = 5
complexity_threshold_complex = 20

# Manager LLM 配置
[hierarchy.manager]
model = "gpt-4o"
temperature = 0.7
max_tokens = 4096

# Operator LLM 配置
[hierarchy.operator]
model = "gpt-4o-mini"
temperature = 0.5
max_tokens = 2048
max_iterations = 10

# 执行配置
[hierarchy.execution]
max_parallel_agents = 4
max_sub_goals = 20
max_failures = 3
execution_timeout_seconds = 600
retry_on_failure = true
max_retries = 2

# 降级配置
[hierarchy.fallback]
enable_fallback = true
fallback_to_react = true
fallback_to_single = false
```

### 6.2 环境变量

| 变量 | 说明 | 默认值 |
|------|------|-------|
| `AGENT_HIERARCHY_ENABLED` | 启用分层架构 | `false` |
| `AGENT_MULTI_AGENT_ENABLED` | 启用多 Agent | `false` |
| `AGENT_MAX_PARALLEL_AGENTS` | 最大并行 Agent 数 | `4` |
| `AGENT_MAX_SUB_GOALS` | 最大子目标数 | `20` |
| `AGENT_FALLBACK_ENABLED` | 启用降级 | `true` |

### 6.3 模式选择

```rust
// src/agent/config.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// 单一 Agent（现有默认模式）
    Single,
    /// 分层架构（Manager + Operator）
    Hierarchical,
    /// 多 Agent 群体
    MultiAgent,
    /// 纯 ReAct
    ReAct,
}
```

---

## 7. SSE 事件与可观测性

### 7.1 SSE 事件

```rust
// 路由层事件
SseEventRouterDecision {
    event: "router_decision",
    mode: AgentMode,
    complexity: TaskComplexity,
    max_iterations: usize,
}

// manager 层事件
SseEventManagerDecomposition {
    event: "manager_decomposition",
    sub_goals: Vec<SubGoalWithCapabilities>,
    strategy: ExecutionStrategy,
}
SseEventManagerLevelStart {
    event: "manager_level_start",
    level: u32,
    goal_ids: Vec<String>,
}
SseEventManagerLevelEnd {
    event: "manager_level_end",
    level: u32,
    completed: usize,
    failed: usize,
}
SseEventManagerFailureDecision {
    event: "manager_failure_decision",
    decision: FailureDecision,
}

// operator 层事件
SseEventOperatorStart {
    event: "operator_start",
    goal_id: String,
    operator_id: String,
    capabilities: Vec<Capability>,
}
SseEventOperatorReAct {
    event: "operator_react",
    cycle: u64,
    thought: String,
    tool_name: String,
    tool_args: Value,
}
SseEventOperatorResult {
    event: "operator_result",
    goal_id: String,
    status: TaskStatus,
    artifacts_produced: Vec<String>,
}

// 多 Agent 事件
SseEventAgentSpawn {
    event: "agent_spawn",
    agent_id: String,
    capabilities: Vec<Capability>,
}
SseEventAgentResult {
    event: "agent_result",
    agent_id: String,
    result: TaskResult,
}
SseEventAggregation {
    event: "result_aggregation",
    total: usize,
    successful: usize,
    artifacts: Vec<String>,
}

// 降级事件
SseEventFallback {
    event: "fallback",
    from_mode: AgentMode,
    to_mode: AgentMode,
    reason: String,
}
```

### 7.2 SSE 事件序列示例

```
router_decision {"mode": "hierarchical", "complexity": "complex", "max_iterations": 30}
manager_decomposition {"sub_goals": [{"goal_id": "1", "capabilities": ["FileRead"]}, ...], "strategy": "hybrid"}
manager_level_start {"level": 0, "goal_ids": ["1"]}
operator_start {"goal_id": "1", "operator_id": "op_1", "capabilities": ["FileRead"]}
operator_react {"cycle": 1, "thought": "需要读取文件", "tool_name": "read_file", "tool_args": {"path": "main.rs"}}
operator_result {"goal_id": "1", "status": "completed", "artifacts_produced": ["artifact_1"]}
manager_level_start {"level": 1, "goal_ids": ["2", "3"]}
operator_start {"goal_id": "2", "operator_id": "op_2", "capabilities": ["FileWrite", "CommandExecution"]}
operator_start {"goal_id": "3", "operator_id": "op_3", "capabilities": ["NetworkRequest"]}
operator_result {"goal_id": "2", "status": "completed", "artifacts_produced": ["artifact_2"]}
operator_result {"goal_id": "3", "status": "completed", "artifacts_produced": ["artifact_3"]}
result_aggregation {"total": 3, "successful": 3, "artifacts": ["artifact_1", "artifact_2", "artifact_3"]}
```

---

## 8. 与其他架构的对比

### 8.1 架构演进

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           架构演进                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  单体 Agent                                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  User → [LLM + Tools] → Response                                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                      │
│                                      ▼                                      │
│  ReAct                                                                      │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  User → [Thought → Action → Observation] × N → Response              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                      │
│                                      ▼                                      │
│  分层架构 (Manager + Operator)                                               │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  User → Manager → [Sub-goal 1 → Operator] → [Sub-goal 2 → Operator]  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                      │
│                                      ▼                                      │
│  多 Agent 群体                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  User → Orchestrator → [Agent A] ∥ [Agent B] ∥ [Agent C] → Aggregate │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 选择指南

| 任务复杂度 | 推荐架构 | 理由 |
|-----------|---------|------|
| 简单（1-2 步） | 单体 Agent | 无需额外开销 |
| 中等（3-5 步） | ReAct | 即时推理，适应变化 |
| 复杂（5+ 步，有依赖） | 分层架构 | 规划与执行分离，层级执行 |
| 超复杂（多子任务，可并行） | 多 Agent 群体 | 并行执行，提高效率 |

---

## 9. 实现计划（待定）

### Phase 1: 基础设施
- [ ] 定义核心数据结构（Task, SubGoal, Capability, ArtifactStore）
- [ ] 实现 Router 路由逻辑
- [ ] 实现 Manager Agent 基本结构
- [ ] 实现 Operator Agent 基本结构
- [ ] 添加配置项

### Phase 2: 分层执行
- [ ] 实现 Manager 任务分解（Function Calling）
- [ ] 实现 DAG 构建和拓扑排序
- [ ] 实现 Operator ReAct 循环（Function Calling）
- [ ] 实现阻塞式层级执行
- [ ] 添加 SSE 事件

### Phase 3: 状态管理
- [ ] 实现 ArtifactStore
- [ ] 实现依赖注入机制
- [ ] 实现 Capability → Tools 映射

### Phase 4: 多 Agent 协作
- [ ] 实现 Orchestrator
- [ ] 实现子 Agent 并行执行
- [ ] 实现结果汇总

### Phase 5: 降级与容错
- [ ] 实现降级机制
- [ ] 实现失败重试
- [ ] 实现重规划逻辑
- [ ] 添加监控与日志

### Phase 6: 测试与调优
- [ ] 单元测试
- [ ] 集成测试
- [ ] Prompt 调优
- [ ] 性能优化

---

## 10. 风险与注意事项

### 10.1 风险

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Manager 分解质量差 | 后续执行效率低 | 设置子目标数量上限 + 降级机制 |
| 子 Agent 冲突 | 状态不一致 | ArtifactStore 集中管理 |
| Token 消耗大 | 成本增加 | 设置执行上限和超时 |
| 错误传播 | 整体失败 | 隔离子 Agent 失败 + 降级 |
| Function Calling 解析失败 | 执行中断 | 添加重试和 fallback |

### 10.2 设计权衡

| 权衡 | 选择 | 理由 |
|------|------|------|
| Manager/Operator 是否同模型 | 可配置 | 灵活适应不同需求 |
| 子 Agent 数量 | 最大 4-8 | 避免过多并行开销 |
| 状态共享方式 | ArtifactStore 集中存储 | 简化一致性，避免直接通信 |
| JSON vs Function Calling | Function Calling | 更可靠，避免解析失败 |

---

## 11. 参考资料

- [Mobile-Agent-E: Hierarchical Agent Architecture](https://arxiv.org/abs/2405.19993)
- [OpenClaw: Multi-Agent Architecture](https://github.com/...)
- [AutoGPT: Hierarchical Agent](https://github.com/Significant-Gravitas/AutoGPT)
- [Plan-and-Execute Agents](https://tutorialq.com/ai/single-agent/plan-and-execute-agents)
- [ReAct: Synergizing Reasoning and Acting](https://arxiv.org/abs/2210.03629)
