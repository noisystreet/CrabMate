//! 分层多 Agent 协作架构
//!
//! 该模块实现 Manager + Operator 分层架构：
//! - Router: 根据任务复杂度选择执行模式
//! - Manager: 任务分解与协调
//! - Operator: 子目标执行（ReAct 循环）
//! - ArtifactStore: 全局产物存储
//! - BuildState: 构建状态管理

pub mod artifact_resolver;
pub mod artifact_store;
pub mod build_state;
pub mod events;
pub mod execution;
pub mod goal_verifier;
pub mod manager;
pub mod operator;
pub mod router;
pub mod runner;
pub mod task;
pub mod tool_executor;

pub use artifact_resolver::{ArtifactResolver, prepare_build_env};
pub use artifact_store::ArtifactStore;
pub use build_state::{BuildState, CompileCommand, Diagnostic, DiagnosticSeverity};
pub use execution::{HierarchicalExecutionResult, HierarchicalExecutor};
pub use goal_verifier::{GoalVerifier, VerificationResult};
pub use manager::{FailureDecision, ManagerAgent, ManagerConfig, ManagerError};
pub use operator::{OperatorAgent, OperatorConfig, OperatorError};
pub use router::{
    AgentMode, Router, RouterError, RouterOutput, RoutingStrategy, SmartRouter, TaskComplexity,
};
pub use runner::{HierarchyRunnerParams, HierarchyRunnerResult};
pub use task::{
    Artifact, ArtifactKind, BuildArtifactKind, BuildRequirements, Capability, ExecutionStrategy,
    GoalAcceptance, SubGoal, TaskResult, TaskStatus,
};
pub use tool_executor::{ExtractedArtifact, ExtractedArtifactKind, ToolExecutionResult};
