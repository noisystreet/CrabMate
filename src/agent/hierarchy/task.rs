//! 分层多 Agent 架构的核心数据结构

use serde::{Deserialize, Serialize};

/// 任务状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed {
        reason: String,
    },
    Skipped {
        reason: String,
    },
    /// 需要动态分解为子目标
    NeedsDecomposition {
        reason: String,
        suggested_subgoals: usize,
    },
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
    /// 本子目标内按时间顺序记录的工具名（如 `run_executable`、`run_command`），供分层 GoalVerifier 验收
    #[serde(default)]
    pub tools_invoked: Vec<String>,
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
    /// 构建产物（编译任务专用）
    BuildArtifact(BuildArtifactKind),
}

/// 构建产物类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildArtifactKind {
    /// 源码文件
    SourceFile,
    /// 目标文件（.o/.obj）
    ObjectFile,
    /// 可执行文件
    Executable,
    /// 静态库（.a/.lib）
    StaticLibrary,
    /// 动态库（.so/.dll/.dylib）
    DynamicLibrary,
    /// 构建日志
    BuildLog,
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

/// 子目标类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GoalType {
    /// 分析/收集信息（失败后不重试修复，直接跳过）
    Analyze,
    /// 修复/执行（失败后尝试修复）
    #[default]
    Fix,
}

/// 构建需求声明
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildRequirements {
    /// 需要的构建产物类型
    #[serde(default)]
    pub needs_artifacts: Vec<BuildArtifactKind>,
    /// 产生的构建产物类型
    #[serde(default)]
    pub produces_artifacts: Vec<BuildArtifactKind>,
}

/// 子目标验收条件
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalAcceptance {
    /// 期望文件存在（相对路径列表）
    #[serde(default)]
    pub expect_file_exists: Vec<String>,
    /// 期望命令成功执行（验证命令）
    #[serde(default)]
    pub expect_command_success: Option<String>,
    /// 期望输出包含特定字符串
    #[serde(default)]
    pub expect_output_contains: Vec<String>,
    /// 期望退出码（默认 0）
    #[serde(default)]
    pub expect_exit_code: Option<i32>,
}

impl GoalAcceptance {
    /// 创建文件存在验证
    pub fn file_exists(path: &str) -> Self {
        Self {
            expect_file_exists: vec![path.to_string()],
            ..Default::default()
        }
    }

    /// 创建命令成功验证
    pub fn command_success(cmd: &str) -> Self {
        Self {
            expect_command_success: Some(cmd.to_string()),
            expect_exit_code: Some(0),
            ..Default::default()
        }
    }

    /// 创建输出包含验证
    pub fn output_contains(text: &str) -> Self {
        Self {
            expect_output_contains: vec![text.to_string()],
            ..Default::default()
        }
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
    /// 该子目标需要的工具名称列表
    #[serde(default)]
    pub required_tools: Vec<String>,
    /// 子目标类型：analyze（分析/收集） 或 fix（修复/执行）
    #[serde(default)]
    pub goal_type: GoalType,
    /// 构建需求（编译任务使用）
    #[serde(default)]
    pub build_requirements: BuildRequirements,
    /// 验收条件（验证子目标是否达成）
    #[serde(default)]
    pub acceptance: Option<GoalAcceptance>,
    /// 最大重试次数（验证失败时）
    #[serde(default)]
    pub max_retries: Option<usize>,
}

impl SubGoal {
    pub fn new(goal_id: &str, description: &str) -> Self {
        Self {
            goal_id: goal_id.to_string(),
            description: description.to_string(),
            priority: 0,
            depends_on: Vec::new(),
            required_tools: Vec::new(),
            goal_type: GoalType::default(),
            build_requirements: BuildRequirements::default(),
            acceptance: None,
            max_retries: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.required_tools = tools;
        self
    }

    pub fn with_depends_on(mut self, deps: Vec<String>) -> Self {
        self.depends_on = deps;
        self
    }

    pub fn with_goal_type(mut self, goal_type: GoalType) -> Self {
        self.goal_type = goal_type;
        self
    }

    pub fn with_acceptance(mut self, acceptance: GoalAcceptance) -> Self {
        self.acceptance = Some(acceptance);
        self
    }

    pub fn with_max_retries(mut self, retries: usize) -> Self {
        self.max_retries = Some(retries);
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

impl ExecutionStrategy {
    /// 转换为字符串
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
            Self::Hybrid => "hybrid",
        }
    }
}
