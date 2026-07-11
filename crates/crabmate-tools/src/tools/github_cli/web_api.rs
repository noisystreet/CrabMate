//! GitHub Web API 共享逻辑：结构化 JSON，供 HTTP handler 与日后 CLI/TUI 复用。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::common::{extract_stdout_from_formatted, gh_allowed, run_gh_vec};
use super::pr_issue::gh_pr_list;
use super::pr_workflow::gh_pr_checks;

const REPO_VIEW_FIELDS: &str = "nameWithOwner,url,defaultBranchRef";
const PR_LIST_FIELDS: &[&str] = &[
    "number",
    "title",
    "state",
    "url",
    "headRefName",
    "baseRefName",
    "isDraft",
];
const PR_VIEW_FIELDS: &[&str] = &[
    "number",
    "title",
    "state",
    "url",
    "headRefName",
    "baseRefName",
    "isDraft",
];

fn gh_exit_code(formatted: &str) -> Option<i32> {
    formatted
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("退出码："))
        .and_then(|s| s.trim().parse().ok())
}

fn gh_tool_error(formatted: &str) -> String {
    let t = formatted.trim();
    if t.is_empty() {
        return "gh 命令失败".to_string();
    }
    let stdout = extract_stdout_from_formatted(t).trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }
    t.lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_gh_json_stdout(formatted: &str) -> Result<JsonValue, String> {
    if gh_exit_code(formatted) != Some(0) {
        return Err(gh_tool_error(formatted));
    }
    let stdout = extract_stdout_from_formatted(formatted).trim();
    if stdout.is_empty() {
        return Err("gh 未返回 JSON 输出".to_string());
    }
    serde_json::from_str(stdout).map_err(|e| format!("解析 gh JSON 失败：{e}"))
}

fn current_git_branch(working_dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(working_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

fn is_git_repo(working_dir: &Path) -> bool {
    working_dir.join(".git").exists()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GithubRepoContextData {
    pub connected: bool,
    pub is_git_repo: bool,
    pub gh_available: bool,
    pub repo: Option<String>,
    pub url: Option<String>,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubPrItemData {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: Option<String>,
    pub head_ref: Option<String>,
    pub base_ref: Option<String>,
    pub is_draft: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GithubPrsData {
    pub items: Vec<GithubPrItemData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubPrCheckItemData {
    pub name: String,
    pub state: String,
    pub bucket: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GithubChecksSummaryData {
    pub total: usize,
    pub passing: usize,
    pub failing: usize,
    pub pending: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GithubPrCurrentChecksData {
    pub pr_number: Option<u64>,
    pub pr_title: Option<String>,
    pub pr_url: Option<String>,
    pub checks: Vec<GithubPrCheckItemData>,
    pub summary: GithubChecksSummaryData,
}

fn json_str(v: &JsonValue, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn json_u64(v: &JsonValue, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

fn json_bool(v: &JsonValue, key: &str) -> Option<bool> {
    v.get(key).and_then(|x| x.as_bool())
}

fn parse_pr_item(v: &JsonValue) -> Option<GithubPrItemData> {
    let number = json_u64(v, "number")?;
    let title = json_str(v, "title").unwrap_or_else(|| format!("#{number}"));
    let state = json_str(v, "state").unwrap_or_else(|| "UNKNOWN".to_string());
    Some(GithubPrItemData {
        number,
        title,
        state,
        url: json_str(v, "url"),
        head_ref: json_str(v, "headRefName"),
        base_ref: json_str(v, "baseRefName"),
        is_draft: json_bool(v, "isDraft"),
    })
}

fn summarize_checks(items: &[GithubPrCheckItemData]) -> GithubChecksSummaryData {
    let mut summary = GithubChecksSummaryData {
        total: items.len(),
        ..Default::default()
    };
    for item in items {
        let st = item.state.to_ascii_lowercase().replace(['_', '-'], "");
        if st.contains("fail") {
            summary.failing += 1;
        } else if st.contains("pend")
            || st.contains("progress")
            || st.contains("queued")
            || st.contains("waiting")
        {
            summary.pending += 1;
        } else if st.contains("pass") || st.contains("success") || st.contains("ok") {
            summary.passing += 1;
        } else {
            summary.pending += 1;
        }
    }
    summary
}

fn parse_check_items(v: &JsonValue) -> Vec<GithubPrCheckItemData> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let name = json_str(item, "name")?;
            let state = json_str(item, "state")
                .or_else(|| json_str(item, "bucket"))
                .unwrap_or_else(|| "?".to_string());
            Some(GithubPrCheckItemData {
                name,
                state: state.clone(),
                bucket: json_str(item, "bucket"),
                link: json_str(item, "link"),
            })
        })
        .collect()
}

/// 解析当前工作区的 GitHub 仓库上下文（只读）。
pub fn github_repo_context(
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> Result<GithubRepoContextData, String> {
    let gh_available = gh_allowed(allowed_commands).is_ok();
    let is_git_repo = is_git_repo(working_dir);
    let current_branch = current_git_branch(working_dir);
    let mut out = GithubRepoContextData {
        connected: false,
        is_git_repo,
        gh_available,
        current_branch,
        ..Default::default()
    };
    if !is_git_repo {
        return Ok(out);
    }
    if !gh_available {
        return Ok(out);
    }
    let argv = vec![
        "repo".into(),
        "view".into(),
        "--json".into(),
        REPO_VIEW_FIELDS.into(),
    ];
    let formatted = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    let v = parse_gh_json_stdout(&formatted)?;
    out.connected = true;
    out.repo = json_str(&v, "nameWithOwner");
    out.url = json_str(&v, "url");
    out.default_branch = v.get("defaultBranchRef").and_then(|b| json_str(b, "name"));
    Ok(out)
}

/// 列出 open PR（默认最多 30 条）。
pub fn github_pr_list_open(
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    limit: Option<u32>,
) -> Result<GithubPrsData, String> {
    gh_allowed(allowed_commands)?;
    let args = serde_json::json!({
        "state": "open",
        "limit": limit.unwrap_or(30),
        "fields": PR_LIST_FIELDS,
    });
    let formatted = gh_pr_list(
        &args.to_string(),
        max_output_len,
        allowed_commands,
        working_dir,
    );
    let v = parse_gh_json_stdout(&formatted)?;
    let items = v
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_pr_item).collect())
        .unwrap_or_default();
    Ok(GithubPrsData { items })
}

/// 当前分支关联 PR 的 checks（省略 PR number 时与 `gh pr checks` 默认一致）。
pub fn github_pr_current_checks(
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> Result<GithubPrCurrentChecksData, String> {
    gh_allowed(allowed_commands)?;
    let mut out = GithubPrCurrentChecksData::default();

    let view_argv = vec![
        "pr".into(),
        "view".into(),
        "--json".into(),
        PR_VIEW_FIELDS.join(","),
    ];
    let view_formatted = run_gh_vec(view_argv, max_output_len, allowed_commands, working_dir);
    if gh_exit_code(&view_formatted) == Some(0)
        && let Ok(v) = parse_gh_json_stdout(&view_formatted)
    {
        out.pr_number = json_u64(&v, "number");
        out.pr_title = json_str(&v, "title");
        out.pr_url = json_str(&v, "url");
    }

    let checks_args = serde_json::json!({ "structured": true });
    let checks_formatted = gh_pr_checks(
        &checks_args.to_string(),
        max_output_len,
        allowed_commands,
        working_dir,
    );
    let checks_v = parse_gh_json_stdout(&checks_formatted)?;
    out.checks = parse_check_items(&checks_v);
    out.summary = summarize_checks(&out.checks);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_checks_counts_buckets() {
        let items = vec![
            GithubPrCheckItemData {
                name: "ci".into(),
                state: "SUCCESS".into(),
                bucket: None,
                link: None,
            },
            GithubPrCheckItemData {
                name: "lint".into(),
                state: "FAILURE".into(),
                bucket: None,
                link: None,
            },
            GithubPrCheckItemData {
                name: "deploy".into(),
                state: "IN_PROGRESS".into(),
                bucket: None,
                link: None,
            },
        ];
        let s = summarize_checks(&items);
        assert_eq!(s.total, 3);
        assert_eq!(s.passing, 1);
        assert_eq!(s.failing, 1);
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn parse_pr_item_reads_fields() {
        let v = serde_json::json!({
            "number": 42,
            "title": "feat",
            "state": "OPEN",
            "url": "https://example/pr/42",
            "headRefName": "feat/x",
            "baseRefName": "main",
            "isDraft": false
        });
        let item = parse_pr_item(&v).expect("item");
        assert_eq!(item.number, 42);
        assert_eq!(item.title, "feat");
        assert_eq!(item.head_ref.as_deref(), Some("feat/x"));
    }
}
