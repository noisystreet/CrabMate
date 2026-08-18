//! 后台工具任务 HTTP 端点（`GET /tools/jobs/{id}`、`POST /tools/jobs/{id}/cancel`）JSON 形状。
//!
//! 契约见 `docs/design/background_tool_jobs_contract.md` §3。错误码见
//! [`crate::cm_api_contract::error_codes`]（`JOB_NOT_FOUND` / `JOB_EXPIRED` / `JOB_OWNERSHIP_MISMATCH`）。

use schemars::JsonSchema;
use serde::Serialize;

/// `GET /tools/jobs/{tool_job_id}` 200 响应（字段与 `tool_result` 同源，`status` 为任务状态）。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ToolJobStatusResponseBody {
    /// 与路径一致。
    pub tool_job_id: String,
    /// `queued` | `running` | `succeeded` | `failed` | `cancelled` | `timed_out`。
    pub status: String,
    /// 终态才有；`timed_out` / `cancelled` 为 `null`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 终态输出快照（复用 `command_max_output_len` 截断）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// 终态摘要（与 `tool_result.summary` 同源）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 终态失败时的 `tool_result.error_code` 词汇。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 与 `ToolFailureCategory::as_str` 一致。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<String>,
    /// job 结束后的最终值；超时/取消恒为 `false`。
    pub workspace_changed: bool,
    /// 稳定 1（软字段扩展不 bump）。
    pub result_version: u32,
}

/// `POST /tools/jobs/{tool_job_id}/cancel` 200 响应。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ToolJobCancelResponseBody {
    pub tool_job_id: String,
    /// 取消后状态：`cancelled`（幂等重取消亦为 `cancelled`）。
    pub status: String,
}
