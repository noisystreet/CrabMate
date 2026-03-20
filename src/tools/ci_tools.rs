//! 本地 CI 流水线工具：fmt + clippy + test + frontend lint

use std::path::Path;

use super::{cargo_tools, frontend_tools, security_tools};

pub fn ci_pipeline_local(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let run_fmt = v.get("run_fmt").and_then(|x| x.as_bool()).unwrap_or(true);
    let run_clippy = v
        .get("run_clippy")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let run_test = v.get("run_test").and_then(|x| x.as_bool()).unwrap_or(true);
    let run_frontend_lint = v
        .get("run_frontend_lint")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let fail_fast = v.get("fail_fast").and_then(|x| x.as_bool()).unwrap_or(false);
    let summary_only = v
        .get("summary_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut sections = Vec::new();
    let mut summary: Vec<(String, &'static str)> = Vec::new();
    if run_fmt {
        let r = cargo_fmt_check(workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("cargo fmt --check".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(&mut summary, run_clippy, run_test, run_frontend_lint);
            return build_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("cargo fmt --check".to_string(), "skipped"));
    }
    if run_clippy {
        let r = cargo_tools::cargo_clippy(
            r#"{"all_targets":true}"#,
            workspace_root,
            max_output_len,
        );
        let failed = section_failed(&r);
        summary.push(("cargo clippy".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(&mut summary, false, run_test, run_frontend_lint);
            return build_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("cargo clippy".to_string(), "skipped"));
    }
    if run_test {
        let r = cargo_tools::cargo_test("{}", workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("cargo test".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_skipped(&mut summary, false, false, run_frontend_lint);
            return build_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("cargo test".to_string(), "skipped"));
    }
    if run_frontend_lint {
        let r = frontend_tools::frontend_lint(
            r#"{"subdir":"frontend","script":"lint"}"#,
            workspace_root,
            max_output_len,
        );
        let failed = section_failed(&r) && !r.contains("跳过（");
        summary.push(("frontend lint".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
    } else {
        summary.push(("frontend lint".to_string(), "skipped"));
    }
    build_output(&summary, &sections, summary_only, false)
}

pub fn release_ready_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let run_ci = v.get("run_ci").and_then(|x| x.as_bool()).unwrap_or(true);
    let run_audit = v.get("run_audit").and_then(|x| x.as_bool()).unwrap_or(true);
    let run_deny = v.get("run_deny").and_then(|x| x.as_bool()).unwrap_or(true);
    let require_clean = v
        .get("require_clean_worktree")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let fail_fast = v.get("fail_fast").and_then(|x| x.as_bool()).unwrap_or(false);
    let summary_only = v
        .get("summary_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    let mut sections = Vec::new();
    let mut summary: Vec<(String, &'static str)> = Vec::new();

    if run_ci {
        let r = ci_pipeline_local(
            r#"{"run_fmt":true,"run_clippy":true,"run_test":true,"run_frontend_lint":true,"fail_fast":false,"summary_only":false}"#,
            workspace_root,
            max_output_len,
        );
        let failed = r.contains("failed=");
        summary.push(("ci_pipeline_local".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_release_skipped(&mut summary, run_audit, run_deny, require_clean);
            return build_release_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("ci_pipeline_local".to_string(), "skipped"));
    }

    if run_audit {
        let r = security_tools::cargo_audit("{}", workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("cargo_audit".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_release_skipped(&mut summary, false, run_deny, require_clean);
            return build_release_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("cargo_audit".to_string(), "skipped"));
    }

    if run_deny {
        let r = security_tools::cargo_deny("{}", workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("cargo_deny".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
        if fail_fast && failed {
            push_release_skipped(&mut summary, false, false, require_clean);
            return build_release_output(&summary, &sections, summary_only, true);
        }
    } else {
        summary.push(("cargo_deny".to_string(), "skipped"));
    }

    if require_clean {
        let r = git_clean_check(workspace_root, max_output_len);
        let failed = section_failed(&r);
        summary.push(("git_clean_check".to_string(), if failed { "failed" } else { "passed" }));
        sections.push(r);
    } else {
        summary.push(("git_clean_check".to_string(), "skipped"));
    }

    build_release_output(&summary, &sections, summary_only, false)
}

fn push_release_skipped(summary: &mut Vec<(String, &'static str)>, audit: bool, deny: bool, clean: bool) {
    if audit {
        summary.push(("cargo_audit".to_string(), "skipped"));
    }
    if deny {
        summary.push(("cargo_deny".to_string(), "skipped"));
    }
    if clean {
        summary.push(("git_clean_check".to_string(), "skipped"));
    }
}

fn build_release_output(
    summary: &[(String, &'static str)],
    sections: &[String],
    summary_only: bool,
    fail_fast_triggered: bool,
) -> String {
    let passed = summary.iter().filter(|(_, s)| *s == "passed").count();
    let failed = summary.iter().filter(|(_, s)| *s == "failed").count();
    let skipped = summary.iter().filter(|(_, s)| *s == "skipped").count();
    let mut summary_text = String::new();
    summary_text.push_str("release_ready_check summary:\n");
    for (name, status) in summary {
        summary_text.push_str(&format!("- {}: {}\n", name, status));
    }
    summary_text.push_str(&format!(
        "统计：passed={}, failed={}, skipped={}",
        passed, failed, skipped
    ));
    if fail_fast_triggered {
        summary_text.push_str("\n已启用 fail_fast，出现失败后已停止后续步骤");
    }
    if summary_only {
        return summary_text;
    }
    if sections.is_empty() {
        summary_text
    } else {
        format!("{}\n\n====================\n\n{}", summary_text, sections.join("\n\n====================\n\n"))
    }
}

fn git_clean_check(workspace_root: &Path, max_output_len: usize) -> String {
    let out = std::process::Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(workspace_root)
        .output();
    match out {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if status != 0 {
                let msg = if !stderr.trim().is_empty() { stderr } else { stdout };
                return format!("git clean check (exit={}):\n{}", status, truncate_simple(&msg, max_output_len));
            }
            if stdout.trim().is_empty() {
                "git clean check (exit=0):\n工作区干净".to_string()
            } else {
                format!("git clean check (exit=1):\n存在未提交改动：\n{}", truncate_simple(&stdout, max_output_len))
            }
        }
        Err(e) => format!("git clean check (exit=1):\n执行失败: {}", e),
    }
}

fn truncate_simple(s: &str, max_output_len: usize) -> String {
    if s.len() <= max_output_len {
        s.to_string()
    } else {
        format!("{}\n\n... (输出已截断)", &s[..max_output_len])
    }
}

fn push_skipped(summary: &mut Vec<(String, &'static str)>, clippy: bool, test: bool, frontend: bool) {
    if clippy {
        summary.push(("cargo clippy".to_string(), "skipped"));
    }
    if test {
        summary.push(("cargo test".to_string(), "skipped"));
    }
    if frontend {
        summary.push(("frontend lint".to_string(), "skipped"));
    }
}

fn build_output(
    summary: &[(String, &'static str)],
    sections: &[String],
    summary_only: bool,
    fail_fast_triggered: bool,
) -> String {
    let passed = summary.iter().filter(|(_, s)| *s == "passed").count();
    let failed = summary.iter().filter(|(_, s)| *s == "failed").count();
    let skipped = summary.iter().filter(|(_, s)| *s == "skipped").count();
    let mut summary_text = String::new();
    summary_text.push_str("ci_pipeline_local summary:\n");
    for (name, status) in summary {
        summary_text.push_str(&format!("- {}: {}\n", name, status));
    }
    summary_text.push_str(&format!(
        "统计：passed={}, failed={}, skipped={}",
        passed, failed, skipped
    ));
    if fail_fast_triggered {
        summary_text.push_str("\n已启用 fail_fast，出现失败后已停止后续步骤");
    }
    if summary_only {
        return summary_text;
    }
    if sections.is_empty() {
        summary_text
    } else {
        format!("{}\n\n====================\n\n{}", summary_text, sections.join("\n\n====================\n\n"))
    }
}

fn section_failed(s: &str) -> bool {
    let first = s.lines().next().unwrap_or("");
    let Some(idx) = first.find("(exit=") else {
        return false;
    };
    let rest = &first[idx + "(exit=".len()..];
    let Some(end) = rest.find(')') else {
        return false;
    };
    rest[..end].trim().parse::<i32>().map(|c| c != 0).unwrap_or(false)
}

fn cargo_fmt_check(workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_root.join("Cargo.toml").is_file() {
        return "cargo fmt --check: 跳过（未找到 Cargo.toml）".to_string();
    }
    let out = std::process::Command::new("cargo")
        .arg("fmt")
        .arg("--")
        .arg("--check")
        .current_dir(workspace_root)
        .output();
    match out {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let body = if !stderr.trim().is_empty() {
                stderr.to_string()
            } else if !stdout.trim().is_empty() {
                stdout.to_string()
            } else {
                "(无输出)".to_string()
            };
            let mut s = body;
            if s.len() > max_output_len {
                s = format!("{}\n\n... (输出已截断)", &s[..max_output_len]);
            }
            format!("cargo fmt --check (exit={}):\n{}", status, s)
        }
        Err(e) => format!("cargo fmt --check: 执行失败（{}）", e),
    }
}

pub fn cargo_fmt_check_tool(_args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_fmt_check(workspace_root, max_output_len)
}

