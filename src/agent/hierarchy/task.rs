//! 分层多 Agent 架构的核心数据结构

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub duration_ms: u64,
}

/// 产物类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    File,
    CommandOutput,
    ApiResponse,
    CodeSnippet,
    Summary,
    Other,
}

/// 产物/制品
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// 产生该 artifact 的 goal_id
    pub produced_by: String,
    /// 消费该 artifact 的 goal_ids
    #[serde(default)]
    pub consumed_by: Vec<String>,
}

impl Artifact {
    pub fn new(id: &str, name: &str, kind: ArtifactKind, produced_by: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            path: None,
            content: None,
            metadata: serde_json::Value::Null,
            produced_by: produced_by.to_string(),
            consumed_by: Vec::new(),
        }
    }

    pub fn with_content(mut self, content: &str) -> Self {
        self.content = Some(content.to_string());
        self
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }
}

/// 能力/技能（用于 Operator 工具分配）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Capability {
    #[default]
    FileRead,
    FileWrite,
    CommandExecution,
    NetworkRequest,
    WebSearch,
}

impl Capability {
    pub fn all() -> Vec<Capability> {
        vec![
            Capability::FileRead,
            Capability::FileWrite,
            Capability::CommandExecution,
            Capability::NetworkRequest,
            Capability::WebSearch,
        ]
    }
}

/// 子目标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubGoal {
    pub goal_id: String,
    pub description: String,
    #[serde(default)]
    pub priority: u32,
    /// 依赖的 goal_ids（这些必须先完成）
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 该子目标需要的工具类型
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
}

impl SubGoal {
    pub fn new(goal_id: &str, description: &str) -> Self {
        Self {
            goal_id: goal_id.to_string(),
            description: description.to_string(),
            priority: 0,
            depends_on: Vec::new(),
            required_capabilities: Vec::new(),
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.required_capabilities = caps;
        self
    }

    pub fn with_depends_on(mut self, deps: Vec<String>) -> Self {
        self.depends_on = deps;
        self
    }
}

/// 执行策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionStrategy {
    /// 顺序执行
    Sequential,
    /// 完全并行
    Parallel,
    /// 依赖感知的混合执行
    #[default]
    Hybrid,
}
