use std::path::Path;

use serde_json::Value as JsonValue;

use crate::tools::command;

pub const MAX_LIMIT: u32 = 200;
pub const DEFAULT_LIST_LIMIT: u32 = 30;
pub const MAX_SEARCH_LIMIT: u32 = 100;
pub const MAX_SEARCH_QUERY_BYTES: usize = 400;
pub const MAX_RELEASE_TAG_LEN: usize = 200;
pub const MAX_JOB_NAME_LEN: usize = 128;
pub const MAX_PR_TITLE_BYTES: usize = 240;
pub const MAX_PR_BODY_BYTES: usize = 65_536;
pub const MAX_PR_REF_TOKEN_BYTES: usize = 200;

pub fn is_safe_token(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && !t.contains("..") && !t.starts_with('/')
}

pub fn validate_repo(repo: &str) -> Result<(), String> {
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

pub fn validate_extra_args(args: &[String]) -> Result<(), String> {
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

pub fn validate_api_path(path: &str) -> Result<(), String> {
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

pub fn gh_allowed(allowed: &[String]) -> Result<(), String> {
    if allowed.iter().any(|c| c == "gh") {
        Ok(())
    } else {
        Err("错误：当前配置 allowed_commands 未包含 gh（可在 config/tools.toml 或 CM_ALLOWED_COMMANDS 中加入）".to_string())
    }
}

pub fn join_json_fields(fields: &[String]) -> Result<String, String> {
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

pub fn clamp_limit(n: Option<u32>) -> u32 {
    n.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIMIT)
}

pub fn try_pretty_json(stdout: &str) -> Option<String> {
    let t = stdout.trim();
    if t.is_empty() {
        return None;
    }
    let v: JsonValue = serde_json::from_str(t).ok()?;
    serde_json::to_string_pretty(&v).ok()
}

pub fn wrap_with_parsed(raw: String, stdout: &str) -> String {
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
pub fn extract_stdout_from_formatted(out: &str) -> &str {
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
pub fn attach_json_if_exit_zero(formatted: String, stdout_raw: &str) -> String {
    let first = formatted.lines().next().unwrap_or("");
    let code = first
        .strip_prefix("退出码：")
        .and_then(|s| s.trim().parse::<i32>().ok());
    if code != Some(0) {
        return formatted;
    }
    wrap_with_parsed(formatted, stdout_raw)
}

pub fn run_gh_vec(
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

pub fn validate_run_id(id: &str) -> Result<(), String> {
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

pub fn validate_release_tag(tag: &str) -> Result<(), String> {
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

pub fn validate_search_query(q: &str) -> Result<(), String> {
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

pub fn validate_pr_title(title: &str) -> Result<(), String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("错误：title 不能为空".to_string());
    }
    if t.len() > MAX_PR_TITLE_BYTES {
        return Err(format!(
            "错误：title 过长（上限 {} 字节）",
            MAX_PR_TITLE_BYTES
        ));
    }
    if t.contains('\0') || t.contains('\n') || t.contains('\r') {
        return Err("错误：title 不得含换行或空字符".to_string());
    }
    Ok(())
}

pub fn validate_pr_body(body: &str) -> Result<(), String> {
    if body.len() > MAX_PR_BODY_BYTES {
        return Err(format!(
            "错误：body 过长（上限 {} 字节）",
            MAX_PR_BODY_BYTES
        ));
    }
    if body.contains('\0') {
        return Err("错误：body 不得含空字符".to_string());
    }
    Ok(())
}

/// `--base` / `--head` 传给 `gh pr create` 的单个 token（分支名或 `owner:branch` 等）。
pub fn validate_pr_ref_token(token: &str) -> Result<(), String> {
    let t = token.trim();
    if t.is_empty() {
        return Err("错误：base/head 不能为空".to_string());
    }
    if t.len() > MAX_PR_REF_TOKEN_BYTES {
        return Err(format!(
            "错误：base/head 过长（上限 {} 字节）",
            MAX_PR_REF_TOKEN_BYTES
        ));
    }
    if t.contains("..") || t.starts_with('/') {
        return Err("错误：base/head 不得含 \"..\" 或以 \"/\" 开头".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
    {
        return Err(
            "错误：base/head 仅允许字母数字与 - _ . / : @（与常见分支 / fork:branch 写法一致）"
                .to_string(),
        );
    }
    Ok(())
}

pub fn validate_job_name(name: &str) -> Result<(), String> {
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

pub fn clamp_search_limit(n: Option<u32>) -> u32 {
    n.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_SEARCH_LIMIT)
}

pub fn write_workspace_temp_markdown(
    working_dir: &Path,
    filename: &str,
    content: &[u8],
    err_label: &str,
) -> Result<(tempfile::TempDir, String), String> {
    let dir = tempfile::tempdir_in(working_dir)
        .map_err(|e| format!("错误：无法在工作区内创建临时目录：{e}"))?;
    let path = dir.path().join(filename);
    std::fs::write(&path, content)
        .map_err(|e| format!("错误：写入 {err_label} 临时文件失败：{e}"))?;
    let path_str = path
        .to_str()
        .ok_or_else(|| format!("错误：{err_label} 临时文件路径非 UTF-8"))?
        .to_string();
    Ok((dir, path_str))
}

pub fn push_repo_arg(v: &JsonValue, argv: &mut Vec<String>) -> Result<(), String> {
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        validate_repo(r)?;
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    Ok(())
}

pub fn push_extra_args_from_json(v: &JsonValue, argv: &mut Vec<String>) -> Result<(), String> {
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        validate_extra_args(&extra)?;
        argv.extend(extra);
    }
    Ok(())
}

pub fn push_bool_flag(v: &JsonValue, key: &str, flag: &str, argv: &mut Vec<String>) {
    if v.get(key).and_then(|x| x.as_bool()) == Some(true) {
        argv.push(flag.into());
    }
}

pub fn push_trimmed_string_flag(v: &JsonValue, key: &str, flag: &str, argv: &mut Vec<String>) {
    if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            argv.push(flag.into());
            argv.push(t.to_string());
        }
    }
}
