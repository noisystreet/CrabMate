//! 后台任务查询工具：`background_job`（只读，`status` / `list` / `output`）与 `background_job_cancel`。
//!
//! 只读三动作合并自原 `background_job_status` / `background_job_list` / `background_job_output`
//! ——它们查询同一份进程内注册表，原先各占一份 schema。`cancel` 会改变任务状态，属写副作用
//! 工具，故**保持独立工具名**：合并进来会把上面三个只读查询一并拖出并行只读批 / 只读重试 /
//! `Ask`·`Plan` 门控 / 只读 TTL 缓存（见 `registry_policy::builtin_write_effect_tools`）。
//!
//! 两者只读进程内后台任务注册表（经 [`ToolJobsToolHost`] 注入），**不发起执行**、不重复实现
//! 审批/白名单，因此与 ADR `docs/design/background_tool_jobs.md` 「不新增发起执行型工具组」的
//! 约束不冲突。
//!
//! 注意：本模块属 `cm_tools`，禁止引用 `cm_internal` 内部门面（见 `docs/design/crate_dep_policy.md`）。

use std::path::Path;

use crate::cm_tools::memory_tool_host::ToolJobsToolHost;

use super::parse_args_json;
use super::tool_param_types::{BackgroundJobAction, BackgroundJobArgs, BackgroundJobCancelArgs};

/// `background_job`（`action=list`）未显式传入 `limit` 时的默认条数。
const DEFAULT_LIST_LIMIT: u32 = 20;
/// `background_job`（`action=list`）的条数硬上限（与参数 schema 的 `schemars(range)` 保持一致）。
const MAX_LIST_LIMIT: u32 = 100;

/// 宿主缺失（并行只读批 / 工作流节点等路径不注入注册表）时的降级文案，避免 panic。
const HOST_UNAVAILABLE: &str =
    "错误：后台任务查询不可用——当前执行路径未注入后台任务注册表。请在主对话回合中直接调用本工具。";

/// 后台任务只读入口：按 `action` 分派到 `status` / `list` / `output`。
pub fn background_job(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> String {
    let v = match parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let BackgroundJobArgs {
        action,
        tool_job_id,
        cursor,
        limit,
    } = match serde_json::from_value::<BackgroundJobArgs>(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 background_job 形状不一致: {e}"),
    };
    match action {
        BackgroundJobAction::Status => match require_job_id(tool_job_id.as_deref()) {
            Ok(id) => status_impl(id, host, max_output_len),
            Err(e) => e,
        },
        BackgroundJobAction::List => list_impl(limit, host, working_dir, max_output_len),
        BackgroundJobAction::Output => match require_job_id(tool_job_id.as_deref()) {
            Ok(id) => output_impl(id, cursor, host, max_output_len),
            Err(e) => e,
        },
    }
}

/// `status` / `output` 共用：`tool_job_id` 必填且 trim 后非空。
fn require_job_id(raw: Option<&str>) -> Result<&str, String> {
    let id = raw.unwrap_or("").trim();
    if id.is_empty() {
        return Err(
            "参数错误：`tool_job_id` 不能为空；可先用 background_job（`action=list`）查看可用任务 id。"
                .to_string(),
        );
    }
    Ok(id)
}

fn status_impl(id: &str, host: Option<&dyn ToolJobsToolHost>, max_output_len: usize) -> String {
    match host {
        Some(h) => h.status(id, max_output_len),
        None => HOST_UNAVAILABLE.to_string(),
    }
}

fn list_impl(
    limit: Option<u32>,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> String {
    let limit = limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT) as usize;
    match host {
        Some(h) => h.list(working_dir, limit, max_output_len),
        None => HOST_UNAVAILABLE.to_string(),
    }
}

fn output_impl(
    id: &str,
    cursor: Option<u64>,
    host: Option<&dyn ToolJobsToolHost>,
    max_output_len: usize,
) -> String {
    match host {
        Some(h) => h.output(id, cursor, max_output_len),
        None => HOST_UNAVAILABLE.to_string(),
    }
}

/// 取消后台任务（终止语义，非发起执行）。
pub fn background_job_cancel(args_json: &str, host: Option<&dyn ToolJobsToolHost>) -> String {
    let v = match parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let BackgroundJobCancelArgs { tool_job_id } =
        match serde_json::from_value::<BackgroundJobCancelArgs>(v) {
            Ok(a) => a,
            Err(e) => return format!("参数 JSON 与 background_job_cancel 形状不一致: {e}"),
        };
    match require_job_id(Some(tool_job_id.as_str())) {
        Ok(id) => match host {
            Some(h) => h.cancel(id),
            None => HOST_UNAVAILABLE.to_string(),
        },
        Err(e) => e,
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

        fn output(&self, id: &str, cursor: Option<u64>, _max_output_len: usize) -> String {
            format!("stub output: {id} cursor={cursor:?}")
        }

        fn cancel(&self, id: &str) -> String {
            format!("stub cancel: {id}")
        }
    }

    fn ws() -> &'static Path {
        Path::new("/tmp/ws")
    }

    #[test]
    fn rejects_unknown_field() {
        let out = background_job(
            r#"{"action":"status","tool_job_id":"j1","extra":1}"#,
            None,
            ws(),
            1024,
        );
        assert!(out.contains("形状不一致"), "{out}");
    }

    #[test]
    fn rejects_unknown_action() {
        let out = background_job(r#"{"action":"nope"}"#, None, ws(), 1024);
        assert!(out.contains("形状不一致"), "{out}");
    }

    #[test]
    fn rejects_missing_action() {
        let out = background_job(r#"{"tool_job_id":"j1"}"#, None, ws(), 1024);
        assert!(out.contains("形状不一致"), "{out}");
    }

    #[test]
    fn status_rejects_empty_id() {
        let out = background_job(
            r#"{"action":"status","tool_job_id":"  "}"#,
            Some(&StubHost),
            ws(),
            1024,
        );
        assert!(out.contains("不能为空"), "{out}");
    }

    #[test]
    fn status_rejects_missing_id() {
        let out = background_job(r#"{"action":"status"}"#, Some(&StubHost), ws(), 1024);
        assert!(out.contains("不能为空"), "{out}");
    }

    #[test]
    fn status_degrades_without_host() {
        let out = background_job(
            r#"{"action":"status","tool_job_id":"j1"}"#,
            None,
            ws(),
            1024,
        );
        assert!(out.contains("不可用"), "{out}");
    }

    #[test]
    fn status_delegates_to_host_with_trimmed_id() {
        let out = background_job(
            r#"{"action":"status","tool_job_id":" j1 "}"#,
            Some(&StubHost),
            ws(),
            1024,
        );
        assert_eq!(out, "stub status: j1");
    }

    #[test]
    fn list_delegates_to_host_with_default_limit() {
        let out = background_job(r#"{"action":"list"}"#, Some(&StubHost), ws(), 1024);
        assert_eq!(out, "stub list: /tmp/ws limit=20");
    }

    #[test]
    fn list_clamps_limit_to_max() {
        let out = background_job(
            r#"{"action":"list","limit":500}"#,
            Some(&StubHost),
            ws(),
            1024,
        );
        assert_eq!(out, "stub list: /tmp/ws limit=100");
    }

    #[test]
    fn list_degrades_without_host() {
        let out = background_job(r#"{"action":"list"}"#, None, ws(), 1024);
        assert!(out.contains("不可用"), "{out}");
    }

    #[test]
    fn output_rejects_empty_id() {
        let out = background_job(
            r#"{"action":"output","tool_job_id":"  "}"#,
            Some(&StubHost),
            ws(),
            1024,
        );
        assert!(out.contains("不能为空"), "{out}");
    }

    #[test]
    fn output_degrades_without_host() {
        let out = background_job(
            r#"{"action":"output","tool_job_id":"j1"}"#,
            None,
            ws(),
            1024,
        );
        assert!(out.contains("不可用"), "{out}");
    }

    #[test]
    fn output_delegates_with_cursor() {
        let out = background_job(
            r#"{"action":"output","tool_job_id":" j1 ","cursor":7}"#,
            Some(&StubHost),
            ws(),
            1024,
        );
        assert_eq!(out, "stub output: j1 cursor=Some(7)");
    }

    #[test]
    fn cancel_rejects_empty_id() {
        let out = background_job_cancel(r#"{"tool_job_id":""}"#, Some(&StubHost));
        assert!(out.contains("不能为空"), "{out}");
    }

    #[test]
    fn cancel_degrades_without_host() {
        let out = background_job_cancel(r#"{"tool_job_id":"j1"}"#, None);
        assert!(out.contains("不可用"), "{out}");
    }

    #[test]
    fn cancel_delegates_to_host_with_trimmed_id() {
        let out = background_job_cancel(r#"{"tool_job_id":" j1 "}"#, Some(&StubHost));
        assert_eq!(out, "stub cancel: j1");
    }
}
