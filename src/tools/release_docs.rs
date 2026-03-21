//! 发版辅助：变更日志 Markdown 草稿、依赖许可证摘要（只生成文本，不写仓库）。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::process::Command;

use super::git;

fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    format!(
        "{}\n\n... (输出已按 {} 字节截断)",
        &s[..max_bytes.saturating_sub(80)],
        max_bytes
    )
}

/// 仅允许常见 Git 引用字符，降低注入风险（传给 `git log` 等子进程）。
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
    let mut cmd = Command::new("git");
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(working_dir);
    let out = cmd.output().map_err(|e| format!("无法执行 git: {}", e))?;
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

/// 基于 `git log` 生成 **Markdown 草稿**（不写任何文件）。
///
/// 参数：
/// - `since` / `until`：可选引用，范围 `since..until`；皆空则自 HEAD 起；仅 `until` 时 `git log until`
/// - `max_commits`：默认 500，上限 2000
/// - `group_by`：`date`（按提交日聚合）| `flat`（单层列表）| `tag_ranges`（按相邻 tag 分段，tag 按 `-v:refname` 降序取最近若干）
/// - `max_tag_sections`：`tag_ranges` 时最多展示几段（默认 25）
pub fn changelog_draft(args_json: &str, working_dir: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    if let Err(e) = git::ensure_git_repo(working_dir) {
        return e;
    }

    let since = v
        .get("since")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("");
    let until = v
        .get("until")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("");
    if !since.is_empty() && !is_safe_git_ref(since) {
        return "错误：since 含非法字符".to_string();
    }
    if !until.is_empty() && !is_safe_git_ref(until) {
        return "错误：until 含非法字符".to_string();
    }

    let max_commits = v
        .get("max_commits")
        .and_then(|x| x.as_u64())
        .unwrap_or(500)
        .clamp(1, 2000) as usize;

    let group_by = v
        .get("group_by")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "date".to_string());

    let max_tag_sections = v
        .get("max_tag_sections")
        .and_then(|x| x.as_u64())
        .unwrap_or(25)
        .clamp(1, 100) as usize;

    let mut md = String::from(
        "> 以下为 **Markdown 草稿**，由 `changelog_draft` 生成；**未写入**任何文件，请人工审阅后使用。\n\n",
    );

    let body = match group_by.as_str() {
        "flat" => changelog_flat(working_dir, since, until, max_commits),
        "tag_ranges" | "tags" => {
            changelog_by_tag_ranges(working_dir, max_commits, max_tag_sections)
        }
        "date" => changelog_by_date(working_dir, since, until, max_commits),
        _ => changelog_by_date(working_dir, since, until, max_commits),
    };

    match body {
        Ok(s) => {
            md.push_str(&s);
            truncate_str(&md, max_output_len)
        }
        Err(e) => e,
    }
}

/// 追加 `git log` 的 rev 参数（若有）；`(空,空)` 表示从 HEAD 走祖先。
fn push_log_rev(args: &mut Vec<String>, since: &str, until: &str) {
    match (since.is_empty(), until.is_empty()) {
        (true, true) => {}
        (true, false) => args.push(until.to_string()),
        (false, true) => args.push(format!("{}..HEAD", since)),
        (false, false) => args.push(format!("{}..{}", since, until)),
    }
}

fn changelog_by_date(
    working_dir: &Path,
    since: &str,
    until: &str,
    max_commits: usize,
) -> Result<String, String> {
    let mut args: Vec<String> = vec![
        "log".into(),
        format!("--max-count={}", max_commits),
        "--format=%cs|%h %s".into(),
    ];
    push_log_rev(&mut args, since, until);
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let text = run_git_stdout(working_dir, &argv)?;

    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((date, rest)) = line.split_once('|') else {
            continue;
        };
        sections
            .entry(date.to_string())
            .or_default()
            .push(format!("- {}", rest.trim()));
    }

    let mut out = String::from("# 变更日志（草稿）\n\n");
    if !since.is_empty() || !until.is_empty() {
        out.push_str(&format!(
            "_范围：`{}` … `{}`（Git 范围语法）_\n\n",
            if since.is_empty() { "(起)" } else { since },
            if until.is_empty() { "HEAD" } else { until }
        ));
    }
    for (date, items) in sections.into_iter().rev() {
        out.push_str(&format!("## {}\n\n", date));
        for it in items {
            out.push_str(&it);
            out.push('\n');
        }
        out.push('\n');
    }
    if out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}

fn changelog_flat(
    working_dir: &Path,
    since: &str,
    until: &str,
    max_commits: usize,
) -> Result<String, String> {
    let mut args: Vec<String> = vec![
        "log".into(),
        format!("--max-count={}", max_commits),
        "--format=- %h %s".into(),
    ];
    push_log_rev(&mut args, since, until);
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let text = run_git_stdout(working_dir, &argv)?;
    let mut out = String::from("# 变更日志（草稿 · 平铺）\n\n");
    out.push_str(&text);
    Ok(out)
}

fn changelog_by_tag_ranges(
    working_dir: &Path,
    max_commits_per_section: usize,
    max_sections: usize,
) -> Result<String, String> {
    let tags_out = run_git_stdout(working_dir, &["tag", "--sort=-v:refname"])?;
    let tags: Vec<String> = tags_out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if tags.is_empty() {
        return Ok("# 变更日志（草稿 · 按 tag）\n\n_未找到任何 tag，可改用 `group_by: \"date\"` 或 `flat`._\n"
            .to_string());
    }
    if tags.len() < 2 {
        return Ok(
            "# 变更日志（草稿 · 按 tag 区间）\n\n_仅有一个 tag，无法形成相邻区间；请改用 `group_by: \"date\"` / `flat`，或增加 tag 后再试。_\n"
                .to_string(),
        );
    }

    let take_tags = (max_sections + 1).min(tags.len());
    let tags: Vec<String> = tags.into_iter().take(take_tags).collect();
    let pairs = tags.len() - 1;

    let mut out = String::from("# 变更日志（草稿 · 按 tag 区间）\n\n");
    out.push_str("_说明：每段为「较旧 tag .. 较新 tag」之间的提交（`--no-merges`）；tag 按 `-v:refname` 降序取前 `max_tag_sections+1` 个后两两相邻配对。_\n\n");

    for i in 0..pairs {
        let hi = &tags[i];
        let lo = &tags[i + 1];
        if !is_safe_git_ref(hi) || !is_safe_git_ref(lo) {
            continue;
        }
        let range = format!("{}..{}", lo, hi);
        let mc = format!("--max-count={}", max_commits_per_section);
        let log = run_git_stdout(
            working_dir,
            &["log", &mc, "--no-merges", "--format=- %h %s", &range],
        )?;

        out.push_str(&format!("## `{}` ← `{}`\n\n", hi, lo));
        if log.trim().is_empty() {
            out.push_str("_（此区间内无提交）_\n\n");
        } else {
            out.push_str(log.trim_end());
            out.push_str("\n\n");
        }
    }

    Ok(out)
}

/// 使用 `cargo metadata` 解析依赖包，生成 **crate → 许可证** Markdown 表（只读，不写文件）。
///
/// - `workspace_only`：仅工作区成员包（默认 false，含传递依赖）
/// - `max_crates`：最多输出行数（默认 500，上限 3000）
pub fn license_notice(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }

    let workspace_only = v
        .get("workspace_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let max_crates = v
        .get("max_crates")
        .and_then(|x| x.as_u64())
        .unwrap_or(500)
        .clamp(1, 3000) as usize;

    let output = match Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .current_dir(workspace_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format!("无法执行 cargo metadata: {}", e),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return format!(
            "cargo metadata 失败 (exit={}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    let meta: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(m) => m,
        Err(e) => return format!("解析 cargo metadata JSON 失败: {}", e),
    };

    let workspace_ids: HashSet<String> = meta
        .get("workspace_members")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let packages = match meta.get("packages").and_then(|x| x.as_array()) {
        Some(p) => p,
        None => return "错误：metadata 中无 packages 数组".to_string(),
    };

    let mut by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for pkg in packages {
        let Some(id) = pkg.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        if workspace_only && !workspace_ids.contains(id) {
            continue;
        }
        let Some(name) = pkg.get("name").and_then(|x| x.as_str()) else {
            continue;
        };
        let lic = pkg
            .get("license")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| "(Cargo.toml 未声明 license)".to_string());
        by_name.entry(name.to_string()).or_default().insert(lic);
    }

    let mut md = String::from(
        "> 以下为 **许可证摘要草稿**（来自 `cargo metadata`），**非法律意见**；未声明项已标注。发版前请人工核对。\n\n",
    );
    md.push_str("# 依赖许可证摘要\n\n");
    md.push_str("| crate | license（合并多版本） |\n|---|---|\n");

    for (count, (name, set)) in by_name.iter().enumerate() {
        if count >= max_crates {
            break;
        }
        let lic_cell = set.iter().cloned().collect::<Vec<_>>().join(" · ");
        let name_esc = name.replace('|', "\\|");
        let lic_esc = lic_cell.replace('|', "\\|");
        md.push_str(&format!("| {} | {} |\n", name_esc, lic_esc));
    }

    if by_name.len() > max_crates {
        md.push_str(&format!(
            "\n_（仅展示前 {} 条，共 {} 个 crate 名；可调大 `max_crates`）_\n",
            max_crates,
            by_name.len()
        ));
    }

    truncate_str(&md, max_output_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_table_from_sample_metadata() {
        let sample = r#"{
            "packages": [
                {"id":"a","name":"serde","license":"MIT"},
                {"id":"b","name":"serde","license":"MIT OR Apache-2.0"},
                {"id":"c","name":"nolic","license":null}
            ],
            "workspace_members": []
        }"#;
        let meta: serde_json::Value = serde_json::from_str(sample).unwrap();
        let packages = meta["packages"].as_array().unwrap();
        let mut by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for pkg in packages {
            let name = pkg["name"].as_str().unwrap();
            let lic = pkg["license"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| "(Cargo.toml 未声明 license)".to_string());
            by_name.entry(name.to_string()).or_default().insert(lic);
        }
        assert!(by_name["serde"].len() == 2);
        assert!(by_name["nolic"].contains("(Cargo.toml 未声明 license)"));
    }

    #[test]
    fn safe_git_ref_rejects_shell() {
        assert!(!is_safe_git_ref("main;rm -rf"));
        assert!(is_safe_git_ref("v1.0.0"));
        assert!(is_safe_git_ref("feature/foo-bar"));
    }
}
