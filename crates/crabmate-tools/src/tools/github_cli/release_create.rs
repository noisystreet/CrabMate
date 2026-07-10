//! `gh release create`（写远端 Release）。

use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    gh_allowed, push_bool_flag, push_extra_args_from_json, push_repo_arg, push_trimmed_string_flag,
    run_gh_vec, validate_pr_body, validate_release_tag, write_workspace_temp_markdown,
};

fn git_notes_since_last_tag(working_dir: &Path, max_commits: usize) -> Result<String, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["describe", "--tags", "--abbrev=0"])
        .current_dir(working_dir);
    let tag_out = cmd.output().map_err(|e| format!("无法执行 git: {e}"))?;
    let since = if tag_out.status.success() {
        let t = String::from_utf8_lossy(&tag_out.stdout).trim().to_string();
        if t.is_empty() {
            String::new()
        } else {
            format!("{t}..HEAD")
        }
    } else {
        String::new()
    };

    let mut log_cmd = std::process::Command::new("git");
    log_cmd.current_dir(working_dir);
    log_cmd.arg("log");
    if since.is_empty() {
        log_cmd.args(["--max-count", &max_commits.to_string(), "--format=- %h %s"]);
    } else {
        log_cmd.args([
            since.as_str(),
            &format!("--max-count={max_commits}"),
            "--format=- %h %s",
        ]);
    }
    let out = log_cmd
        .output()
        .map_err(|e| format!("无法执行 git log: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git log 失败: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_release_notes(v: &JsonValue, working_dir: &Path) -> Result<String, String> {
    let explicit = v.get("notes").and_then(|x| x.as_str()).unwrap_or("").trim();
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }
    if !v
        .get("auto_notes")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        return Ok(String::new());
    }
    let max = v
        .get("max_commits")
        .and_then(|x| x.as_u64())
        .unwrap_or(100)
        .clamp(1, 500) as usize;
    match git_notes_since_last_tag(working_dir, max) {
        Ok(s) if !s.is_empty() => Ok(format!("## Changes\n\n{s}")),
        Ok(_) => Ok("## Changes\n\n_无新提交._".to_string()),
        Err(e) => Ok(format!("## Changes\n\n_无法生成 notes：{e}_")),
    }
}

fn gh_release_create_build_argv(
    v: &JsonValue,
    tag: &str,
    notes_path: String,
) -> Result<Vec<String>, String> {
    let mut argv = vec![
        "release".into(),
        "create".into(),
        tag.to_string(),
        "--notes-file".into(),
        notes_path,
    ];
    push_trimmed_string_flag(v, "title", "--title", &mut argv);
    push_repo_arg(v, &mut argv)?;
    push_trimmed_string_flag(v, "target", "--target", &mut argv);
    push_bool_flag(v, "draft", "--draft", &mut argv);
    push_bool_flag(v, "prerelease", "--prerelease", &mut argv);
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh release create`（notes 经工作区临时 `--notes-file`）
pub fn gh_release_create(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let tag = match v.get("tag").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 tag".to_string(),
    };
    if let Err(e) = validate_release_tag(tag) {
        return e;
    }

    let notes = match resolve_release_notes(&v, working_dir) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if let Err(e) = validate_pr_body(&notes) {
        return e;
    }

    let (dir, notes_path) = match write_workspace_temp_markdown(
        working_dir,
        "crabmate_release_notes.md",
        notes.as_bytes(),
        "release notes",
    ) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let argv = match gh_release_create_build_argv(&v, tag, notes_path) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    drop(dir);
    out
}
