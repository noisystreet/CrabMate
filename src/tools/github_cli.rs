//! 内置 GitHub CLI（`gh`）封装：结构化参数；**退出码 0** 且 **stdout 整段为合法 JSON** 时附加格式化块（与是否传入 `--json` 字段无关）。
//!
//! 须 **`allowed_commands` 含 `gh`**（嵌入默认已含）。**`gh_api`** 在变更类 HTTP 方法下可能修改远端资源，已列入写副作用工具集。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value as JsonValue;

use super::command;
use super::output_util;

const MAX_LIMIT: u32 = 200;
const DEFAULT_LIST_LIMIT: u32 = 30;
const MAX_SEARCH_LIMIT: u32 = 100;
const TRUNCATE_LINES: usize = 500;
const MAX_SEARCH_QUERY_BYTES: usize = 400;
const MAX_RELEASE_TAG_LEN: usize = 200;
const MAX_JOB_NAME_LEN: usize = 128;

fn is_safe_token(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && !t.contains("..") && !t.starts_with('/')
}

fn validate_repo(repo: &str) -> Result<(), String> {
    let t = repo.trim();
    if t.is_empty() {
        return Err("错误：repo 不能为空".to_string());
    }
    if t.contains("..") || t.starts_with('/') {
        return Err("错误：repo 不得含 \"..\" 或以 \"/\" 开头（请用 owner/repo）".to_string());
    }
    let parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 {
        return Err("错误：repo 须为 owner/repo 两段式".to_string());
    }
    Ok(())
}

fn validate_extra_args(args: &[String]) -> Result<(), String> {
    for a in args {
        if !is_safe_token(a) {
            return Err(format!(
                "错误：extra_args 中含非法参数 {:?}（不得含 \"..\" 或以 \"/\" 开头）",
                a
            ));
        }
    }
    Ok(())
}

fn validate_api_path(path: &str) -> Result<(), String> {
    let t = path.trim();
    if t.is_empty() {
        return Err("错误：path 不能为空".to_string());
    }
    if t.contains("..") {
        return Err("错误：path 不得包含 \"..\"".to_string());
    }
    if t.starts_with('/') {
        return Err(
            "错误：path 不得以 \"/\" 开头（请用相对路径，如 repos/owner/repo/issues）".to_string(),
        );
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/_.@-:".contains(c))
    {
        return Err("错误：path 仅允许字母数字与 / _ . @ - :".to_string());
    }
    Ok(())
}

fn gh_allowed(allowed: &[String]) -> Result<(), String> {
    if allowed.iter().any(|c| c == "gh") {
        Ok(())
    } else {
        Err("错误：当前配置 allowed_commands 未包含 gh（可在 config/tools.toml 或 AGENT_ALLOWED_COMMANDS 中加入）".to_string())
    }
}

fn join_json_fields(fields: &[String]) -> Result<String, String> {
    if fields.is_empty() {
        return Err("错误：fields 数组不能为空".to_string());
    }
    let mut out = Vec::new();
    for f in fields {
        let t = f.trim();
        if t.is_empty() {
            return Err("错误：fields 中含空字符串".to_string());
        }
        if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("错误：非法 json 字段名 {:?}", t));
        }
        out.push(t.to_string());
    }
    Ok(out.join(","))
}

fn clamp_limit(n: Option<u32>) -> u32 {
    n.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIMIT)
}

fn try_pretty_json(stdout: &str) -> Option<String> {
    let t = stdout.trim();
    if t.is_empty() {
        return None;
    }
    let v: JsonValue = serde_json::from_str(t).ok()?;
    serde_json::to_string_pretty(&v).ok()
}

fn wrap_with_parsed(raw: String, stdout: &str) -> String {
    if let Some(pretty) = try_pretty_json(stdout) {
        format!(
            "{raw}\n\n---\n解析后的 JSON（供模型直接使用）：\n{pretty}",
            raw = raw.trim_end(),
            pretty = pretty
        )
    } else {
        raw
    }
}

/// 从 `command::run` 风格输出中提取「标准输出：」段落（不含后续「标准错误：」）。
fn extract_stdout_from_formatted(out: &str) -> &str {
    let Some(idx) = out.find("标准输出：\n") else {
        return "";
    };
    let start = idx + "标准输出：\n".len();
    let end = out[start..]
        .find("\n标准错误：\n")
        .map(|e| start + e)
        .unwrap_or(out.len());
    &out[start..end]
}

/// 首行 `退出码：N` 为 0 且 stdout 可解析为 JSON 时附加格式化块。
fn attach_json_if_exit_zero(formatted: String, stdout_raw: &str) -> String {
    let first = formatted.lines().next().unwrap_or("");
    let code = first
        .strip_prefix("退出码：")
        .and_then(|s| s.trim().parse::<i32>().ok());
    if code != Some(0) {
        return formatted;
    }
    wrap_with_parsed(formatted, stdout_raw)
}

fn run_gh_vec(
    argv: Vec<String>,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    let args_json = match serde_json::to_string(&serde_json::json!({
        "command": "gh",
        "args": argv,
    })) {
        Ok(s) => s,
        Err(e) => return format!("错误：构造 gh 参数失败：{e}"),
    };
    let out = command::run(
        &args_json,
        max_output_len,
        allowed_commands,
        working_dir,
        None,
    );
    let stdout = extract_stdout_from_formatted(&out).to_string();
    attach_json_if_exit_zero(out, stdout.as_str())
}

fn validate_run_id(id: &str) -> Result<(), String> {
    let t = id.trim();
    if t.is_empty() {
        return Err("错误：run_id 不能为空".to_string());
    }
    if t.len() > 24 {
        return Err("错误：run_id 过长".to_string());
    }
    if !t.chars().all(|c| c.is_ascii_digit()) {
        return Err(
            "错误：run_id 须为纯数字（与 `gh run list --json databaseId` 一致）".to_string(),
        );
    }
    Ok(())
}

fn validate_release_tag(tag: &str) -> Result<(), String> {
    let t = tag.trim();
    if t.is_empty() {
        return Err("错误：tag 不能为空".to_string());
    }
    if t.len() > MAX_RELEASE_TAG_LEN {
        return Err("错误：tag 过长".to_string());
    }
    if t.contains("..") || t.starts_with('/') {
        return Err("错误：tag 不得含 \"..\" 或以 \"/\" 开头".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.+@".contains(c))
    {
        return Err("错误：tag 仅允许字母数字与 - _ . + @".to_string());
    }
    Ok(())
}

fn validate_search_query(q: &str) -> Result<(), String> {
    let t = q.trim();
    if t.is_empty() {
        return Err("错误：query 不能为空".to_string());
    }
    if t.len() > MAX_SEARCH_QUERY_BYTES {
        return Err(format!(
            "错误：query 过长（上限 {} 字节）",
            MAX_SEARCH_QUERY_BYTES
        ));
    }
    if t.contains("..") {
        return Err("错误：query 不得包含 \"..\"".to_string());
    }
    for ch in t.chars() {
        if matches!(ch, '\n' | '\r' | '\0' | '\t') {
            return Err("错误：query 不得含换行、制表符或空字符".to_string());
        }
        if matches!(ch, ';' | '|' | '&' | '`' | '$' | '<' | '>') {
            return Err(format!("错误：query 含不允许的字符 {:?}", ch));
        }
    }
    Ok(())
}

fn validate_job_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("错误：job 不能为空".to_string());
    }
    if t.len() > MAX_JOB_NAME_LEN {
        return Err("错误：job 名称过长".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ')
    {
        return Err("错误：job 仅允许字母数字、空格、连字符与下划线".to_string());
    }
    Ok(())
}

fn clamp_search_limit(n: Option<u32>) -> u32 {
    n.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_SEARCH_LIMIT)
}

/// `gh pr list`
pub fn gh_pr_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let mut argv = vec!["pr".into(), "list".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(s) = v.get("state").and_then(|x| x.as_str()) {
        let st = s.trim();
        if !matches!(st, "open" | "closed" | "merged" | "all") {
            return "错误：state 须为 open、closed、merged 或 all".to_string();
        }
        if st != "open" {
            argv.push("--state".into());
            argv.push(st.to_string());
        }
    }
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh pr view <n>`
pub fn gh_pr_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let num = match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= 999_999 => n.to_string(),
        _ => return "错误：缺少或非法 number（须为 1～999999 的正整数）".to_string(),
    };
    let mut argv = vec!["pr".into(), "view".into(), num];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh issue list`
pub fn gh_issue_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let mut argv = vec!["issue".into(), "list".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(s) = v.get("state").and_then(|x| x.as_str()) {
        let st = s.trim();
        if !matches!(st, "open" | "closed" | "all") {
            return "错误：state 须为 open、closed 或 all".to_string();
        }
        if st != "open" {
            argv.push("--state".into());
            argv.push(st.to_string());
        }
    }
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh issue view <n>`
pub fn gh_issue_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let num = match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= 9_999_999 => n.to_string(),
        _ => return "错误：缺少或非法 number（须为正整数）".to_string(),
    };
    let mut argv = vec!["issue".into(), "view".into(), num];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh run list`
pub fn gh_run_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let mut argv = vec!["run".into(), "list".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh pr diff`（只读）
pub fn gh_pr_diff(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let num = match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= 999_999 => n.to_string(),
        _ => return "错误：缺少或非法 number".to_string(),
    };
    let mut argv = vec!["pr".into(), "diff".into(), num];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if v.get("patch").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--patch".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh run view`（日志/摘要；输出受 `command_max_output_len` 截断）
pub fn gh_run_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let run_id = match v.get("run_id").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 run_id".to_string(),
    };
    if let Err(e) = validate_run_id(run_id) {
        return e;
    }
    let mut argv = vec!["run".into(), "view".into(), run_id.to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if v.get("log").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--log".into());
        if let Some(j) = v.get("job").and_then(|x| x.as_str()) {
            if let Err(e) = validate_job_name(j) {
                return e;
            }
            argv.push("--job".into());
            argv.push(j.trim().to_string());
        }
    }
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh release list`
pub fn gh_release_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let mut argv = vec!["release".into(), "list".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh release view`
pub fn gh_release_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let tag = match v.get("tag").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 tag".to_string(),
    };
    if let Err(e) = validate_release_tag(tag) {
        return e;
    }
    let mut argv = vec!["release".into(), "view".into(), tag.to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh search`（仅允许 issues / prs / repos）
pub fn gh_search(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let scope = match v.get("scope").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 scope".to_string(),
    };
    if !matches!(scope, "issues" | "prs" | "repos") {
        return "错误：scope 须为 issues、prs 或 repos".to_string();
    }
    let q = match v.get("query").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return "错误：缺少 query".to_string(),
    };
    if let Err(e) = validate_search_query(q) {
        return e;
    }
    let mut argv = vec!["search".into(), scope.into(), q.trim().to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if scope != "repos" {
            if let Err(e) = validate_repo(r) {
                return e;
            }
            argv.push("--repo".into());
            argv.push(r.trim().to_string());
        } else {
            return "错误：scope=repos 时不要使用 repo 参数".to_string();
        }
    }
    let lim = clamp_search_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh api`（受限 path + 方法；可选 JSON body 经 stdin 传入）
pub fn gh_api(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v: JsonValue = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：JSON 解析失败：{e}"),
    };
    let path = match v.get("path").and_then(|x| x.as_str()) {
        Some(p) => p.trim(),
        None => return "错误：缺少 path".to_string(),
    };
    if let Err(e) = validate_api_path(path) {
        return e;
    }
    let method = v
        .get("method")
        .and_then(|x| x.as_str())
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PATCH" | "PUT" | "DELETE"
    ) {
        return "错误：method 须为 GET、HEAD、POST、PATCH、PUT 或 DELETE".to_string();
    }
    let body = v.get("body").and_then(|x| x.as_str()).map(str::trim);
    if let Some(b) = body {
        if !b.is_empty() && method == "GET" {
            return "错误：GET 请求不应带 body".to_string();
        }
        if !b.is_empty() && serde_json::from_str::<JsonValue>(b).is_err() {
            return "错误：body 须为合法 JSON 字符串".to_string();
        }
    }
    let mut extra: Vec<String> = Vec::new();
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        extra = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
    }

    let mut cmd = Command::new("gh");
    cmd.arg("api");
    if method != "GET" {
        cmd.arg("--method").arg(&method);
    }
    cmd.arg(path);
    cmd.args(&extra);
    cmd.current_dir(working_dir);
    let body_nonempty = body.is_some_and(|b| !b.is_empty());
    if body_nonempty {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return format!("错误：无法启动 gh：{e}"),
    };
    if body_nonempty {
        let b = body.unwrap_or("");
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b.as_bytes());
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return format!("错误：等待 gh 结束失败：{e}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = out.status.code().unwrap_or(-1);

    let mut result = format!("退出码：{code}\n");
    if !stdout.is_empty() {
        result.push_str("标准输出：\n");
        result.push_str(stdout.as_ref());
        if !stdout.ends_with('\n') {
            result.push('\n');
        }
    }
    if !stderr.is_empty() {
        result.push_str("标准错误：\n");
        result.push_str(stderr.as_ref());
        if !stderr.ends_with('\n') {
            result.push('\n');
        }
    }
    if stdout.is_empty() && stderr.is_empty() && out.status.success() {
        result.push_str("(无输出)");
    }

    let truncated = output_util::truncate_output_lines(&result, max_output_len, TRUNCATE_LINES);
    attach_json_if_exit_zero(truncated, stdout.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        vec!["gh".into()]
    }

    #[test]
    fn validate_repo_rejects_absolute() {
        assert!(validate_repo("/a/b").is_err());
        assert!(validate_repo("a/../b").is_err());
        assert!(validate_repo("o/r").is_ok());
    }

    #[test]
    fn validate_api_path_cases() {
        assert!(validate_api_path("repos/foo/bar/issues").is_ok());
        assert!(validate_api_path("/repos/x").is_err());
        assert!(validate_api_path("repos/../x").is_err());
    }

    #[test]
    fn attach_json_if_exit_zero_appends_on_json_stdout() {
        let raw = "退出码：0\n标准输出：\n[1,2]\n".to_string();
        let out = attach_json_if_exit_zero(raw, "[1,2]");
        assert!(out.contains("解析后的 JSON"), "{}", out);
    }

    #[test]
    fn attach_json_skips_on_nonzero_exit() {
        let raw = "退出码：1\n标准输出：\n{}\n".to_string();
        let out = attach_json_if_exit_zero(raw, "{}");
        assert!(!out.contains("解析后的 JSON"), "{}", out);
    }

    #[test]
    fn validate_search_query_rejects_shell_chars() {
        assert!(validate_search_query("foo;rm").is_err());
        assert!(validate_search_query("repo:foo/bar").is_ok());
    }

    #[test]
    fn validate_run_id_numeric() {
        assert!(validate_run_id("12345").is_ok());
        assert!(validate_run_id("12a").is_err());
    }

    #[test]
    fn gh_pr_list_requires_gh_in_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_list("{}", 4096, &[], dir.path());
        assert!(out.contains("未包含 gh"), "{}", out);
    }

    #[test]
    fn gh_pr_list_invokes_gh_or_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_list(
            r#"{"limit":1,"fields":["number","title"]}"#,
            8192,
            &allowed(),
            dir.path(),
        );
        assert!(
            out.contains("退出码：") || out.contains("无法执行") || out.contains("不存在"),
            "unexpected: {out}"
        );
    }
}
