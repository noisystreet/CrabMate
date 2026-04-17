//! 分层多 Agent 协作架构
//!
//! 该模块实现 Manager + Operator 分层架构：
//! - Router: 根据任务复杂度选择执行模式
//! - Manager: 任务分解与协调
//! - Operator: 子目标执行（ReAct 循环）
//! - ArtifactStore: 全局产物存储

pub mod artifact_store;
pub mod events;
pub mod execution;
pub mod manager;
pub mod operator;
pub mod router;
pub mod runner;
pub mod task;

pub use artifact_store::ArtifactStore;
pub use execution::{HierarchicalExecutionResult, HierarchicalExecutor};
pub use manager::{FailureDecision, ManagerAgent, ManagerConfig, ManagerError};
pub use operator::{OperatorAgent, OperatorConfig, OperatorError};
pub use router::{AgentMode, Router, RouterOutput, TaskComplexity};
pub use runner::{HierarchyRunnerParams, HierarchyRunnerResult};
pub use task::{Capability, ExecutionStrategy, SubGoal, TaskResult, TaskStatus};
