//! 后台工具任务（`run_command` 的 `async=true`）：状态/记录类型、进程内注册表、worker 执行。
//!
//! 决策与契约：`docs/design/background_tool_jobs.md`（ADR）、`background_tool_jobs_contract.md`（字段级接口）。
//!
//! - **单副本内存实现**：serve 重启即丢（启动 sweep 为空操作），不承诺崩溃恢复；
//!   孤儿进程无法可靠识别（子进程无标记）。**多副本**需外部代理/持久化，另立项。
//! - 复用 [`crate::cm_tools::subprocess_session`]（进程组 kill、并发排空管道、墙钟、截断缓冲）。
//! - 结果**不自动回填模型**：由调用方轮询带回后续回合。

pub mod registry;
pub mod types;
pub mod worker;

pub use registry::{
    CancelOutcome, GetOutcome, JobRegistryStats, OutputPollOutcome, RegisterError, ToolJobRegistry,
};
pub use types::{
    JobLimits, JobOutcome, JobOutputLog, JobRecord, JobStatus, MAX_ITEMS_PER_RESPONSE,
    MAX_OUTPUT_ITEMS, OutputEvent, OutputLogRead,
};
pub use worker::{
    JobOutputSink, JobSpawn, drain_queued, enqueue_and_launch, launch_job, run_job_blocking,
    spawn_cleanup_task,
};

/// 按当前 `[tool_registry]` 配置构建注册表（web 启动 / 热重载时读取一次；已运行 job 不受影响）。
#[must_use]
pub fn registry_from_config(cfg: &crate::cm_config::AgentConfig) -> std::sync::Arc<ToolJobRegistry> {
    std::sync::Arc::new(ToolJobRegistry::new(JobLimits::from_config(
        &cfg.tool_registry_policy,
    )))
}
