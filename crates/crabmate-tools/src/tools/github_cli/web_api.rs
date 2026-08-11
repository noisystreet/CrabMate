//! GitHub Web API 共享逻辑：结构化 JSON，供 HTTP handler 与日后 CLI/TUI 复用。

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::common::{
    command_formatted_exit_code, extract_stdout_from_formatted, gh_allowed, run_gh_vec,
};
use super::pr_workflow::gh_pr_checks;

const REPO_VIEW_FIELDS: &str = "nameWithOwner,url,defaultBranchRef";
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
    command_formatted_exit_code(formatted)
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
    parse_gh_json_stdout_exits(formatted, &[0])
}

/// 允许的退出码下解析 stdout JSON（`gh pr checks`：0 全过 / 1 有失败 / 8 仍有 pending）。
fn parse_gh_json_stdout_exits(formatted: &str, ok_exits: &[i32]) -> Result<JsonValue, String> {
    match gh_exit_code(formatted) {
        Some(c) if ok_exits.contains(&c) => {}
        _ => {
            if formatted.contains("不支持 `pr checks --json`") {
                return Err(
                    "本机 GitHub CLI 不支持 `gh pr checks --json`（需 ≥ 2.50）。请升级 `gh`。"
                        .to_string(),
                );
            }
            return Err(gh_tool_error(formatted));
        }
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
    if working_dir.join(".git").exists() {
        return true;
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(working_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
}

fn git_remote_get_url(working_dir: &Path, remote: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(working_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// 将常见 GitHub remote 转为浏览器可打开的 HTTPS 页与 `owner/repo`。
///
/// 支持 `https://github.com/…`、`git@github.com:…`、`ssh://git@github.com/…`。
/// 路径大小写保留自原始 remote（仅对 scheme/host 做大小写不敏感匹配）。
fn github_web_from_remote_url(raw: &str) -> Option<(String, String)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let prefix_len = if lower.starts_with("https://github.com/") {
        "https://github.com/".len()
    } else if lower.starts_with("https://www.github.com/") {
        "https://www.github.com/".len()
    } else if lower.starts_with("git@github.com:") {
        "git@github.com:".len()
    } else if lower.starts_with("ssh://git@github.com/") {
        "ssh://git@github.com/".len()
    } else if lower.starts_with("ssh://github.com/") {
        "ssh://github.com/".len()
    } else {
        return None;
    };
    // 上述前缀均为 ASCII，字节长度与原串一致。
    let path = s.get(prefix_len..)?;
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    let name = format!("{owner}/{repo}");
    let url = format!("https://github.com/{name}");
    Some((name, url))
}

fn fill_repo_from_git_remotes(working_dir: &Path, out: &mut GithubRepoContextData) {
    if out
        .url
        .as_deref()
        .map(str::trim)
        .is_some_and(|u| !u.is_empty())
    {
        return;
    }
    for remote in ["origin", "upstream"] {
        let Some(raw) = git_remote_get_url(working_dir, remote) else {
            continue;
        };
        let Some((repo, url)) = github_web_from_remote_url(&raw) else {
            continue;
        };
        if out
            .repo
            .as_deref()
            .map(str::trim)
            .is_none_or(|r| r.is_empty())
        {
            out.repo = Some(repo);
        }
        out.url = Some(url);
        return;
    }
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
///
/// `connected` 表示本机 `gh repo view` 成功（CLI 已认证）。即使未连接，仍可能从
/// `git remote`（`origin` / `upstream`）解析出可打开的仓库 HTTPS URL。
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
    if gh_available {
        let argv = vec![
            "repo".into(),
            "view".into(),
            "--json".into(),
            REPO_VIEW_FIELDS.into(),
        ];
        let formatted = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
        if let Ok(v) = parse_gh_json_stdout(&formatted) {
            out.connected = true;
            out.repo = json_str(&v, "nameWithOwner");
            out.url = json_str(&v, "url");
            out.default_branch = v.get("defaultBranchRef").and_then(|b| json_str(b, "name"));
        }
    }
    fill_repo_from_git_remotes(working_dir, &mut out);
    Ok(out)
}

/// 指定 PR（或当前分支关联 PR）的 checks。
pub fn github_pr_checks(
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    number: Option<u64>,
) -> Result<GithubPrCurrentChecksData, String> {
    gh_allowed(allowed_commands)?;
    let mut out = GithubPrCurrentChecksData::default();

    let mut view_argv = vec!["pr".into(), "view".into()];
    if let Some(n) = number {
        if n == 0 || n > 999_999 {
            return Err("number 须为 1～999999".to_string());
        }
        view_argv.push(n.to_string());
    }
    view_argv.push("--json".into());
    view_argv.push(PR_VIEW_FIELDS.join(","));
    let view_formatted = run_gh_vec(view_argv, max_output_len, allowed_commands, working_dir);
    if gh_exit_code(&view_formatted) == Some(0)
        && let Ok(v) = parse_gh_json_stdout(&view_formatted)
    {
        out.pr_number = json_u64(&v, "number");
        out.pr_title = json_str(&v, "title");
        out.pr_url = json_str(&v, "url");
    }

    let mut checks_args = serde_json::json!({ "structured": true });
    if let Some(n) = number {
        checks_args["number"] = serde_json::json!(n);
    }
    let checks_formatted = gh_pr_checks(
        &checks_args.to_string(),
        max_output_len,
        allowed_commands,
        working_dir,
    );
    let checks_v = parse_gh_json_stdout_exits(&checks_formatted, &[0, 1, 8])?;
    out.checks = parse_check_items(&checks_v);
    out.summary = summarize_checks(&out.checks);
    Ok(out)
}

/// 当前分支关联 PR 的 checks（省略 PR number 时与 `gh pr checks` 默认一致）。
pub fn github_pr_current_checks(
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> Result<GithubPrCurrentChecksData, String> {
    github_pr_checks(max_output_len, allowed_commands, working_dir, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn github_web_from_remote_url_parses_common_forms() {
        let cases = [
            (
                "https://github.com/octocat/Hello-World.git",
                "octocat/Hello-World",
                "https://github.com/octocat/Hello-World",
            ),
            (
                "git@github.com:octocat/Hello-World.git",
                "octocat/Hello-World",
                "https://github.com/octocat/Hello-World",
            ),
            (
                "ssh://git@github.com/octocat/Hello-World",
                "octocat/Hello-World",
                "https://github.com/octocat/Hello-World",
            ),
        ];
        for (raw, repo, url) in cases {
            let got = github_web_from_remote_url(raw).expect(raw);
            assert_eq!(got.0, repo, "repo for {raw}");
            assert_eq!(got.1, url, "url for {raw}");
        }
        assert!(github_web_from_remote_url("https://gitlab.com/a/b").is_none());
        assert!(github_web_from_remote_url("").is_none());
    }

    #[test]
    fn github_repo_context_treats_subdir_as_git_repo() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
        if !dir.is_dir() {
            return;
        }
        let allowed = vec!["gh".to_string()];
        match github_repo_context(65536, &allowed, &dir) {
            Ok(ctx) => assert!(ctx.is_git_repo, "subdir inside repo should count as git"),
            Err(e) => {
                // gh CLI not installed or not authenticated; skip
                eprintln!("skipping: gh CLI unavailable ({e})");
            }
        }
    }

    #[test]
    fn github_checks_from_git_subdir_parses_structured_json() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
        if !dir.is_dir() {
            return;
        }
        let allowed = vec!["gh".to_string()];
        match github_pr_current_checks(65536, &allowed, &dir) {
            Ok(result) => assert!(!result.checks.is_empty(), "expected CI checks from gh"),
            Err(e) => {
                // gh CLI not installed or not authenticated; skip
                eprintln!("skipping: gh CLI unavailable ({e})");
            }
        }
    }

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
}
