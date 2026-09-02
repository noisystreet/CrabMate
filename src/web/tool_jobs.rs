//! `GET /tools/jobs/{id}`、`POST /tools/jobs/{id}/cancel` handler。
//!
//! 契约 `docs/design/background_tool_jobs_contract.md` §3：轮询返回终态/运行态快照；
//! 取消仅 `queued`/`running` 生效、完成态 409 不覆盖、幂等；归属校验（可选
//! `X-Workspace-Root` 头）不符 403。随机 `tool_job_id` 为能力凭证（主防护）。

use std::sync::Arc;
use std::time::SystemTime;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::AppState;
use crate::cm_api_contract::error_codes;
use crate::cm_internal::tool_jobs::registry::{CancelOutcome, GetOutcome, OutputPollOutcome};
use crate::cm_internal::tool_jobs::types::{JobStatus, OutputLogRead};
use crate::web::http_types::chat::ApiError;
use crate::web::http_types::tool_jobs::{
    ToolJobCancelResponseBody, ToolJobOutputItem, ToolJobOutputResponseBody,
    ToolJobStatusResponseBody,
};

/// 可选归属校验头：客户端声明其所在 workspace root；与任务创建时记录比对。
const X_WORKSPACE_ROOT: &str = "x-workspace-root";

type ApiErr = (StatusCode, Json<ApiError>);

fn err(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiErr {
    (status, Json(ApiError::new(code, message)))
}

/// 规范化路径用于归属比对：去首尾空白与末尾分隔符（保留根 `/`）。
fn normalize_workspace_root(p: &str) -> String {
    let mut s = p.trim().to_string();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// 可选归属校验（契约 §3.3）：不带头即放行（知晓 id 即能力凭证）。
fn check_workspace_ownership(workspace: &std::path::Path, headers: &HeaderMap) -> Result<(), ApiErr> {
    let Some(hdr) = headers.get(X_WORKSPACE_ROOT) else {
        return Ok(());
    };
    let Ok(root) = hdr.to_str() else {
        return Err(err(
            StatusCode::FORBIDDEN,
            error_codes::JOB_OWNERSHIP_MISMATCH,
            "无法解析 X-Workspace-Root 请求头",
        ));
    };
    let root = root.trim();
    if root.is_empty() {
        return Ok(());
    }
    let rec_root = normalize_workspace_root(&workspace.to_string_lossy());
    if normalize_workspace_root(root) != rec_root {
        return Err(err(
            StatusCode::FORBIDDEN,
            error_codes::JOB_OWNERSHIP_MISMATCH,
            "请求的 X-Workspace-Root 与该后台任务的归属工作区不符",
        ));
    }
    Ok(())
}

/// `GET /tools/jobs/{tool_job_id}` 输出增量轮询的可选查询参数。
#[derive(serde::Deserialize)]
pub(crate) struct OutputQuery {
    /// 上次响应返回的游标；解析失败/负数按省略处理（从最早可用起）。
    #[serde(default)]
    cursor: Option<String>,
}

/// `GET /tools/jobs/{tool_job_id}`：轮询任务状态与（终态）输出。
pub(crate) async fn tool_job_status_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ToolJobStatusResponseBody>, ApiErr> {
    let registry = &state.aux.tool_job_registry;
    let rec = match registry.get_checked(&id, SystemTime::now()) {
        GetOutcome::Found(rec) => rec,
        GetOutcome::Expired => {
            return Err(err(
                StatusCode::GONE,
                error_codes::JOB_EXPIRED,
                "后台任务已过保留时长（TTL+宽限）并被清理",
            ));
        }
        GetOutcome::NotFound => {
            return Err(err(
                StatusCode::NOT_FOUND,
                error_codes::JOB_NOT_FOUND,
                "后台任务不存在或从未创建",
            ));
        }
    };
    check_workspace_ownership(&rec.workspace, &headers)?;
    let terminal = rec.status.is_terminal();
    let outcome = rec.outcome.as_ref();
    let stdout = outcome.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let stderr = outcome.map(|o| String::from_utf8_lossy(&o.stderr).into_owned());
    let summary = if terminal {
        crate::cm_tools::tools::summarize_tool_call("run_command", &rec.args_json)
    } else {
        None
    };
    Ok(Json(ToolJobStatusResponseBody {
        tool_job_id: rec.id,
        status: rec.status.as_str().to_string(),
        exit_code: outcome.and_then(|o| o.exit_code),
        stdout,
        stderr,
        summary,
        error_code: outcome.and_then(|o| o.error_code.clone()),
        failure_category: outcome.and_then(|o| o.failure_category.clone()),
        workspace_changed: rec.workspace_changed,
        result_version: 1,
    }))
}

/// 组装 `GET /tools/jobs/{id}/output` 的 200 响应体（`stream` 标签化 + 保留元素透传）。
fn output_response_body(
    id: String,
    status: JobStatus,
    log_read: OutputLogRead,
    eof: bool,
) -> ToolJobOutputResponseBody {
    let items: Vec<ToolJobOutputItem> = log_read
        .items
        .iter()
        .map(|e| ToolJobOutputItem {
            seq: e.seq,
            stream: e.stream.as_sse_label().to_string(),
            text: e.text.clone(),
        })
        .collect();
    ToolJobOutputResponseBody {
        tool_job_id: id,
        status: status.as_str().to_string(),
        cursor: log_read.next_cursor,
        truncated: log_read.truncated,
        eof,
        items,
    }
}

/// `GET /tools/jobs/{tool_job_id}/output`：实时输出增量轮询（`tail -f` 语义）。
/// 契约 `docs/design/background_tool_jobs_output_streaming_contract.md` §2/§3。
pub(crate) async fn tool_job_output_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<OutputQuery>,
    headers: HeaderMap,
) -> Result<Json<ToolJobOutputResponseBody>, ApiErr> {
    let registry = &state.aux.tool_job_registry;
    // 游标解析失败/负数/溢出 → 按省略（从最早可用起；宁从头，不错序）。
    let cursor = query
        .cursor
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok());
    let outcome = registry.poll_output(&id, cursor, SystemTime::now());
    let (status, workspace, log_read, eof) = match outcome {
        OutputPollOutcome::Found {
            status,
            workspace,
            log_read,
            eof,
        } => (status, workspace, log_read, eof),
        OutputPollOutcome::Expired => {
            return Err(err(
                StatusCode::GONE,
                error_codes::JOB_EXPIRED,
                "后台任务已过保留时长（TTL+宽限）并被清理",
            ));
        }
        OutputPollOutcome::NotFound => {
            return Err(err(
                StatusCode::NOT_FOUND,
                error_codes::JOB_NOT_FOUND,
                "后台任务不存在或从未创建",
            ));
        }
    };
    check_workspace_ownership(&workspace, &headers)?;
    Ok(Json(output_response_body(id, status, log_read, eof)))
}

/// `POST /tools/jobs/{tool_job_id}/cancel`：取消（`queued` 直接转移；`running` 置取消标记）。
/// 完成态 409 不覆盖；已是 `cancelled` 幂等 200。
pub(crate) async fn tool_job_cancel_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ToolJobCancelResponseBody>, (StatusCode, Json<serde_json::Value>)> {
    let registry = &state.aux.tool_job_registry;
    let rec = match registry.get_checked(&id, SystemTime::now()) {
        GetOutcome::Found(rec) => rec,
        GetOutcome::Expired => {
            return Err((
                StatusCode::GONE,
                Json(serde_json::json!({
                    "code": error_codes::JOB_EXPIRED,
                    "message": "后台任务已过保留时长（TTL+宽限）并被清理",
                })),
            ));
        }
        GetOutcome::NotFound => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "code": error_codes::JOB_NOT_FOUND,
                    "message": "后台任务不存在或从未创建",
                })),
            ));
        }
    };
    if let Err((status, Json(api))) = check_workspace_ownership(&rec.workspace, &headers) {
        return Err((
            status,
            Json(serde_json::json!({ "code": api.code, "message": api.message })),
        ));
    }
    cancel_response(id.clone(), registry.cancel(&id))
}

/// 取消结果 → HTTP 响应（契约 §3.2）：`cancelled` 幂等 200；其它终态 409 不覆盖；过期/不存在 410/404。
fn cancel_response(
    id: String,
    outcome: CancelOutcome,
) -> Result<Json<ToolJobCancelResponseBody>, (StatusCode, Json<serde_json::Value>)> {
    match outcome {
        CancelOutcome::Cancelled => Ok(Json(ToolJobCancelResponseBody {
            tool_job_id: id,
            status: "cancelled".to_string(),
        })),
        // 已是 cancelled：幂等返回 200（契约 §3.2），不视为冲突。
        CancelOutcome::AlreadyFinished(JobStatus::Cancelled) => Ok(Json(ToolJobCancelResponseBody {
            tool_job_id: id,
            status: "cancelled".to_string(),
        })),
        CancelOutcome::AlreadyFinished(status) => Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "status": status.as_str() })),
        )),
        CancelOutcome::Expired => Err((
            StatusCode::GONE,
            Json(serde_json::json!({
                "code": error_codes::JOB_EXPIRED,
                "message": "后台任务已过保留时长（TTL+宽限）并被清理",
            })),
        )),
        CancelOutcome::NotFound => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "code": error_codes::JOB_NOT_FOUND,
                "message": "后台任务不存在或从未创建",
            })),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_internal::tool_jobs::types::JobRecord;
    use std::path::PathBuf;

    fn record(workspace: &str) -> JobRecord {
        JobRecord {
            id: "tooljob_test".to_string(),
            workspace: PathBuf::from(workspace),
            source_turn_job_id: None,
            status: crate::cm_internal::tool_jobs::types::JobStatus::Queued,
            created_at: SystemTime::now(),
            finished_at: None,
            cancel_requested: false,
            cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            spawn: crate::cm_internal::tool_jobs::JobSpawn {
                program: "true".to_string(),
                args: Vec::new(),
                cwd: PathBuf::from("/"),
                extra_env: Vec::new(),
                wall: std::time::Duration::from_secs(10),
                max_output_len: 1024,
            },
            args_json: r#"{"command":"true"}"#.to_string(),
            workspace_changed: false,
            outcome: None,
        }
    }

    fn headers_with(root: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(X_WORKSPACE_ROOT, root.parse().expect("header"));
        h
    }

    #[test]
    fn ownership_passes_without_header_or_matching_root() {
        let rec = record("/home/user/project");
        assert!(check_workspace_ownership(&rec.workspace, &HeaderMap::new()).is_ok());
        assert!(check_workspace_ownership(&rec.workspace, &headers_with("/home/user/project")).is_ok());
        // 尾斜杠与空白容忍。
        assert!(check_workspace_ownership(&rec.workspace, &headers_with(" /home/user/project/ ")).is_ok());
        // 空头等同未提供。
        assert!(check_workspace_ownership(&rec.workspace, &headers_with("  ")).is_ok());
    }

    #[test]
    fn ownership_rejects_mismatched_root() {
        let rec = record("/home/user/project");
        assert!(check_workspace_ownership(&rec.workspace, &headers_with("/home/user/other")).is_err());
        assert!(check_workspace_ownership(&rec.workspace, &headers_with("/home/user/project/sub")).is_err());
    }

    #[test]
    fn output_body_maps_streams_and_serializes_stable_keys() {
        use crate::cm_internal::tool_jobs::types::{OutputEvent, OutputLogRead};
        let log_read = OutputLogRead {
            items: vec![
                OutputEvent {
                    seq: 1,
                    stream: crate::cm_tools::subprocess_session::SessionStream::Stdout,
                    text: "compile ok\n".to_string(),
                },
                OutputEvent {
                    seq: 2,
                    stream: crate::cm_tools::subprocess_session::SessionStream::Stderr,
                    text: "warning: x\n".to_string(),
                },
            ],
            next_cursor: 3,
            truncated: false,
        };
        let body = output_response_body(
            "tooljob_ab".to_string(),
            JobStatus::Running,
            log_read,
            false,
        );
        assert_eq!(body.tool_job_id, "tooljob_ab");
        assert_eq!(body.status, "running");
        assert_eq!(body.cursor, 3);
        assert!(!body.truncated);
        assert!(!body.eof);
        assert_eq!(body.items.len(), 2);
        assert_eq!(body.items[0].stream, "stdout");
        assert_eq!(body.items[1].stream, "stderr");
        // 序列化形状稳定（契约 §2.2 键集合）。
        let v = serde_json::to_value(&body).expect("json");
        let obj = v.as_object().expect("object");
        for key in [
            "tool_job_id",
            "status",
            "cursor",
            "truncated",
            "eof",
            "items",
        ] {
            assert!(obj.contains_key(key), "缺少键 {key}");
        }
        assert_eq!(obj["items"][1]["seq"], 2);
        assert_eq!(obj["items"][1]["stream"], "stderr");
    }

    fn cancel_outcome_body(
        id: &str,
        outcome: CancelOutcome,
    ) -> (Option<ToolJobCancelResponseBody>, StatusCode, serde_json::Value) {
        match cancel_response(id.to_string(), outcome) {
            Ok(Json(body)) => (Some(body), StatusCode::OK, serde_json::Value::Null),
            Err((status, Json(body))) => (None, status, body),
        }
    }

    #[test]
    fn cancel_response_cancelled_is_idempotent_200() {
        // 首次取消 queued/running。
        let (body, status, _) = cancel_outcome_body("tooljob_x", CancelOutcome::Cancelled);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.expect("body").status, "cancelled");
        // 已是 cancelled：仍 200（契约 §3.2 幂等）。
        let (body, status, _) = cancel_outcome_body(
            "tooljob_x",
            CancelOutcome::AlreadyFinished(JobStatus::Cancelled),
        );
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.expect("body").status, "cancelled");
    }

    #[test]
    fn cancel_response_other_terminal_is_409_not_overwrite() {
        for status in [
            JobStatus::Succeeded,
            JobStatus::Failed,
            JobStatus::TimedOut,
        ] {
            let (body, status_code, body_json) =
                cancel_outcome_body("tooljob_x", CancelOutcome::AlreadyFinished(status));
            assert!(body.is_none());
            assert_eq!(status_code, StatusCode::CONFLICT);
            assert_eq!(body_json["status"], status.as_str());
        }
    }

    #[test]
    fn cancel_response_expired_410_and_not_found_404() {
        let (_, status, body) = cancel_outcome_body("tooljob_x", CancelOutcome::Expired);
        assert_eq!(status, StatusCode::GONE);
        assert_eq!(body["code"], error_codes::JOB_EXPIRED);
        let (_, status, body) = cancel_outcome_body("tooljob_x", CancelOutcome::NotFound);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], error_codes::JOB_NOT_FOUND);
    }
}
