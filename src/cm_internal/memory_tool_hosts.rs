//! `crabmate-internal` 对 [`crate::cm_tools::memory_tool_host`] 的实现。

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use crate::cm_config::AgentConfig;
use crate::cm_tools::memory_tool_host::{
    CodebaseSemanticToolHost, LongTermMemoryToolHost, ToolJobsToolHost,
};

use crate::cm_internal::long_term_memory_tools::{
    LongTermMemoryHostInner, LongTermMemoryToolState, long_term_forget, long_term_memory_list,
    long_term_remember, summarize_experience,
};
use crate::cm_internal::memory::codebase_semantic_index::{CodebaseSemanticToolParams, run_tool};
use crate::cm_internal::tool_jobs::GetOutcome;

pub struct CodebaseSemanticHost {
    pub params: CodebaseSemanticToolParams,
}

impl CodebaseSemanticHost {
    pub fn from_config(cfg: &AgentConfig) -> Self {
        Self {
            params: CodebaseSemanticToolParams::from_agent_config(cfg),
        }
    }

    pub fn from_params(params: CodebaseSemanticToolParams) -> Self {
        Self { params }
    }
}

impl CodebaseSemanticToolHost for CodebaseSemanticHost {
    fn run_search(&self, args_json: &str, working_dir: &Path, max_output_len: usize) -> String {
        run_tool(args_json, working_dir, &self.params, max_output_len)
    }
}

pub struct LongTermMemoryHost {
    inner: LongTermMemoryHostInner,
}

impl LongTermMemoryHost {
    pub fn new(
        rt: Arc<crate::cm_internal::memory::long_term_memory::LongTermMemoryRuntime>,
        scope_id: String,
    ) -> Self {
        Self {
            inner: LongTermMemoryHostInner { rt, scope_id },
        }
    }
}

impl LongTermMemoryToolHost for LongTermMemoryHost {
    fn dispatch(&self, tool_name: &str, args_json: &str, cfg: &AgentConfig) -> String {
        let st = LongTermMemoryToolState {
            cfg,
            rt: &self.inner.rt,
            scope: self.inner.scope_id.as_str(),
        };
        match tool_name {
            "long_term_remember" => long_term_remember(args_json, &st),
            "long_term_forget" => long_term_forget(args_json, &st),
            "long_term_memory_list" => long_term_memory_list(args_json, &st),
            "summarize_experience" => summarize_experience(args_json, &st),
            _ => format!("错误：未知长期记忆工具 `{tool_name}`"),
        }
    }
}

/// 工具 dispatch 路径在栈上构造记忆宿主，供 [`crate::cm_tools::tools::ToolContext`] 借用。
pub struct DispatchMemoryHosts {
    pub codebase: CodebaseSemanticHost,
    pub long_term: Option<LongTermMemoryHost>,
}

impl DispatchMemoryHosts {
    pub fn from_dispatch_inputs(
        cfg: &AgentConfig,
        ltm: Option<Arc<crate::cm_internal::memory::long_term_memory::LongTermMemoryRuntime>>,
        scope_id: Option<&str>,
    ) -> Self {
        let codebase = CodebaseSemanticHost::from_config(cfg);
        let (mem_rt, mem_scope) =
            crate::cm_internal::memory::long_term_memory::tool_context_memory_extras(cfg, ltm, scope_id);
        let long_term = mem_rt
            .zip(mem_scope)
            .map(|(rt, scope)| LongTermMemoryHost::new(rt, scope));
        Self {
            codebase,
            long_term,
        }
    }

    pub fn codebase_ref(&self) -> &dyn CodebaseSemanticToolHost {
        &self.codebase
    }

    pub fn long_term_ref(&self) -> Option<&dyn LongTermMemoryToolHost> {
        self.long_term
            .as_ref()
            .map(|h| h as &dyn LongTermMemoryToolHost)
    }
}

/// `background_job_status` / `background_job_list` 的宿主：只读进程内 [`ToolJobRegistry`]。
///
/// 仅暴露查询能力（不含发起执行/取消），与 ADR `docs/design/background_tool_jobs.md`
/// 「不新增发起执行型工具组」的约束一致。
pub struct ToolJobsHost {
    registry: Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
}

impl ToolJobsHost {
    pub fn new(registry: Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>) -> Self {
        Self { registry }
    }
}

/// 按字符边界截断，避免切进多字节序列。
fn truncate_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…（输出已截断，仅显示前 {end} 字节）", &s[..end])
}

fn append_stream(out: &mut String, label: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    if text.trim().is_empty() {
        return;
    }
    out.push_str(label);
    out.push('\n');
    out.push_str(&text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
}

impl ToolJobsToolHost for ToolJobsHost {
    fn status(&self, id: &str, max_output_len: usize) -> String {
        let rec = match self.registry.get_checked(id, SystemTime::now()) {
            GetOutcome::Found(rec) => rec,
            GetOutcome::Expired => {
                return format!("错误：后台任务 `{id}` 已过保留时长（TTL+宽限）并被清理。");
            }
            GetOutcome::NotFound => {
                return format!("错误：后台任务 `{id}` 不存在或从未创建。");
            }
        };
        let mut out = String::new();
        out.push_str(&format!("后台任务 {}\n", rec.id));
        out.push_str(&format!("状态: {}\n", rec.status.as_str()));
        out.push_str(&format!("工作区: {}\n", rec.workspace.display()));
        out.push_str(&format!(
            "工作区已变更: {}\n",
            if rec.workspace_changed { "是" } else { "否" }
        ));
        if let Some(summary) =
            crate::cm_tools::tools::summarize_tool_call("run_command", &rec.args_json)
        {
            out.push_str(&format!("命令: {summary}\n"));
        }
        match rec.outcome.as_ref() {
            Some(o) => {
                if let Some(code) = o.exit_code {
                    out.push_str(&format!("退出码: {code}\n"));
                }
                if let Some(code) = o.error_code.as_deref() {
                    out.push_str(&format!("错误码: {code}\n"));
                }
                if let Some(cat) = o.failure_category.as_deref() {
                    out.push_str(&format!("失败分类: {cat}\n"));
                }
                append_stream(&mut out, "--- stdout ---", &o.stdout);
                append_stream(&mut out, "--- stderr ---", &o.stderr);
            }
            None if rec.status.is_terminal() => {
                out.push_str("（任务已终态，但无输出记录。）\n");
            }
            None => {
                out.push_str("（任务尚未结束，暂无输出；可稍后再查。）\n");
            }
        }
        truncate_text(&out, max_output_len)
    }

    fn list(&self, workspace: &Path, limit: usize, max_output_len: usize) -> String {
        let rows = self.registry.list(Some(workspace), limit);
        if rows.is_empty() {
            return format!("后台任务：工作区 {} 下暂无记录。", workspace.display());
        }
        let now = SystemTime::now();
        let mut out = format!(
            "后台任务（工作区 {}，显示 {} 条，最新在前）:\n",
            workspace.display(),
            rows.len()
        );
        for r in &rows {
            let age_secs = now
                .duration_since(r.created_at)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let cmd = crate::cm_tools::tools::summarize_tool_call("run_command", &r.args_json)
                .unwrap_or_else(|| r.args_json.clone());
            out.push_str(&format!(
                "- {} [{}] {}（{}秒前）\n",
                r.id,
                r.status.as_str(),
                cmd,
                age_secs
            ));
        }
        truncate_text(&out, max_output_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_internal::tool_jobs::{
        CancelOutcome, JobLimits, JobOutcome, JobSpawn, JobStatus, ToolJobRegistry,
    };
    use std::path::PathBuf;
    use std::time::Duration;

    const NO_TRUNCATION: usize = 4096;

    fn limits() -> JobLimits {
        JobLimits {
            max_concurrent: 4,
            max_queued: 4,
            ttl: Duration::from_secs(3600),
            grace: Duration::from_secs(60),
            max_entries: 32,
            output_buffer_bytes: 262_144,
        }
    }

    fn spawn_default() -> JobSpawn {
        JobSpawn {
            program: "true".to_string(),
            args: Vec::new(),
            cwd: PathBuf::from("/"),
            extra_env: Vec::new(),
            wall: Duration::from_secs(10),
            max_output_len: 1024,
        }
    }

    fn setup() -> (Arc<ToolJobRegistry>, ToolJobsHost) {
        let reg = Arc::new(ToolJobRegistry::new(limits()));
        let host = ToolJobsHost::new(Arc::clone(&reg));
        (reg, host)
    }

    fn register(reg: &ToolJobRegistry, workspace: &str, args_json: &str) -> String {
        reg.register(
            PathBuf::from(workspace),
            None,
            spawn_default(),
            args_json.to_string(),
            None,
        )
        .expect("register")
    }

    fn succeeded_outcome() -> JobOutcome {
        JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: b"all good\n".to_vec(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        }
    }

    #[test]
    fn status_unknown_id_reports_not_found() {
        let (_reg, host) = setup();
        assert_eq!(
            host.status("tooljob_missing", NO_TRUNCATION),
            "错误：后台任务 `tooljob_missing` 不存在或从未创建。"
        );
    }

    #[test]
    fn status_queued_reports_pending_output() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"cargo","args":["test"]}"#);
        let out = host.status(&id, NO_TRUNCATION);
        assert!(out.contains(&format!("后台任务 {id}")), "{out}");
        assert!(out.contains("状态: queued"), "{out}");
        assert!(out.contains("工作区: /ws"), "{out}");
        assert!(out.contains("工作区已变更: 否"), "{out}");
        assert!(out.contains("命令: cargo test"), "{out}");
        assert!(out.contains("任务尚未结束，暂无输出"), "{out}");
    }

    #[test]
    fn status_succeeded_reports_exit_code_and_stdout() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"true"}"#);
        assert!(reg.try_start().is_some(), "try_start");
        assert!(reg.complete(&id, succeeded_outcome(), true), "complete");
        let out = host.status(&id, NO_TRUNCATION);
        assert!(out.contains("状态: succeeded"), "{out}");
        assert!(out.contains("工作区已变更: 是"), "{out}");
        assert!(out.contains("退出码: 0"), "{out}");
        assert!(out.contains("--- stdout ---\nall good\n"), "{out}");
        // 空 stderr 不产生小标题（`append_stream` 跳过空白流）。
        assert!(!out.contains("--- stderr ---"), "{out}");
    }

    #[test]
    fn status_failed_reports_error_code_and_failure_category() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"sleep"}"#);
        assert!(reg.try_start().is_some(), "try_start");
        let mut outcome = JobOutcome::failed("timeout");
        outcome.status = JobStatus::TimedOut;
        outcome.failure_category = Some("wall_clock".to_string());
        outcome.stderr = b"timed out\n".to_vec();
        assert!(reg.complete(&id, outcome, false), "complete");
        let out = host.status(&id, NO_TRUNCATION);
        assert!(out.contains("状态: timed_out"), "{out}");
        assert!(out.contains("错误码: timeout"), "{out}");
        assert!(out.contains("失败分类: wall_clock"), "{out}");
        assert!(out.contains("--- stderr ---\ntimed out\n"), "{out}");
        assert!(!out.contains("退出码:"), "{out}");
        assert!(!out.contains("--- stdout ---"), "{out}");
    }

    #[test]
    fn status_cancelled_queued_reports_error_code() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"true"}"#);
        // `queued` 直接取消 → 终态 `cancelled`，且 `outcome` 与 SSE 终态补发同构
        // （`error_code = cancelled`），使轮询侧与补发侧一致。
        assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);
        let out = host.status(&id, NO_TRUNCATION);
        assert!(out.contains("状态: cancelled"), "{out}");
        assert!(out.contains("错误码: cancelled"), "{out}");
        // 取消无输出：空 stdout/stderr 不渲染小标题。
        assert!(!out.contains("--- stdout ---"), "{out}");
        assert!(!out.contains("--- stderr ---"), "{out}");
    }

    #[test]
    fn status_expired_after_ttl_cleanup() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"true"}"#);
        assert!(reg.try_start().is_some(), "try_start");
        assert!(reg.complete(&id, succeeded_outcome(), false), "complete");
        // TTL 3600s + grace 60s：推进到 3700s 之后清理，再查询应报「已过保留时长」。
        assert!(reg.cleanup(SystemTime::now() + Duration::from_secs(3700)) >= 1);
        assert_eq!(
            host.status(&id, NO_TRUNCATION),
            format!("错误：后台任务 `{id}` 已过保留时长（TTL+宽限）并被清理。")
        );
    }

    #[test]
    fn status_truncates_at_max_output_len() {
        let (reg, host) = setup();
        let id = register(&reg, "/ws", r#"{"command":"true"}"#);
        let full = host.status(&id, NO_TRUNCATION);
        assert!(!full.contains("输出已截断"), "{full}");

        let cut = host.status(&id, 16);
        assert!(cut.len() < full.len(), "{cut}");
        assert!(cut.ends_with(" 字节）"), "{cut}");
        assert!(cut.contains("输出已截断"), "{cut}");
        assert!(full.starts_with(&cut[..6]), "{cut}");
    }

    #[test]
    fn truncate_text_never_splits_multibyte_chars() {
        // 「中文」每字 3 字节；7 落在第 3 个字中间，应回退到 6。
        let out = truncate_text("中文中文", 7);
        assert!(out.starts_with("中文\n"), "{out}");
        assert_eq!(out, "中文\n…（输出已截断，仅显示前 6 字节）");
    }

    #[test]
    fn list_without_records_reports_none_for_workspace() {
        let (reg, host) = setup();
        let _ = register(&reg, "/other", r#"{"command":"true"}"#);
        assert_eq!(
            host.list(Path::new("/ws"), 20, NO_TRUNCATION),
            "后台任务：工作区 /ws 下暂无记录。"
        );
    }

    #[test]
    fn list_filters_workspace_newest_first_with_command_summary() {
        let (reg, host) = setup();
        let first = register(&reg, "/ws", r#"{"command":"cargo","args":["test"]}"#);
        // `created_at` 取 `SystemTime::now()`，退避 1ms 以保证倒序断言确定性。
        std::thread::sleep(Duration::from_millis(1));
        let second = register(&reg, "/ws", r#"{"command":"true"}"#);
        let other = register(&reg, "/other", r#"{"command":"true"}"#);

        let out = host.list(Path::new("/ws"), 20, NO_TRUNCATION);
        assert!(
            out.starts_with("后台任务（工作区 /ws，显示 2 条，最新在前）:"),
            "{out}"
        );
        assert!(!out.contains(&other), "{out}");
        let first_at = out.find(&first).expect("first row");
        let second_at = out.find(&second).expect("second row");
        assert!(second_at < first_at, "{out}");
        assert!(out.contains(&format!("- {second} [queued] true（")), "{out}");
        assert!(
            out.contains(&format!("- {first} [queued] cargo test（")),
            "{out}"
        );
    }

    #[test]
    fn list_respects_limit() {
        let (reg, host) = setup();
        let _ = register(&reg, "/ws", r#"{"command":"true"}"#);
        let _ = register(&reg, "/ws", r#"{"command":"true"}"#);
        let out = host.list(Path::new("/ws"), 1, NO_TRUNCATION);
        assert!(out.contains("显示 1 条"), "{out}");
        assert_eq!(out.matches("- tooljob_").count(), 1, "{out}");
    }
}
