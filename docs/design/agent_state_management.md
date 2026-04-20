# CrabMate Agent 状态管理设计文档

**状态**：设计稿（迭代中）  
**受众**：维护者、架构设计者、核心贡献者  
**关联文档**：`HIERARCHICAL_MULTI_AGENT_ARCHITECTURE.md`, `PLAN_EXECUTE_VERIFY_ARCHITECTURE.md`, `memory_todo.md`

---

## 1. 设计目标

### 1.1 核心问题

当前 CrabMate 的 Agent 执行存在以下状态管理痛点：

| 问题 | 场景 | 影响 |
|------|------|------|
| **步骤间状态隔离** | C++ 编译任务中，编译步骤无法感知源码步骤创建的文件 | 重复创建、编译失败 |
| **产物生命周期模糊** | Artifact 仅存储路径，无内容哈希和版本信息 | 无法判断增量编译 |
| **执行上下文丢失** | 重试或重新规划时，前序步骤的中间结果丢失 | 重复执行、效率低下 |
| **跨会话状态断裂** | 长期任务分多次执行时，状态无法恢复 | 用户体验差 |

### 1.2 设计目标

借鉴主流开源 Agent（OpenAI Swarm、LangGraph、AutoGPT）的实现，构建 CrabMate 的状态管理体系：

1. **短期记忆（Working Memory）**：单轮/单会话内的执行状态
2. **中期记忆（Session State）**：单次分层执行的完整上下文
3. **长期记忆（Persistent Memory）**：跨会话的知识与产物索引
4. **构建状态（Build State）**：编译型任务的专用状态追踪

---

## 2. 主流方案分析

### 2.1 OpenAI Swarm

```python
# Swarm 的核心：context_variables 作为共享状态
class Agent:
    def __init__(self, instructions, functions, context_variables=None):
        self.instructions = instructions
        self.functions = functions
        self.context_variables = context_variables or {}

# 状态传递：Agent 之间通过 context_variables 共享
response = client.run(
    agent=triage_agent,
    messages=messages,
    context_variables={"user_id": "123", "build_dir": "/tmp/build"}
)
```

**借鉴点**：
- 轻量级字典作为状态载体
- Agent 间显式传递上下文
- 函数可读写 context_variables

### 2.2 LangGraph

```python
# LangGraph 的核心：StateGraph + Checkpointer
from langgraph.graph import StateGraph
from langgraph.checkpoint import MemorySaver

# 定义状态 Schema
class AgentState(TypedDict):
    messages: Annotated[list, add_messages]
    build_artifacts: Annotated[list, add_artifacts]
    compilation_state: dict

# 状态持久化
graph = StateGraph(AgentState)
checkpointer = MemorySaver()  # 支持断点续传
```

**借鉴点**：
- 类型化的状态定义（TypedDict）
- 状态注解（Annotated）控制合并策略
- Checkpointer 实现断点续传

### 2.3 AutoGPT

```python
# AutoGPT 的核心：Agent Loop + 外部存储
class Agent:
    def __init__(self):
        self.memory = VectorMemory()      # 长期记忆
        self.workspace = FileWorkspace()  # 文件工作区
        self.state = AgentState()         # 执行状态

    async def run(self, task):
        # 1. 从记忆加载相关上下文
        context = self.memory.query(task)
        # 2. 执行步骤，更新状态
        self.state.update(step_result)
        # 3. 持久化产物到工作区
        self.workspace.save(artifacts)
```

**借鉴点**：
- 分层存储：向量记忆 + 文件工作区 + 执行状态
- 显式的状态生命周期管理
- 产物与工作区解耦

---

## 3. CrabMate 状态管理架构

### 3.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CrabMate Agent State Management                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                    LAYER 1: Working Memory (工作记忆)                    ││
│  │  - 当前会话的消息历史                                                    ││
│  │  - 工具调用上下文                                                        ││
│  │  - 生命周期：单次请求                                                    ││
│  │  - 存储：内存 (Vec<Message>)                                             ││
│  └─────────────────────────────────────────────────────────────────────────┘│
│                                    │                                         │
│                                    ▼                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                    LAYER 2: Session State (会话状态)                     ││
│  │  - 分层执行的完整上下文                                                  ││
│  │  - ArtifactStore：产物存储                                               ││
│  │  - BuildState：构建状态（编译任务）                                      ││
│  │  - ExecutionContext：执行上下文（重试、重规划）                          ││
│  │  - 生命周期：单次分层执行                                                ││
│  │  - 存储：内存 + 可选 Checkpoint                                          ││
│  └─────────────────────────────────────────────────────────────────────────┘│
│                                    │                                         │
│                                    ▼                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                    LAYER 3: Persistent Memory (长期记忆)                 ││
│  │  - 跨会话的向量记忆                                                      ││
│  │  - 工作区文件索引                                                        ││
│  │  - 用户偏好与实体记忆                                                    ││
│  │  - 生命周期：永久                                                        ││
│  │  - 存储：SQLite + 向量数据库                                             ││
│  └─────────────────────────────────────────────────────────────────────────┘│
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 核心数据结构

#### 3.2.1 Working Memory

```rust
// 已存在：src/types.rs
pub struct Message {
    pub role: String,
    pub content: Option<MessageContent>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
}

// 当前会话状态
pub struct WorkingMemory {
    pub messages: Vec<Message>,
    pub conversation_id: String,
    pub workspace_path: Option<PathBuf>,
}
```

#### 3.2.2 Session State（新增）

```rust
// src/agent/state/mod.rs

/// 会话级状态（分层执行共享）
pub struct SessionState {
    /// 会话 ID
    pub session_id: String,
    
    /// 产物存储（已存在，扩展）
    pub artifact_store: ArtifactStore,
    
    /// 构建状态（编译任务专用，新增）
    pub build_state: Option<BuildState>,
    
    /// 执行上下文（新增）
    pub execution_context: ExecutionContext,
    
    /// 共享变量（借鉴 Swarm context_variables）
    pub context_variables: HashMap<String, serde_json::Value>,
}

/// 构建状态（C++/Rust 等编译任务）
pub struct BuildState {
    /// 源码文件 -> 内容哈希
    pub source_files: HashMap<PathBuf, String>,
    
    /// 编译产物
    pub object_files: Vec<PathBuf>,
    pub executables: Vec<PathBuf>,
    pub libraries: Vec<PathBuf>,
    
    /// 编译命令历史
    pub compile_commands: Vec<CompileCommand>,
    
    /// 诊断信息（错误/警告）
    pub diagnostics: Vec<Diagnostic>,
    
    /// 构建目录
    pub build_dir: PathBuf,
}

/// 执行上下文（重试、重规划使用）
pub struct ExecutionContext {
    /// 已执行的步骤记录
    pub step_history: Vec<StepRecord>,
    
    /// 重试计数
    pub retry_counts: HashMap<String, u32>,
    
    /// 检查点（用于断点续传）
    pub checkpoints: Vec<Checkpoint>,
}

pub struct StepRecord {
    pub step_id: String,
    pub goal: SubGoal,
    pub result: TaskResult,
    pub artifacts_produced: Vec<String>,
    pub timestamp: u64,
}

pub struct Checkpoint {
    pub checkpoint_id: String,
    pub state_snapshot: SessionStateSnapshot,
    pub created_at: u64,
}
```

#### 3.2.3 Artifact 扩展（增强）

```rust
// src/agent/hierarchy/task.rs

/// 产物类型（扩展）
pub enum ArtifactKind {
    // 原有类型
    File,
    CommandOutput,
    ApiResponse,
    CodeSnippet,
    Summary,
    Other,
    
    // 新增：构建产物类型
    BuildArtifact(BuildArtifactKind),
}

pub enum BuildArtifactKind {
    SourceFile,      // 源码
    ObjectFile,      // 目标文件 .o
    Executable,      // 可执行文件
    StaticLibrary,   // 静态库 .a
    DynamicLibrary,  // 动态库 .so/.dll
    BuildLog,        // 编译日志
    DependencyFile,  // 依赖文件 .d
}

/// 产物（扩展）
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub kind: ArtifactKind,
    pub path: Option<PathBuf>,
    pub content: Option<String>,
    
    // 新增：版本信息
    pub content_hash: Option<String>,
    pub created_at: u64,
    pub modified_at: u64,
    
    // 新增：元数据
    pub metadata: ArtifactMetadata,
    
    pub produced_by: String,
    pub consumed_by: Vec<String>,
}

pub struct ArtifactMetadata {
    /// 文件大小
    pub size: Option<u64>,
    
    /// MIME 类型
    pub mime_type: Option<String>,
    
    /// 编码
    pub encoding: Option<String>,
    
    /// 自定义属性
    pub custom: serde_json::Value,
}
```

### 3.3 状态流转

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Router    │────▶│   Manager   │────▶│  Operator   │
│  (决策层)   │     │  (规划层)   │     │  (执行层)   │
└─────────────┘     └─────────────┘     └──────┬──────┘
                                                │
                                                ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Checkpoint │◀────│ SessionState│◀────│ ArtifactStore│
│  (持久化)   │     │  (协调层)   │     │  (产物层)   │
└─────────────┘     └─────────────┘     └─────────────┘
```

**状态流转规则**：

1. **Router → Manager**：传递初始 `SessionState`（空状态或恢复状态）
2. **Manager → Operator**：传递 `SessionState` 引用，Operator 只读访问依赖产物
3. **Operator → ArtifactStore**：执行完成后，产物写入 ArtifactStore
4. **Operator → SessionState**：更新 `build_state`、`execution_context`
5. **SessionState → Checkpoint**：关键节点持久化（可选）

---

## 4. 关键机制设计

### 4.1 产物发现与依赖注入

```rust
// src/agent/state/artifact_resolver.rs

/// 产物解析器：自动发现前序步骤产物
pub struct ArtifactResolver<'a> {
    artifact_store: &'a ArtifactStore,
    build_state: Option<&'a BuildState>,
}

impl<'a> ArtifactResolver<'a> {
    /// 根据目标类型查找产物
    pub fn find_by_kind(&self, kind: ArtifactKind) -> Vec<&Artifact> {
        self.artifact_store.all()
            .into_iter()
            .filter(|a| a.kind == kind)
            .collect()
    }
    
    /// 根据路径模式查找产物
    pub fn find_by_pattern(&self, pattern: &str) -> Vec<&Artifact> {
        let glob = glob::Pattern::new(pattern).ok()?;
        self.artifact_store.all()
            .into_iter()
            .filter(|a| {
                a.path.as_ref()
                    .map(|p| glob.matches(p.to_str().unwrap_or("")))
                    .unwrap_or(false)
            })
            .collect()
    }
    
    /// 获取构建产物的完整路径
    pub fn resolve_build_artifact(&self, name: &str) -> Option<PathBuf> {
        self.build_state?
            .executables.iter()
            .chain(&self.build_state?.object_files)
            .find(|p| p.file_name()?.to_str()? == name)
            .cloned()
    }
}

// 在 Operator 中使用
impl OperatorAgent {
    async fn execute(&mut self, goal: &SubGoal, state: &SessionState) -> TaskResult {
        // 1. 解析依赖产物
        let resolver = ArtifactResolver::new(&state.artifact_store, state.build_state.as_ref());
        
        // 2. 根据 goal 的依赖声明自动注入
        for dep_kind in &goal.build_requirements.needs_artifacts {
            let artifacts = resolver.find_by_kind(ArtifactKind::BuildArtifact(dep_kind.clone()));
            // 将产物路径注入工具调用参数
        }
        
        // 3. 执行...
    }
}
```

### 4.2 增量编译支持

```rust
// src/agent/state/build_state.rs

impl BuildState {
    /// 检查是否需要重新编译
    pub fn needs_recompile(&self, source: &Path, new_content: &str) -> bool {
        let new_hash = compute_hash(new_content);
        
        match self.source_files.get(source) {
            None => true,  // 新文件
            Some(old_hash) => {
                if old_hash != &new_hash {
                    true  // 内容变更
                } else {
                    // 检查依赖文件是否变更
                    self.check_dependencies_changed(source)
                }
            }
        }
    }
    
    /// 记录编译结果
    pub fn record_compilation(&mut self, cmd: &CompileCommand) {
        self.compile_commands.push(cmd.clone());
        
        if cmd.success {
            // 更新源文件哈希
            if let Some(content) = std::fs::read_to_string(&cmd.source).ok() {
                self.source_files.insert(
                    cmd.source.clone(),
                    compute_hash(&content)
                );
            }
            
            // 记录产物
            if cmd.output.extension() == Some("o".as_ref()) {
                self.object_files.push(cmd.output.clone());
            }
        }
    }
    
    /// 生成 compile_commands.json（用于 IDE 集成）
    pub fn generate_compile_commands_json(&self) -> String {
        let entries: Vec<_> = self.compile_commands.iter().map(|cmd| {
            json!({
                "directory": self.build_dir.to_str().unwrap_or(""),
                "command": cmd.command,
                "file": cmd.source.to_str().unwrap_or(""),
                "output": cmd.output.to_str().unwrap_or(""),
            })
        }).collect();
        
        serde_json::to_string_pretty(&entries).unwrap_or_default()
    }
}
```

### 4.3 检查点与恢复

```rust
// src/agent/state/checkpoint.rs

/// 检查点管理器
pub struct CheckpointManager {
    storage: Arc<dyn CheckpointStorage>,
}

#[async_trait]
pub trait CheckpointStorage: Send + Sync {
    async fn save(&self, checkpoint: &Checkpoint) -> Result<(), Error>;
    async fn load(&self, checkpoint_id: &str) -> Result<Option<Checkpoint>, Error>;
    async fn list(&self, session_id: &str) -> Result<Vec<Checkpoint>, Error>;
}

/// SQLite 实现
pub struct SqliteCheckpointStorage {
    conn: Connection,
}

#[async_trait]
impl CheckpointStorage for SqliteCheckpointStorage {
    async fn save(&self, checkpoint: &Checkpoint) -> Result<(), Error> {
        let data = bincode::serialize(&checkpoint.state_snapshot)?;
        self.conn.execute(
            "INSERT INTO checkpoints (id, session_id, data, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                checkpoint.checkpoint_id,
                checkpoint.state_snapshot.session_id,
                data,
                checkpoint.created_at
            ],
        )?;
        Ok(())
    }
    // ...
}

// 在 Execution 中使用
impl HierarchicalExecutor {
    async fn execute_with_checkpointing(&self, manager_output: ManagerOutput) -> Result<...> {
        // 每个层级执行前创建检查点
        for (level_idx, level) in levels.iter().enumerate() {
            // 保存检查点
            if self.config.enable_checkpointing {
                let checkpoint = self.state.create_checkpoint();
                self.checkpoint_manager.save(&checkpoint).await?;
            }
            
            // 执行层级
            let result = self.execute_level(level).await?;
            
            // 失败时从检查点恢复
            if result.has_failures() && self.config.enable_retry {
                let last_checkpoint = self.checkpoint_manager
                    .load_last(&self.state.session_id)
                    .await?;
                self.state.restore_from_checkpoint(last_checkpoint);
            }
        }
    }
}
```

---

## 5. 与现有架构集成

### 5.1 集成点

```
┌─────────────────────────────────────────────────────────────────┐
│                     现有架构                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │   Router    │─▶│   Manager   │─▶│   Operator  │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
│        │                │                │                      │
│        ▼                ▼                ▼                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │  ArtifactStore│  │  Execution  │  │ ToolRegistry│             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼ 集成
┌─────────────────────────────────────────────────────────────────┐
│                     新增状态管理                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │ SessionState│  │ BuildState  │  │ Checkpoint  │             │
│  │   (协调层)   │  │  (编译状态)  │  │  (持久化)   │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
│        │                │                │                      │
│        ▼                ▼                ▼                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │ArtifactResolver│ │CompileCommand│ │ SQLiteStorage│            │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 渐进式实现路线

| 阶段 | 目标 | 改动范围 | 优先级 |
|------|------|----------|--------|
| **P0** | Artifact 扩展（哈希、元数据） | `task.rs`, `artifact_store.rs` | 高 |
| **P1** | BuildState 基础实现 | 新增 `build_state.rs` | 高 |
| **P2** | SessionState 整合 | `execution.rs`, `operator.rs` | 高 |
| **P3** | 产物自动发现 | 新增 `artifact_resolver.rs` | 中 |
| **P4** | 检查点机制 | 新增 `checkpoint.rs` | 中 |
| **P5** | 长期记忆集成 | `long_term_memory.rs` | 低 |

### 5.3 配置项

```toml
# config.toml
[agent.state]
# 启用构建状态追踪
enable_build_state = true

# 启用检查点
enable_checkpointing = true
checkpoint_interval = "30s"  # 检查点间隔

# 产物保留策略
artifact_retention = "7d"    # 产物保留时间

# 增量编译
incremental_compilation = true

[agent.state.build]
# 默认构建目录
build_dir = ".crabmate/build"

# 生成 compile_commands.json
generate_compile_commands = true

# 缓存编译产物
cache_object_files = true
```

---

## 6. 使用示例

### 6.1 C++ 编译任务

```rust
// 用户请求："创建一个 C++ Hello World 程序并编译运行"

// Manager 分解为子目标：
let sub_goals = vec![
    SubGoal {
        goal_id: "write_cpp".to_string(),
        description: "创建 main.cpp".to_string(),
        build_requirements: BuildRequirements {
            produces_artifacts: vec![BuildArtifactKind::SourceFile],
            ..Default::default()
        },
        ..Default::default()
    },
    SubGoal {
        goal_id: "compile".to_string(),
        description: "编译 main.cpp".to_string(),
        depends_on: vec!["write_cpp".to_string()],
        build_requirements: BuildRequirements {
            needs_artifacts: vec![BuildArtifactKind::SourceFile],
            produces_artifacts: vec![BuildArtifactKind::ObjectFile],
            incremental_check: true,  // 启用增量编译检查
        },
        ..Default::default()
    },
    SubGoal {
        goal_id: "link".to_string(),
        description: "链接可执行文件".to_string(),
        depends_on: vec!["compile".to_string()],
        build_requirements: BuildRequirements {
            needs_artifacts: vec![BuildArtifactKind::ObjectFile],
            produces_artifacts: vec![BuildArtifactKind::Executable],
        },
        ..Default::default()
    },
    SubGoal {
        goal_id: "run".to_string(),
        description: "运行程序".to_string(),
        depends_on: vec!["link".to_string()],
        build_requirements: BuildRequirements {
            needs_artifacts: vec![BuildArtifactKind::Executable],
        },
        ..Default::default()
    },
];

// 执行过程中：
// 1. write_cpp 步骤创建 main.cpp，ArtifactStore 记录 SourceFile
// 2. compile 步骤通过 ArtifactResolver 发现 main.cpp
//    - BuildState.needs_recompile() 检查是否需要编译
//    - 执行 g++ -c main.cpp -o main.o
//    - BuildState.record_compilation() 记录结果
// 3. link 步骤通过 BuildState.object_files 获取 main.o
//    - 执行 g++ main.o -o hello
//    - BuildState.executables 记录 hello
// 4. run 步骤通过 BuildState 解析 hello 路径并执行
```

---

## 7. 附录

### 7.1 术语对照

| 术语 | 英文 | 含义 |
|------|------|------|
| 产物 | Artifact | 步骤产生的文件或数据 |
| 构建状态 | Build State | 编译任务的专用状态 |
| 检查点 | Checkpoint | 可恢复的执行快照 |
| 增量编译 | Incremental Compilation | 只编译变更的文件 |
| 产物解析 | Artifact Resolution | 自动发现前序产物 |

### 7.2 参考文档

- [OpenAI Swarm](https://github.com/openai/swarm)
- [LangGraph Memory](https://langchain-ai.github.io/langgraph/agents/memory/)
- [AutoGPT Architecture](https://github.com/Significant-Gravitas/AutoGPT)
- [CrabMate Hierarchical Architecture](./HIERARCHICAL_MULTI_AGENT_ARCHITECTURE.md)
