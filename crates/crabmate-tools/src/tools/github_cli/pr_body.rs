//! PR 正文草稿：读取仓库模板 + 可选 `git log` 摘要（只读，不写盘）。

use std::path::{Path, PathBuf};

use crate::tools::git;

const DEFAULT_MAX_COMMITS: usize = 30;
const MAX_COMMITS_CAP: usize = 200;

/// 在工作区内查找首个可用的 PR 模板 Markdown。
pub fn discover_pr_template(working_dir: &Path) -> Option<String> {
    let candidates = [
        working_dir.join(".github/pull_request_template.md"),
        working_dir.join(".github/PULL_REQUEST_TEMPLATE.md"),
    ];
    for p in candidates {
        if p.is_file()
            && let Ok(s) = std::fs::read_to_string(&p)
        {
            let t = s.trim();
            if !t.is_empty() {
                return Some(s);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(working_dir.join(".github/PULL_REQUEST_TEMPLATE")) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
            .collect();
        paths.sort();
        for p in paths {
            if let Ok(s) = std::fs::read_to_string(&p) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn is_safe_git_ref(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.chars().any(|c| {
            matches!(
                c,
                ';' | '|' | '&' | '$' | '`' | '\n' | '\r' | '(' | ')' | '<' | '>'
            )
        })
}

fn run_git_stdout(working_dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(working_dir);
    let out = cmd.output().map_err(|e| format!("无法执行 git: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "git 失败 (exit={}): {}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn commit_lines_since_base(
    working_dir: &Path,
    base: &str,
    max_commits: usize,
) -> Result<Vec<String>, String> {
    if !is_safe_git_ref(base) {
        return Err("错误：base 含非法字符".to_string());
    }
    let range = format!("{base}..HEAD");
    let max = max_commits.to_string();
    let text = run_git_stdout(
        working_dir,
        &[
            "log",
            &range,
            &format!("--max-count={max}"),
            "--format=- %h %s",
        ],
    )?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// 组装 PR 正文 Markdown 草稿（不写文件）。
pub fn build_pr_body_draft(
    working_dir: &Path,
    base: Option<&str>,
    max_commits: usize,
    include_template: bool,
    include_commit_log: bool,
) -> Result<String, String> {
    git::ensure_git_repo(working_dir)?;
    let max_commits = max_commits.clamp(1, MAX_COMMITS_CAP);
    let base_ref = base.unwrap_or("main").trim();
    if base_ref.is_empty() {
        return Err("错误：base 不能为空".to_string());
    }

    let mut parts: Vec<String> = Vec::new();
    parts.push(
        "> 以下为 **PR 正文草稿**（`gh_pr_body_draft` / `gh_pr_create` 的 `auto_body`）；请人工审阅后使用。\n"
            .to_string(),
    );

    if include_commit_log {
        match commit_lines_since_base(working_dir, base_ref, max_commits) {
            Ok(lines) if !lines.is_empty() => {
                parts.push(format!("## Summary\n\n_相对 `{base_ref}` 的最近提交：_\n"));
                parts.push(lines.join("\n"));
                parts.push(String::new());
            }
            Ok(_) => {
                parts.push(format!(
                    "## Summary\n\n_相对 `{base_ref}` 未找到额外提交（可能已与基线同步）。_\n"
                ));
            }
            Err(e) => parts.push(format!("## Summary\n\n_无法读取 git log：{e}_\n")),
        }
    }

    if include_template {
        if let Some(tmpl) = discover_pr_template(working_dir) {
            parts.push("## 仓库 PR 模板\n".to_string());
            parts.push(tmpl.trim().to_string());
        } else {
            parts.push("## Test plan\n\n- [ ] 本地测试\n- [ ] CI 通过\n".to_string());
        }
    }

    Ok(parts.join("\n").trim().to_string())
}

/// `gh_pr_body_draft` 工具入口（只读）。
pub fn gh_pr_body_draft(args_json: &str, working_dir: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let base = v.get("base").and_then(|x| x.as_str());
    let max_commits = v
        .get("max_commits")
        .and_then(|x| x.as_u64())
        .unwrap_or(DEFAULT_MAX_COMMITS as u64) as usize;
    let include_template = v
        .get("include_template")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let include_commit_log = v
        .get("include_commit_log")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    match build_pr_body_draft(
        working_dir,
        base,
        max_commits,
        include_template,
        include_commit_log,
    ) {
        Ok(body) => {
            if body.len() > max_output_len {
                let keep = max_output_len.saturating_sub(80);
                let truncated = super::super::output_util::truncate_to_char_boundary(&body, keep);
                format!(
                    "{}\n\n... (输出已按 {} 字节截断)",
                    truncated, max_output_len
                )
            } else {
                body
            }
        }
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_draft_in_git_repo_without_template() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "t@example.com"])
            .current_dir(dir.path())
            .status()
            .expect("git config email");
        std::process::Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir.path())
            .status()
            .expect("git config name");
        std::fs::write(dir.path().join("f.txt"), "x").expect("write");
        std::process::Command::new("git")
            .args(["add", "f.txt"])
            .current_dir(dir.path())
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(dir.path())
            .status()
            .expect("git commit");
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(dir.path())
            .status()
            .expect("branch main");

        let body = build_pr_body_draft(dir.path(), Some("main"), 10, false, true).expect("draft");
        assert!(body.contains("PR 正文草稿"), "{body}");
    }

    #[test]
    fn discover_template_reads_dot_github_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gh = dir.path().join(".github");
        std::fs::create_dir_all(&gh).expect("mkdir");
        std::fs::write(gh.join("pull_request_template.md"), "# TMPL\n\n- item").expect("write");
        let t = discover_pr_template(dir.path()).expect("template");
        assert!(t.contains("TMPL"), "{t}");
    }
}
