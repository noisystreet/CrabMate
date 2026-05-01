//! `gh api` 子命令实现（与 `run_command` 封装分离以降低 `mod.rs` 体量）。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value as JsonValue;

use super::{attach_json_if_exit_zero, gh_allowed, validate_api_path, validate_extra_args};
use crate::tools::output_util;
use crate::tools::parse_args_json;

const TRUNCATE_LINES: usize = 500;

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
