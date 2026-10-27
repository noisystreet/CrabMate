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
use crate::cm_tools::tool_result::ToolError;

use super::parse_args_json;
use super::tool_param_types::{BackgroundJobAction, BackgroundJobArgs, BackgroundJobCancelArgs};

/// `background_job`（`action=list`）未显式传入 `limit` 时的默认条数。
const DEFAULT_LIST_LIMIT: u32 = 20;
/// `background_job`（`action=list`）的条数硬上限（与参数 schema 的 `schemars(range)` 保持一致）。
const MAX_LIST_LIMIT: u32 = 100;

/// 宿主缺失（并行只读批 / 工作流节点等路径不注入注册表）时的降级文案，避免 panic。
///
/// 保留「错误：」前缀：String 面薄壳若被既有内部调用方拿到，正文启发式仍判失败。
const HOST_UNAVAILABLE: &str =
    "错误：后台任务查询不可用——当前执行路径未注入后台任务注册表。请在主对话回合中直接调用本工具。";

/// 后台任务只读入口（String 面薄壳）：按 `action` 分派到 `status` / `list` / `output`。
#[allow(clippy::result_large_err)] // 与 crate 内 `*_try` 约定一致（`ToolError` 含 legacy 解析快照）
pub fn background_job(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> String {
    background_job_try(args_json, host, working_dir, max_output_len).unwrap_or_else(|e| e.message)
}

/// Typed 入口：参数与上下文问题一律显式 [`ToolError`]。
///
/// 迁 Typed 前，`参数错误：`tool_job_id` 不能为空…` 以「参数」开头、既不是
/// `starts_with("错误")` 也不含「失败 / 超时」，被 `looks_like_failure` 判成
/// **成功** —— 模型传错或漏传 id 却被告知查询成功（与 `reminder` / `event` 同型）。
#[allow(clippy::result_large_err)]
pub fn background_job_try(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let BackgroundJobArgs {
        action,
        tool_job_id,
        cursor,
        limit,
    } = serde_json::from_value::<BackgroundJobArgs>(v).map_err(|e| {
        ToolError::invalid_args(format!("参数 JSON 与 background_job 形状不一致: {e}"))
    })?;
    match action {
        BackgroundJobAction::Status => {
            status_impl(require_job_id(tool_job_id.as_deref())?, host, max_output_len)
        }
        BackgroundJobAction::List => list_impl(limit, host, working_dir, max_output_len),
        BackgroundJobAction::Output => output_impl(
            require_job_id(tool_job_id.as_deref())?,
            cursor,
            host,
            max_output_len,
        ),
    }
}

/// `status` / `output` 共用：`tool_job_id` 必填且 trim 后非空。
#[allow(clippy::result_large_err)]
fn require_job_id(raw: Option<&str>) -> Result<&str, ToolError> {
    let id = raw.unwrap_or("").trim();
    if id.is_empty() {
        return Err(ToolError::invalid_args(
            "`tool_job_id` 不能为空；可先用 background_job（`action=list`）查看可用任务 id。"
                .to_string(),
        ));
    }
    Ok(id)
}

#[allow(clippy::result_large_err)]
fn status_impl(
    id: &str,
    host: Option<&dyn ToolJobsToolHost>,
    max_output_len: usize,
) -> Result<String, ToolError> {
    match host {
        Some(h) => Ok(h.status(id, max_output_len)),
        None => Err(ToolError::invalid_args(HOST_UNAVAILABLE.to_string())),
    }
}

#[allow(clippy::result_large_err)]
fn list_impl(
    limit: Option<u32>,
    host: Option<&dyn ToolJobsToolHost>,
    working_dir: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let limit = limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT) as usize;
    match host {
        Some(h) => Ok(h.list(working_dir, limit, max_output_len)),
        None => Err(ToolError::invalid_args(HOST_UNAVAILABLE.to_string())),
    }
}

#[allow(clippy::result_large_err)]
fn output_impl(
    id: &str,
    cursor: Option<u64>,
    host: Option<&dyn ToolJobsToolHost>,
    max_output_len: usize,
) -> Result<String, ToolError> {
    match host {
        Some(h) => Ok(h.output(id, cursor, max_output_len)),
        None => Err(ToolError::invalid_args(HOST_UNAVAILABLE.to_string())),
    }
}

/// 取消后台任务（终止语义，非发起执行）—— String 面薄壳。
#[allow(clippy::result_large_err)]
pub fn background_job_cancel(args_json: &str, host: Option<&dyn ToolJobsToolHost>) -> String {
    background_job_cancel_try(args_json, host).unwrap_or_else(|e| e.message)
}

#[allow(clippy::result_large_err)]
pub fn background_job_cancel_try(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
) -> Result<String, ToolError> {
    let v = parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let BackgroundJobCancelArgs { tool_job_id } =
        serde_json::from_value::<BackgroundJobCancelArgs>(v).map_err(|e| {
            ToolError::invalid_args(format!("参数 JSON 与 background_job_cancel 形状不一致: {e}"))
        })?;
    let id = require_job_id(Some(tool_job_id.as_str()))?;
    match host {
        Some(h) => Ok(h.cancel(id)),
        None => Err(ToolError::invalid_args(HOST_UNAVAILABLE.to_string())),
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

    /// 迁 Typed 后参数/上下文错误必须带显式 `invalid_args`，而不是靠正文措辞
    /// （旧文案 `参数错误：…` 不含 `looks_like_failure` 的关键词，会被判成成功）。
    #[test]
    fn typed_errors_carry_invalid_args_code() {
        let e = background_job_try(r#"{"action":"status"}"#, Some(&StubHost), ws(), 1024)
            .expect_err("缺 tool_job_id 必须报错");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("不能为空"), "{}", e.message);

        let e = background_job_try(r#"{"action":"status","tool_job_id":"j1"}"#, None, ws(), 1024)
            .expect_err("无宿主必须报错，而不是回一段「不可用」正文");
        assert_eq!(e.code, "invalid_args");

        let e = background_job_try(r#"{"action":"nope"}"#, Some(&StubHost), ws(), 1024)
            .expect_err("未知 action 必须报错");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("形状不一致"), "{}", e.message);

        let e = background_job_cancel_try(r#"{"tool_job_id":""}"#, Some(&StubHost))
            .expect_err("空 id 必须报错");
        assert_eq!(e.code, "invalid_args");
    }

    #[test]
    fn successful_paths_stay_byte_identical() {
        let out = background_job_try(
            r#"{"action":"status","tool_job_id":" j1 "}"#,
            Some(&StubHost),
            ws(),
            1024,
        )
        .expect("status 应成功");
        assert_eq!(out, "stub status: j1");
    }
}
