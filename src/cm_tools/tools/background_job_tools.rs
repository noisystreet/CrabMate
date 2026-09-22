//! 后台任务查询工具（只读）：`background_job_status` / `background_job_list`。
//!
//! 二者只读进程内后台任务注册表（经 [`ToolJobsToolHost`] 注入），**不发起执行**、不重复实现
//! 审批/白名单，因此与 ADR `docs/design/background_tool_jobs.md` 「不新增发起执行型工具组」的
//! 约束不冲突。
//!
//! 注意：本模块属 `crabmate-tools`，禁止引用 `cm_internal` 内部门面（见 `docs/design/crate_dep_policy.md`）。

use std::path::Path;

use crate::cm_tools::memory_tool_host::ToolJobsToolHost;

use super::parse_args_json;
use super::tool_param_types::{BackgroundJobListArgs, BackgroundJobStatusArgs};

/// `background_job_list` 未显式传入 `limit` 时的默认条数。
const DEFAULT_LIST_LIMIT: u32 = 20;
/// `background_job_list` 的条数硬上限（与参数 schema 的 `schemars(range)` 保持一致）。
const MAX_LIST_LIMIT: u32 = 100;

/// 宿主缺失（并行只读批 / 工作流节点等路径不注入注册表）时的降级文案，避免 panic。
const HOST_UNAVAILABLE: &str =
    "错误：后台任务查询不可用——当前执行路径未注入后台任务注册表。请在主对话回合中直接调用本工具。";

/// 按 `tool_job_id` 查后台任务状态与（终态）输出。
pub fn background_job_status(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    max_output_len: usize,
) -> String {
    let v = match parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let BackgroundJobStatusArgs { tool_job_id } =
        match serde_json::from_value::<BackgroundJobStatusArgs>(v) {
            Ok(a) => a,
            Err(e) => return format!("参数 JSON 与 background_job_status 形状不一致: {e}"),
        };
    let id = tool_job_id.trim();
    if id.is_empty() {
        return "参数错误：`tool_job_id` 不能为空；可先用 background_job_list 查看可用任务 id。"
            .to_string();
    }
    match host {
        Some(h) => h.status(id, max_output_len),
        None => HOST_UNAVAILABLE.to_string(),
    }
}

/// 列出当前工作区的后台任务（最新创建在前）。
pub fn background_job_list(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> String {
    let v = match parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let BackgroundJobListArgs { limit } = match serde_json::from_value::<BackgroundJobListArgs>(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 background_job_list 形状不一致: {e}"),
    };
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT) as usize;
    match host {
        Some(h) => h.list(working_dir, limit, max_output_len),
        None => HOST_UNAVAILABLE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubHost;

    impl ToolJobsToolHost for StubHost {
        fn status(&self, id: &str, _max_output_len: usize) -> String {
            format!("stub status: {id}")
        }

        fn list(&self, workspace: &Path, limit: usize, _max_output_len: usize) -> String {
            format!("stub list: {} limit={limit}", workspace.display())
        }
    }

    #[test]
    fn status_rejects_unknown_field() {
        let out = background_job_status(r#"{"tool_job_id":"j1","extra":1}"#, None, 1024);
        assert!(out.contains("形状不一致"), "{out}");
    }

    #[test]
    fn status_rejects_empty_id() {
        let out = background_job_status(r#"{"tool_job_id":"  "}"#, Some(&StubHost), 1024);
        assert!(out.contains("不能为空"), "{out}");
    }

    #[test]
    fn status_degrades_without_host() {
        let out = background_job_status(r#"{"tool_job_id":"j1"}"#, None, 1024);
        assert!(out.contains("不可用"), "{out}");
    }

    #[test]
    fn status_delegates_to_host_with_trimmed_id() {
        let out = background_job_status(r#"{"tool_job_id":" j1 "}"#, Some(&StubHost), 1024);
        assert_eq!(out, "stub status: j1");
    }

    #[test]
    fn list_delegates_to_host_with_default_limit() {
        let ws = Path::new("/tmp/ws");
        let out = background_job_list("{}", Some(&StubHost), ws, 1024);
        assert_eq!(out, "stub list: /tmp/ws limit=20");
    }

    #[test]
    fn list_clamps_limit_to_max() {
        let ws = Path::new("/tmp/ws");
        let out = background_job_list(r#"{"limit":500}"#, Some(&StubHost), ws, 1024);
        assert_eq!(out, "stub list: /tmp/ws limit=100");
    }

    #[test]
    fn list_rejects_unknown_field() {
        let ws = Path::new("/tmp/ws");
        let out = background_job_list(r#"{"limit":5,"extra":true}"#, Some(&StubHost), ws, 1024);
        assert!(out.contains("形状不一致"), "{out}");
    }

    #[test]
    fn list_degrades_without_host() {
        let ws = Path::new("/tmp/ws");
        let out = background_job_list("{}", None, ws, 1024);
        assert!(out.contains("不可用"), "{out}");
    }
}
