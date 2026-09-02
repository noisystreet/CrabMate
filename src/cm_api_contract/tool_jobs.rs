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

/// `GET /tools/jobs/{tool_job_id}/output` 200 响应中的一条输出。
///
/// 契约见 `docs/design/background_tool_jobs_output_streaming_contract.md` §2.2。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ToolJobOutputItem {
    /// 全局单调序号（跨响应不重不漏的游标依据）。
    pub seq: u64,
    /// `stdout` | `stderr`。
    pub stream: String,
    /// lossy UTF-8 文本（非法字节 U+FFFD）。
    pub text: String,
}

/// `GET /tools/jobs/{tool_job_id}/output` 200 响应（增量输出轮询）。
#[derive(Serialize, Clone, JsonSchema)]
pub struct ToolJobOutputResponseBody {
    /// 与路径一致。
    pub tool_job_id: String,
    /// 读取时刻状态：`queued` | `running` | `succeeded` | `failed` | `cancelled` | `timed_out`。
    pub status: String,
    /// 下次请求应携带的 `cursor`（= 最后一条 `item.seq` + 1；无 item 时为本次起点）。
    pub cursor: u64,
    /// `true` = 请求游标早于缓冲最早保留 seq（有输出被环形丢弃，本次从最早可用重放）。
    pub truncated: bool,
    /// `true` = 任务已终态且缓冲（含终态裁剪尾部）已全部返回 → 可停止轮询。
    pub eof: bool,
    /// 自游标起的保留元素（升序，单次响应至多 500 条）。
    pub items: Vec<ToolJobOutputItem>,
}
