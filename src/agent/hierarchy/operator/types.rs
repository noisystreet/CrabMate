//! Operator 对外类型：配置、错误、编译错误分类。

use crate::llm::LlmCompleteError;
use crate::types::Tool;

use super::super::artifact_store::ArtifactStore;
use super::super::build_state::BuildState;
use tokio::sync::mpsc::Sender;

/// 编译错误类型
#[derive(Debug, Clone, PartialEq)]
pub enum CompileErrorType {
    /// OpenMP 并行区域错误
    OpenMPError,
    /// 编译器版本不兼容
    CompilerVersionError,
    /// 缺少依赖库
    MissingDependency,
    /// 配置错误（如错误的 arch 配置）
    ConfigError,
    /// 语法错误
    SyntaxError,
    /// 链接错误
    LinkError,
    /// 工作目录错误
    WorkingDirectoryError,
    /// 其他错误
    Other(String),
}

/// 编译错误信息
#[derive(Debug, Clone)]
pub struct CompileErrorInfo {
    /// 错误类型
    pub error_type: CompileErrorType,
    /// 错误描述
    pub description: String,
    /// 建议的修复方案
    pub suggested_fix: String,
    /// 是否可重试
    pub retryable: bool,
    /// 建议的替代配置（如果有）
    pub alternative_config: Option<String>,
}

/// ReAct 策略与工具可见性（与分层执行注入的运行时句柄分离）。
#[derive(Debug, Clone)]
pub struct OperatorPolicy {
    /// 最大 ReAct 迭代次数
    pub max_iterations: usize,
    /// 可用的工具列表（为空表示使用全部工具）
    pub allowed_tools: Vec<String>,
    /// 工具定义列表（用于 LLM 函数调用）
    pub tools_defs: Vec<Tool>,
    /// 是否启用编译错误自动修复
    pub enable_compile_error_recovery: bool,
    /// 编译错误重试次数
    pub compile_error_max_retries: usize,
    /// 已尝试的配置模板（用于避免重复尝试）
    pub attempted_configs: Vec<String>,
    /// 是否启用动态子目标分解
    pub enable_dynamic_decomposition: bool,
    /// 动态分解复杂度阈值（达到此分数触发分解）
    pub dynamic_decomposition_threshold: u8,
}

impl Default for OperatorPolicy {
    fn default() -> Self {
        Self {
            max_iterations: 15,
            allowed_tools: Vec::new(),
            tools_defs: Vec::new(),
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            attempted_configs: Vec::new(),
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 40,
        }
    }
}

/// SSE、产物存储与构建可变状态（按子目标实例化；与 [`OperatorPolicy`] 正交）。
#[derive(Default, Clone)]
pub struct OperatorRuntimeHandles {
    /// SSE 发送器（用于发送工具调用/结果事件）
    pub sse_out: Option<Sender<String>>,
    /// 产物存储（用于状态共享）
    pub artifact_store: Option<ArtifactStore>,
    /// 构建状态（编译任务使用）
    pub build_state: Option<std::sync::Arc<std::sync::Mutex<BuildState>>>,
}

/// Operator Agent 完整配置：策略 + 本轮运行时句柄。
#[derive(Default, Clone)]
pub struct OperatorConfig {
    pub policy: OperatorPolicy,
    pub runtime: OperatorRuntimeHandles,
}

impl std::fmt::Debug for OperatorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorConfig")
            .field("policy", &self.policy)
            .field("runtime", &"<OperatorRuntimeHandles>")
            .finish()
    }
}

/// Operator Agent 错误
#[derive(Debug)]
pub enum OperatorError {
    MaxIterationsReached,
    ToolNotAllowed(String),
    LlmError(LlmCompleteError),
    ParseError(String),
    ExecutionError(String),
}

impl std::fmt::Display for OperatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperatorError::MaxIterationsReached => write!(f, "Max iterations reached"),
            OperatorError::ToolNotAllowed(t) => write!(f, "Tool not allowed: {}", t),
            OperatorError::LlmError(e) => write!(f, "LLM error: {}", e),
            OperatorError::ParseError(s) => write!(f, "Parse error: {}", s),
            OperatorError::ExecutionError(s) => write!(f, "Execution error: {}", s),
        }
    }
}

impl std::error::Error for OperatorError {}

impl From<LlmCompleteError> for OperatorError {
    fn from(e: LlmCompleteError) -> Self {
        OperatorError::LlmError(e)
    }
}

/// Operator Agent（ReAct 执行体；方法见 `agent_impl` / `react_loop`）。
pub struct OperatorAgent {
    pub(super) config: OperatorConfig,
}
