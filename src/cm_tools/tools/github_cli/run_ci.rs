//! `gh run rerun` 与失败 CI 摘要（只读排障）。

use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    command_formatted_exit_code, extract_stdout_from_formatted, gh_allowed,
    push_extra_args_from_json, push_repo_arg, run_gh_vec, validate_job_name, validate_run_id,
};

/// `gh_pr_checks` / `structured: true` 请求的 `--json` 字段（与当前 `gh pr checks --help` 对齐）。
pub const PR_CHECKS_STRUCTURED_JSON_FIELDS: &str =
    "name,bucket,state,link,workflow,description,startedAt,completedAt,event";

fn parse_exit_code(formatted: &str) -> Option<i32> {
    command_formatted_exit_code(formatted)
}

fn gh_stdout(formatted: &str) -> String {
    extract_stdout_from_formatted(formatted).trim().to_string()
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        return text.trim().to_string();
    }
    lines[lines.len() - n..].join("\n")
}

fn summarize_failed_checks_json(stdout: &str) -> Option<String> {
    let v: JsonValue = serde_json::from_str(stdout.trim()).ok()?;
    let arr = v.as_array()?;
    let mut failed = Vec::new();
    let mut pending = Vec::new();
    for item in arr {
        let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("?");
        let state = item
            .get("state")
            .or_else(|| item.get("bucket"))
            .and_then(|x| x.as_str())
            .unwrap_or("?");
        let link = item.get("link").and_then(|x| x.as_str()).unwrap_or("");
        let line = if link.is_empty() {
            format!("- **{name}**: {state}")
        } else {
            format!("- **{name}**: {state} ({link})")
        };
        match check_state_bucket(state) {
            CheckStateBucket::Failed => failed.push(line),
            CheckStateBucket::Pending => pending.push(line),
            CheckStateBucket::Other => {}
        }
    }
    if failed.is_empty() && pending.is_empty() {
        return None;
    }
    Some(render_checks_summary(failed, pending))
}

/// 检查状态归类：失败 / 进行中 / 其它。
enum CheckStateBucket {
    Failed,
    Pending,
    Other,
}

fn check_state_bucket(state: &str) -> CheckStateBucket {
    let st = state.to_ascii_lowercase();
    if st.contains("fail") {
        CheckStateBucket::Failed
    } else if st.contains("pend") || st.contains("progress") || st.contains("queued") {
        CheckStateBucket::Pending
    } else {
        CheckStateBucket::Other
    }
}

/// 渲染失败 / 进行中检查列表。
fn render_checks_summary(failed: Vec<String>, pending: Vec<String>) -> String {
    let mut out = String::from("\n\n---\n**检查摘要**\n");
    if !failed.is_empty() {
        out.push_str("\n### 失败\n");
        out.push_str(&failed.join("\n"));
        out.push('\n');
    }
    if !pending.is_empty() {
        out.push_str("\n### 进行中 / 等待\n");
        out.push_str(&pending.join("\n"));
        out.push('\n');
    }
    out
}

/// 为 `gh pr checks` 在 structured 模式下附加检查摘要。
///
/// **不**要求退出码为 0：`gh pr checks` 在有失败时为 1、仍有进行中为 8，stdout 仍可能是合法 JSON。
pub fn append_checks_summary(formatted: String, stdout: &str) -> String {
    let Some(summary) = summarize_failed_checks_json(stdout) else {
        return formatted;
    };
    format!("{}{}", formatted.trim_end(), summary)
}

/// `gh pr checks --json` 自 GitHub CLI **2.50** 起支持；更旧版本会报 `unknown flag: --json`。
pub fn gh_pr_checks_rejects_json_flag(formatted: &str) -> bool {
    formatted
        .to_ascii_lowercase()
        .contains("unknown flag: --json")
}

/// `structured` 成功路径：在退出码 0/1/8 且 stdout 为 JSON 时附加美化块与失败摘要。
///
/// 退出码 0 时 `run_gh_vec` 可能已附加美化块；1/8 时需在此补上。
pub fn finalize_structured_pr_checks(formatted: String) -> String {
    let stdout = extract_stdout_from_formatted(&formatted).to_string();
    let with_json = match parse_exit_code(&formatted) {
        // 0=全过（`run_gh_vec` 通常已美化）；1=有失败；8=仍有 pending
        Some(1 | 8) => wrap_with_parsed_if_json(formatted, stdout.as_str()),
        _ => formatted,
    };
    append_checks_summary(with_json, stdout.as_str())
}

fn wrap_with_parsed_if_json(formatted: String, stdout: &str) -> String {
    if serde_json::from_str::<JsonValue>(stdout.trim()).is_ok() {
        super::common::wrap_with_parsed(formatted, stdout)
    } else {
        formatted
    }
}

/// `gh run rerun`（写远端：重新运行 workflow）
pub fn gh_run_rerun(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let run_id = match v.get("run_id").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 run_id".to_string(),
    };
    if let Err(e) = validate_run_id(run_id) {
        return e;
    }
    let mut argv = vec!["run".into(), "rerun".into(), run_id.to_string()];
    if let Err(e) = push_repo_arg(&v, &mut argv) {
        return e;
    }
    if v.get("failed").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--failed".into());
    }
    if let Err(e) = push_extra_args_from_json(&v, &mut argv) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

fn collect_failed_jobs(parsed: &JsonValue) -> Vec<(String, String)> {
    parsed
        .get("jobs")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|j| {
                    if j.get("conclusion").and_then(|x| x.as_str()) != Some("failure") {
                        return None;
                    }
                    let name = j.get("name").and_then(|x| x.as_str())?.to_string();
                    let st = j
                        .get("status")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some((name, st))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn failure_summary_header(parsed: &JsonValue, run_id: &str) -> String {
    let title = parsed
        .get("displayTitle")
        .and_then(|x| x.as_str())
        .unwrap_or("(无标题)");
    let url = parsed.get("url").and_then(|x| x.as_str()).unwrap_or("");
    let conclusion = parsed
        .get("conclusion")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let workflow = parsed
        .get("workflowName")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let mut report = format!(
        "# CI 失败摘要\n\n- **Run**: `{run_id}`\n- **Workflow**: {workflow}\n- **结论**: {conclusion}\n- **标题**: {title}\n"
    );
    if !url.is_empty() {
        report.push_str(&format!("- **URL**: {url}\n"));
    }
    report.push('\n');
    report
}

struct JobLogAppendCtx<'a> {
    run_id: &'a str,
    args: &'a JsonValue,
    tail_lines: usize,
    max_output_len: usize,
    allowed_commands: &'a [String],
    working_dir: &'a Path,
}

fn append_job_log_section(
    report: &mut String,
    name: &str,
    status: &str,
    ctx: &JobLogAppendCtx<'_>,
) {
    if let Err(e) = validate_job_name(name) {
        report.push_str(&format!("### {name}\n\n_跳过日志：{e}_\n\n"));
        return;
    }
    report.push_str(&format!("### {name} ({status})\n\n"));
    let mut log_argv = vec![
        "run".into(),
        "view".into(),
        ctx.run_id.to_string(),
        "--log".into(),
        "--job".into(),
        name.to_string(),
    ];
    if let Some(r) = ctx.args.get("repo").and_then(|x| x.as_str()) {
        log_argv.push("-R".into());
        log_argv.push(r.trim().to_string());
    }
    let log_out = run_gh_vec(
        log_argv,
        ctx.max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    );
    let log_text = gh_stdout(&log_out);
    if log_text.is_empty() {
        report.push_str("_（无日志输出或拉取失败）_\n\n");
    } else {
        report.push_str("```\n");
        report.push_str(&tail_lines(&log_text, ctx.tail_lines));
        report.push_str("\n```\n\n");
    }
}

fn truncate_report(report: String, max_output_len: usize) -> String {
    if report.len() <= max_output_len {
        return report;
    }
    let keep = max_output_len.saturating_sub(80);
    let truncated = super::super::output_util::truncate_to_char_boundary(&report, keep);
    format!(
        "{}\n\n... (输出已按 {} 字节截断)",
        truncated, max_output_len
    )
}

/// `gh run failure summary`：解析 failed jobs 并拉取各 job 日志尾部（只读）。
pub fn gh_run_failure_summary(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let run_id = match v.get("run_id").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 run_id".to_string(),
    };
    if let Err(e) = validate_run_id(run_id) {
        return e;
    }
    let tail_lines_n = v
        .get("tail_lines")
        .and_then(|x| x.as_u64())
        .unwrap_or(60)
        .clamp(10, 500) as usize;
    let max_failed_jobs = v
        .get("max_failed_jobs")
        .and_then(|x| x.as_u64())
        .unwrap_or(5)
        .clamp(1, 20) as usize;

    let mut view_argv = vec![
        "run".into(),
        "view".into(),
        run_id.to_string(),
        "--json".into(),
        "jobs,conclusion,displayTitle,url,status,workflowName".into(),
    ];
    if let Err(e) = push_repo_arg(&v, &mut view_argv) {
        return e;
    }

    let view_out = run_gh_vec(view_argv, max_output_len, allowed_commands, working_dir);
    if parse_exit_code(&view_out) != Some(0) {
        return view_out;
    }
    let stdout = gh_stdout(&view_out);
    let parsed: JsonValue = match serde_json::from_str(&stdout) {
        Ok(j) => j,
        Err(e) => return format!("错误：无法解析 run view JSON：{e}\n原始输出：\n{stdout}"),
    };

    let failed_jobs = collect_failed_jobs(&parsed);
    let mut report = failure_summary_header(&parsed, run_id);
    if failed_jobs.is_empty() {
        report.push_str("_未在 JSON 中发现 conclusion=failure 的 job；可能仍在运行或失败发生在 workflow 级。_\n");
        return truncate_report(report, max_output_len);
    }

    report.push_str(&format!("## 失败 Job（最多 {max_failed_jobs} 个）\n\n"));
    let log_ctx = JobLogAppendCtx {
        run_id,
        args: &v,
        tail_lines: tail_lines_n,
        max_output_len,
        allowed_commands,
        working_dir,
    };
    for (name, status) in failed_jobs.iter().take(max_failed_jobs) {
        append_job_log_section(&mut report, name, status, &log_ctx);
    }
    truncate_report(report, max_output_len)
}

#[cfg(test)]
mod tests {
    use super::{
        append_checks_summary, finalize_structured_pr_checks, gh_pr_checks_rejects_json_flag,
        summarize_failed_checks_json,
    };

    #[test]
    fn summarize_failed_checks_json_lists_failures() {
        let raw = r#"[{"name":"ci","state":"FAILURE","link":"https://example.com"}]"#;
        let s = summarize_failed_checks_json(raw).expect("summary");
        assert!(s.contains("失败"), "{s}");
        assert!(s.contains("ci"), "{s}");
    }

    #[test]
    fn gh_pr_checks_rejects_json_flag_detects_stderr() {
        let raw = "命令：gh pr checks 3 --json name\n退出码：1\n标准错误：\nunknown flag: --json\n\nUsage:  gh pr checks [<number> | <url> | <branch>] [flags]\n";
        assert!(gh_pr_checks_rejects_json_flag(raw));
        assert!(!gh_pr_checks_rejects_json_flag(
            "退出码：0\n标准输出：\n[]\n"
        ));
    }

    #[test]
    fn append_checks_summary_works_when_exit_is_failure() {
        let raw = r#"[{"name":"ci","state":"FAILURE","link":"https://example.com"}]"#;
        let formatted = format!("命令：gh pr checks\n退出码：1\n标准输出：\n{raw}\n");
        let out = append_checks_summary(formatted, raw);
        assert!(out.contains("检查摘要"), "{out}");
        assert!(out.contains("失败"), "{out}");
    }

    #[test]
    fn finalize_structured_pr_checks_attaches_summary_on_exit_1() {
        let raw =
            r#"[{"name":"lint","bucket":"fail","state":"FAILURE","link":"https://example.com/x"}]"#;
        let formatted = format!("命令：gh pr checks --json …\n退出码：1\n标准输出：\n{raw}\n");
        let out = finalize_structured_pr_checks(formatted);
        assert!(out.contains("解析后的 JSON"), "{out}");
        assert!(out.contains("检查摘要"), "{out}");
        assert!(out.contains("lint"), "{out}");
    }
}
