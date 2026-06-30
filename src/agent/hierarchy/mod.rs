//! 分层多 Agent 协作架构
//!
//! 该模块实现 Manager + Operator 分层架构：
//! - Router: 根据任务复杂度选择执行模式
//! - Manager: 任务分解与协调
//! - Operator: 子目标执行（ReAct 循环）
//! - DynamicDecomposer: 动态子目标分解
//! - ArtifactStore: 全局产物存储
//! - BuildState: 构建状态管理

pub mod artifact_resolver;
pub mod artifact_store;
pub mod build_state;
pub mod dynamic_decomposer;
pub mod events;
pub mod execution;
pub mod execution_error;
mod execution_helpers;
pub mod goal_acceptance;
pub mod goal_verifier;
pub mod manager;
pub mod manager_json_repair;
pub mod operator;
pub mod router;
pub mod runner;
pub mod session_state;
pub mod subgoal_context;
mod subgoal_required_tools;
pub mod task;
pub mod tool_executor;
pub(crate) mod turn_abort;

pub use subgoal_context::{
    PriorSummaryLimits, build_injected_subgoal_user_extra,
    build_prior_subgoals_summary_block_with_limits, collect_artifacts_for_goals,
    normalize_subgoal_io_contracts,
};

pub use artifact_resolver::{ArtifactResolver, prepare_build_env};
pub use artifact_store::ArtifactStore;
pub use build_state::{BuildState, CompileCommand, Diagnostic, DiagnosticSeverity};
pub use dynamic_decomposer::{ComplexityAssessment, DynamicDecomposeError, DynamicDecomposer};
pub use execution::{HierarchicalExecutionResult, HierarchicalExecutor};
pub use execution_error::ExecutionError;
pub use goal_acceptance::effective_goal_acceptance;
pub use goal_verifier::{GoalVerifier, VerificationResult};
pub use manager::{FailureDecision, ManagerAgent, ManagerConfig, ManagerError};
pub use operator::{
    OperatorAgent, OperatorConfig, OperatorError, OperatorPolicy, OperatorRuntimeHandles,
};
pub use router::{
    AgentMode, Router, RouterError, RouterOutput, RoutingStrategy, SmartRouter, TaskComplexity,
};
pub use runner::{HierarchyRunnerParams, HierarchyRunnerResult};
pub use session_state::{
    ArtifactKind, ArtifactStatus, CompletedTask, HierarchicalSessionState, SessionStateManager,
};
pub use task::{
    Artifact, ArtifactKind as TaskArtifactKind, BuildArtifactKind, BuildRequirements, Capability,
    DependencyContractEntry, ExecutionStrategy, GoalAcceptance, SubGoal, TaskResult, TaskStatus,
};
pub use tool_executor::{ExtractedArtifact, ExtractedArtifactKind, ToolExecutionResult};

pub(crate) use subgoal_required_tools::supplement_subgoal_required_tools;
