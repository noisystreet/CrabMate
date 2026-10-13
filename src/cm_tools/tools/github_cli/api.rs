//! `gh api` 子命令实现（与 `run_command` 封装分离以降低 `mod.rs` 体量）。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value as JsonValue;

use super::{attach_json_if_exit_zero, gh_allowed, validate_api_path, validate_extra_args};
use crate::cm_tools::tools::output_util;
use crate::cm_tools::tools::parse_args_json;

const TRUNCATE_LINES: usize = 500;

fn validate_gh_api_json(v: &JsonValue) -> Result<(), String> {
    let path = match v.get("path").and_then(|x| x.as_str()) {
        Some(p) => p.trim(),
        None => return Err("错误：缺少 path".to_string()),
    };
    validate_api_path(path)?;
    let method = gh_api_method(v);
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PATCH" | "PUT" | "DELETE"
    ) {
        return Err("错误：method 须为 GET、HEAD、POST、PATCH、PUT 或 DELETE".to_string());
    }
    validate_gh_api_body(v.get("body").and_then(|x| x.as_str()).map(str::trim), &method)?;
    gh_api_query_pairs(v)?;
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        validate_extra_args(&extra)?;
    }
    Ok(())
}

/// 校验 `gh api` 的 body：GET 不允许 body，非空 body 须为合法 JSON。
fn validate_gh_api_body(body: Option<&str>, method: &str) -> Result<(), String> {
    let Some(b) = body else {
        return Ok(());
    };
    if !b.is_empty() && method == "GET" {
        return Err("错误：GET 请求不应带 body".to_string());
    }
    if !b.is_empty() && serde_json::from_str::<JsonValue>(b).is_err() {
        return Err("错误：body 须为合法 JSON 字符串".to_string());
    }
    Ok(())
}

fn gh_api_method(v: &JsonValue) -> String {
    v.get("method")
        .and_then(|x| x.as_str())
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase()
}

fn gh_api_extra_args(v: &JsonValue) -> Vec<String> {
    v.get("extra_args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// 将 `query` 对象解析为有序 `(key, value)` 对，供 [`build_gh_api_argv`] 转成 `-f key=value`。
///
/// `query` 缺失或为 `null` 返回空；非对象报错。键须非空且仅 `字母数字 + - _ .`；
/// 值须为字符串/数字/布尔，且不得含 `\0` / `\n` / `\r`。
fn gh_api_query_pairs(v: &JsonValue) -> Result<Vec<(String, String)>, String> {
    let Some(q) = v.get("query") else {
        return Ok(Vec::new());
    };
    if q.is_null() {
        return Ok(Vec::new());
    }
    let obj = q
        .as_object()
        .ok_or_else(|| "错误：query 须为对象（如 {\"state\":\"open\"}）".to_string())?;
    let mut out = Vec::with_capacity(obj.len());
    for (k, val) in obj {
        let key = k.trim();
        if key.is_empty() {
            return Err("错误：query 键不能为空".to_string());
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        {
            return Err(format!("错误：query 键 {key:?} 仅允许字母数字与 - _ ."));
        }
        out.push((key.to_string(), query_scalar_to_string(key, val)?));
    }
    Ok(out)
}

/// `query` 值转为字符串：仅接受字符串/数字/布尔；拒绝换行与空字符。
fn query_scalar_to_string(key: &str, val: &JsonValue) -> Result<String, String> {
    let s = match val {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        _ => return Err(format!("错误：query 值 {key:?} 须为字符串、数字或布尔")),
    };
    if s.contains('\0') || s.contains('\n') || s.contains('\r') {
        return Err(format!("错误：query 值 {key:?} 不得含换行或空字符"));
    }
    Ok(s)
}

/// 构造 `gh api` 之后的 argv。
///
/// **始终**显式传 `--method`：`gh` 见到 `-f`/`-F` 时会默认改用 POST，
/// 只有显式传 `--method GET` 才能保证 GET 不被翻转。`query` 转为
/// `-f key=value`（位于 `extra_args` 之前）。
fn build_gh_api_argv(
    method: &str,
    path: &str,
    query: &[(String, String)],
    extra: &[String],
) -> Vec<String> {
    let mut argv = Vec::with_capacity(3 + query.len() * 2 + extra.len());
    argv.push("--method".to_string());
    argv.push(method.to_string());
    argv.push(path.to_string());
    for (k, v) in query {
        argv.push("-f".to_string());
        argv.push(format!("{k}={v}"));
    }
    argv.extend(extra.iter().cloned());
    argv
}

fn format_gh_api_process_output(out: std::process::Output, max_output_len: usize) -> String {
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
    let v = match parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    if let Err(e) = validate_gh_api_json(&v) {
        return e;
    }
    let path = v.get("path").and_then(|x| x.as_str()).expect("validated");
    let method = gh_api_method(&v);
    let body = v.get("body").and_then(|x| x.as_str()).map(str::trim);
    let query = gh_api_query_pairs(&v).unwrap_or_default();
    let extra = gh_api_extra_args(&v);
    let argv = build_gh_api_argv(&method, path, &query, &extra);

    let mut cmd = Command::new("gh");
    crate::cm_tools::github_token::apply_gh_token_env(&mut cmd);
    cmd.arg("api");
    cmd.args(&argv);
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
        Err(e) => {
            let base = format!("错误：无法启动 gh：{e}");
            return output_util::append_notfound_install_hint(base, &e, "gh");
        }
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
    format_gh_api_process_output(out, max_output_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_argv_always_passes_method_even_for_get() {
        let argv = build_gh_api_argv("GET", "repos/o/r/issues", &[], &[]);
        assert_eq!(argv, vec!["--method", "GET", "repos/o/r/issues"]);
    }

    #[test]
    fn build_argv_serializes_query_before_extra() {
        let query = vec![("state".to_string(), "open".to_string())];
        let extra = vec!["--paginate".to_string()];
        let argv = build_gh_api_argv("GET", "repos/o/r/issues", &query, &extra);
        assert_eq!(
            argv,
            vec![
                "--method",
                "GET",
                "repos/o/r/issues",
                "-f",
                "state=open",
                "--paginate",
            ]
        );
    }

    #[test]
    fn query_pairs_accepts_scalars() {
        let v = json!({"query": {"state": "open", "per_page": 50, "all": true}});
        let mut pairs = gh_api_query_pairs(&v).unwrap();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("all".to_string(), "true".to_string()),
                ("per_page".to_string(), "50".to_string()),
                ("state".to_string(), "open".to_string()),
            ]
        );
    }

    #[test]
    fn query_pairs_missing_or_null_is_empty() {
        assert!(gh_api_query_pairs(&json!({})).unwrap().is_empty());
        assert!(gh_api_query_pairs(&json!({"query": null})).unwrap().is_empty());
    }

    #[test]
    fn query_pairs_rejects_non_object_and_bad_key() {
        assert!(gh_api_query_pairs(&json!({"query": ["a=b"]})).is_err());
        assert!(gh_api_query_pairs(&json!({"query": {"a b": "x"}})).is_err());
        assert!(gh_api_query_pairs(&json!({"query": {"": "x"}})).is_err());
    }

    #[test]
    fn query_pairs_rejects_non_scalar_and_control_chars() {
        assert!(gh_api_query_pairs(&json!({"query": {"a": {"b": 1}}})).is_err());
        assert!(gh_api_query_pairs(&json!({"query": {"a": "x\ny"}})).is_err());
    }
}

