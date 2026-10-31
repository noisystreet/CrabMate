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
use super::tool_param_types::{
    BackgroundJobAction, BackgroundJobArgs, BackgroundJobCancelArgs, BackgroundJobWaitArgs,
    BackgroundJobWaitUntil,
};

/// `background_job`（`action=list`）未显式传入 `limit` 时的默认条数。
const DEFAULT_LIST_LIMIT: u32 = 20;
/// `background_job`（`action=list`）的条数硬上限（与参数 schema 的 `schemars(range)` 保持一致）。
const MAX_LIST_LIMIT: u32 = 100;

/// `background_job_wait` 未传 `timeout_secs` 时的默认等待上限（秒）。
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 60;
/// `background_job_wait` 的等待上限硬顶（与 `run_command` 的 `timeout_secs` 同口径）。
const MAX_WAIT_TIMEOUT_SECS: u64 = 600;
/// `output_matches` 子串的字节上限：匹配用的 `carry` 尾巴与模式长度成正比，必须设界。
const MAX_MATCH_PATTERN_BYTES: usize = 256;
/// 外圈墙钟留白（秒）。`SyncDefault` 工具由 `spawn_blocking` 承载并被外层
/// `tokio::time::timeout(command_timeout_secs)` 包住（见 `tool_registry/execute/dispatch.rs`），
/// 内层必须**早于**它返回，否则模型拿到的是通用「工具执行超时」而不是结构化的等待结果。
const OUTER_WALL_RESERVE_SECS: u64 = 1;

/// 宿主缺失（并行只读批 / 工作流节点等路径不注入注册表）时的降级文案，避免 panic。
///
/// 保留「错误：」前缀：String 面薄壳若被既有内部调用方拿到，正文启发式仍判失败。
const HOST_UNAVAILABLE: &str =
    "错误：后台任务查询不可用——当前执行路径未注入后台任务注册表。请在主对话回合中直接调用本工具。";

/// 后台任务只读入口（String 面薄壳，仅测试使用）：按 `action` 分派到 `status` / `list` / `output`。
#[cfg(test)]
#[allow(clippy::result_large_err)] // 与 crate 内 `*_try` 约定一致（`ToolError` 含正文解析快照 `parsed`）
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

/// 取消后台任务（终止语义，非发起执行）—— String 面薄壳（仅测试使用）。
#[cfg(test)]
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

/// 等待后台任务满足条件（`background_job_wait`）—— **不改变任务状态**。
///
/// `outer_wall_secs` 是该工具在 `spawn_blocking` 外的墙钟上限（`command_timeout_secs`），
/// 用于让内层等待**提前**返回；这样超时是结构化结果，而不是外层 `tokio::time::timeout`
/// 给出的通用「工具执行超时」。
#[allow(clippy::result_large_err)]
pub fn background_job_wait_try(
    args_json: &str,
    host: Option<&dyn ToolJobsToolHost>,
    max_output_len: usize,
    outer_wall_secs: u64,
) -> Result<String, ToolError> {
    let v = parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let BackgroundJobWaitArgs {
        tool_job_id,
        until,
        output_matches,
        timeout_secs,
    } = serde_json::from_value::<BackgroundJobWaitArgs>(v).map_err(|e| {
        ToolError::invalid_args(format!("参数 JSON 与 background_job_wait 形状不一致: {e}"))
    })?;
    let id = require_job_id(Some(tool_job_id.as_str()))?;
    // `output_matches` 给了却没写 `until` 时按调用者显然的意图**推断**为 `output_matches`：
    // 静默落回「等 exit」会让「等 dev server 打印 listening on」变成吃满超时（#1194 review）。
    let until = until.unwrap_or_else(|| {
        if output_matches
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
        {
            BackgroundJobWaitUntil::OutputMatches
        } else {
            BackgroundJobWaitUntil::Exit
        }
    });
    let pattern = match until {
        BackgroundJobWaitUntil::Exit => None,
        BackgroundJobWaitUntil::OutputMatches => {
            let pat = output_matches.as_deref().unwrap_or("").trim();
            if pat.is_empty() {
                return Err(ToolError::invalid_args(
                    "`until=output_matches` 需要非空的 `output_matches`（要查找的子串）。".to_string(),
                ));
            }
            if pat.len() > MAX_MATCH_PATTERN_BYTES {
                return Err(ToolError::invalid_args(format!(
                    "`output_matches` 过长（{} 字节），上限 {MAX_MATCH_PATTERN_BYTES} 字节。",
                    pat.len()
                )));
            }
            Some(pat)
        }
    };
    // 双重钳制：参数口径 1–600，再为外圈墙钟让出留白（`outer_wall_secs` 未知时按硬顶）。
    let ceiling = MAX_WAIT_TIMEOUT_SECS.min(
        outer_wall_secs
            .saturating_sub(OUTER_WALL_RESERVE_SECS)
            .max(1),
    );
    let wait_secs = timeout_secs
        .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECS)
        .clamp(1, ceiling);
    match host {
        Some(h) => Ok(h.wait_until(id, pattern, wait_secs, max_output_len)),
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

        fn wait_until(
            &self,
            id: &str,
            pattern: Option<&str>,
            timeout_secs: u64,
            _max_output_len: usize,
        ) -> String {
            format!("stub wait: {id} pattern={pattern:?} timeout={timeout_secs}")
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

    // ── `background_job_wait` ─────────────────────────────────

    #[test]
    fn wait_defaults_to_exit_and_default_timeout() {
        let out = background_job_wait_try(
            r#"{"tool_job_id":" j1 "}"#,
            Some(&StubHost),
            1024,
            600,
        )
        .expect("wait 应成功");
        assert_eq!(
            out,
            format!("stub wait: j1 pattern=None timeout={DEFAULT_WAIT_TIMEOUT_SECS}")
        );
    }

    #[test]
    fn wait_output_matches_passes_trimmed_pattern() {
        let out = background_job_wait_try(
            r#"{"tool_job_id":"j1","until":"output_matches","output_matches":"  listening on "}"#,
            Some(&StubHost),
            1024,
            600,
        )
        .expect("wait 应成功");
        assert!(out.contains(r#"pattern=Some("listening on")"#), "{out}");
    }

    #[test]
    fn wait_output_matches_is_inferred_when_until_omitted() {
        // 只给 `output_matches`、不写 `until`：必须按 `output_matches` 处理，
        // 而不是静默落回「等 exit」（否则永不退出的 dev server 会吃满超时）。
        let out = background_job_wait_try(
            r#"{"tool_job_id":"j1","output_matches":"listening on"}"#,
            Some(&StubHost),
            1024,
            600,
        )
        .expect("wait 应成功");
        assert!(out.contains(r#"pattern=Some("listening on")"#), "{out}");
    }

    #[test]
    fn wait_blank_output_matches_falls_back_to_exit() {
        // 空白 pattern 不构成「想等 match」的意图，仍按默认 `exit`（不报错地空等 match）。
        let out =
            background_job_wait_try(r#"{"tool_job_id":"j1","output_matches":"  "}"#, Some(&StubHost), 1024, 600)
                .expect("wait 应成功");
        assert!(out.contains("pattern=None"), "{out}");
    }

    #[test]
    fn wait_output_matches_requires_pattern() {
        let e = background_job_wait_try(
            r#"{"tool_job_id":"j1","until":"output_matches"}"#,
            Some(&StubHost),
            1024,
            600,
        )
        .expect_err("缺 pattern 必须报错");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("output_matches"), "{}", e.message);
    }

    #[test]
    fn wait_rejects_overlong_pattern() {
        let long = "a".repeat(MAX_MATCH_PATTERN_BYTES + 1);
        let args = format!(
            r#"{{"tool_job_id":"j1","until":"output_matches","output_matches":"{long}"}}"#
        );
        let e = background_job_wait_try(&args, Some(&StubHost), 1024, 600)
            .expect_err("过长 pattern 必须报错");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("过长"), "{}", e.message);
    }

    #[test]
    fn wait_clamps_timeout_to_schema_ceiling() {
        // 外圈未知（`u64::MAX`）时只剩参数口径：请求 10000 秒 → 600。
        let out = background_job_wait_try(
            r#"{"tool_job_id":"j1","timeout_secs":10000}"#,
            Some(&StubHost),
            1024,
            u64::MAX,
        )
        .expect("wait 应成功");
        assert!(out.ends_with("timeout=600"), "{out}");
    }

    #[test]
    fn wait_leaves_outer_wall_reserve() {
        // 外圈墙钟 10s → 内层最多 9s，保证早于 `tokio::time::timeout` 返回。
        let out = background_job_wait_try(
            r#"{"tool_job_id":"j1","timeout_secs":600}"#,
            Some(&StubHost),
            1024,
            10,
        )
        .expect("wait 应成功");
        assert!(out.ends_with("timeout=9"), "{out}");
    }

    #[test]
    fn wait_rejects_empty_id_and_unknown_field() {
        let e = background_job_wait_try(r#"{"tool_job_id":"  "}"#, Some(&StubHost), 1024, 600)
            .expect_err("空 id 必须报错");
        assert_eq!(e.code, "invalid_args");

        let e = background_job_wait_try(
            r#"{"tool_job_id":"j1","extra":1}"#,
            Some(&StubHost),
            1024,
            600,
        )
        .expect_err("未知字段必须报错");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("形状不一致"), "{}", e.message);
    }

    #[test]
    fn wait_degrades_without_host() {
        let e = background_job_wait_try(r#"{"tool_job_id":"j1"}"#, None, 1024, 600)
            .expect_err("无宿主必须报错");
        assert_eq!(e.code, "invalid_args");
    }
}
