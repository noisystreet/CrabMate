//! 只读环境/工具链诊断摘要：不输出密钥与敏感变量的值或长度（仅 未设置/空/非空）。

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::file;

/// 与 `config/mod.rs`、README、AGENTS 中常见项对齐；按字母序输出。
const TRACKED_ENV_VARS: &[&str] = &[
    "AGENT_ALLOWED_COMMANDS",
    "AGENT_API_BASE",
    "AGENT_API_MAX_RETRIES",
    "AGENT_API_RETRY_DELAY_SECS",
    "AGENT_API_TIMEOUT_SECS",
    "AGENT_CHAT_QUEUE_MAX_CONCURRENT",
    "AGENT_CHAT_QUEUE_MAX_PENDING",
    "AGENT_COMMAND_MAX_OUTPUT_LEN",
    "AGENT_COMMAND_TIMEOUT_SECS",
    "AGENT_CONTEXT_CHAR_BUDGET",
    "AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM",
    "AGENT_CONTEXT_SUMMARY_MAX_TOKENS",
    "AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES",
    "AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS",
    "AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS",
    "AGENT_CURSOR_RULES_DIR",
    "AGENT_CURSOR_RULES_ENABLED",
    "AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD",
    "AGENT_CURSOR_RULES_MAX_CHARS",
    "AGENT_FINAL_PLAN_REQUIREMENT",
    "AGENT_HTTP_HOST",
    "AGENT_MAX_MESSAGE_HISTORY",
    "AGENT_MAX_TOKENS",
    "AGENT_MODEL",
    "AGENT_PLAN_REWRITE_MAX_ATTEMPTS",
    "AGENT_REFLECTION_DEFAULT_MAX_ROUNDS",
    "AGENT_RUN_COMMAND_WORKING_DIR",
    "AGENT_SYSTEM_PROMPT",
    "AGENT_SYSTEM_PROMPT_FILE",
    "AGENT_TEMPERATURE",
    "AGENT_TOOL_MESSAGE_MAX_CHARS",
    "AGENT_WEATHER_TIMEOUT_SECS",
    "AGENT_WEB_SEARCH_API_KEY",
    "AGENT_WEB_SEARCH_MAX_RESULTS",
    "AGENT_WEB_SEARCH_PROVIDER",
    "AGENT_WEB_SEARCH_TIMEOUT_SECS",
    "API_KEY",
    "RUST_BACKTRACE",
    "RUST_LOG",
];

fn is_strict_secret_env(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    if matches!(
        u.as_str(),
        "API_KEY" | "AGENT_WEB_SEARCH_API_KEY" | "AGENT_SYSTEM_PROMPT"
    ) {
        return true;
    }
    if u.ends_with("_API_KEY") || u.ends_with("_SECRET") || u.ends_with("_SECRETS") {
        return true;
    }
    if u.ends_with("_TOKEN") || u.contains("PASSWORD") || u.contains("PRIVATE_KEY") {
        return true;
    }
    if u.contains("BEARER") || u == "AUTHORIZATION" || u == "AUTH" {
        return true;
    }
    // 代理 URL 常带账号口令
    if u.ends_with("PROXY") && u != "NO_PROXY" {
        return true;
    }
    false
}

fn env_presence_line(name: &str, val: Result<String, std::env::VarError>) -> String {
    if is_strict_secret_env(name) {
        match val {
            Err(std::env::VarError::NotPresent) => format!("  {}: 未设置", name),
            Err(std::env::VarError::NotUnicode(_)) => {
                format!("  {}: 已设置(非 Unicode，不展示)", name)
            }
            Ok(s) if s.is_empty() => format!("  {}: 已设置(空)", name),
            Ok(_) => format!("  {}: 已设置(非空，值已隐藏)", name),
        }
    } else {
        match val {
            Err(std::env::VarError::NotPresent) => format!("  {}: 未设置", name),
            Err(std::env::VarError::NotUnicode(_)) => {
                format!("  {}: 已设置(非 Unicode，不展示)", name)
            }
            Ok(s) if s.is_empty() => format!("  {}: 已设置(空)", name),
            Ok(s) => format!("  {}: 已设置(非空, {} 字符，值已隐藏)", name, s.len()),
        }
    }
}

/// 供 CLI `doctor` 等复用：执行命令并取 stdout 首段非空文本。
pub(crate) fn capture_trimmed(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn parse_rustc_vv(text: &str) -> (Option<String>, Option<String>) {
    let mut host = None;
    let mut release = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("host: ") {
            host = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("release: ") {
            release = Some(v.trim().to_string());
        }
    }
    (host, release)
}

fn is_safe_extra_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// 参数 JSON（均可选）：`include_toolchain`（默认 true）、`include_workspace_paths`（默认 true）、`include_env`（默认 true）、`extra_env_vars`（额外大写变量名数组，仅允许 `[A-Z0-9_]+`）
pub fn diagnostic_summary(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let inc_tc = v
        .get("include_toolchain")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let inc_paths = v
        .get("include_workspace_paths")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let inc_env = v
        .get("include_env")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let extra: Vec<String> = v
        .get("extra_env_vars")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .filter(|s| is_safe_extra_env_name(s))
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut out = String::from("诊断摘要（只读，已脱敏；不输出任何环境变量取值）\n");

    if inc_paths {
        out.push_str("【工作区路径】\n");
        match file::canonical_workspace_root(working_dir) {
            Ok(base) => {
                out.push_str(&format!("  根目录: {}\n", base.display()));
                let target = base.join("target");
                out.push_str(&format!(
                    "  target/: {}\n",
                    if target.is_dir() {
                        "存在(目录)"
                    } else {
                        "不存在或非目录"
                    }
                ));
                for rel in [
                    "Cargo.toml",
                    "frontend-leptos/Trunk.toml",
                    "frontend-leptos/dist",
                ] {
                    let p = base.join(rel);
                    let st = if p.is_file() {
                        "文件存在"
                    } else if p.is_dir() {
                        "目录存在"
                    } else {
                        "不存在"
                    };
                    out.push_str(&format!("  {}: {}\n", rel, st));
                }
            }
            Err(e) => {
                out.push_str(&format!("  无法解析工作区根: {}\n", e));
            }
        }
        out.push('\n');
    }

    if inc_tc {
        out.push_str("【Rust 工具链】\n");
        if let Some(s) = capture_trimmed("rustc", &["-V"]) {
            out.push_str(&format!("  rustc -V: {}\n", s));
        } else {
            out.push_str("  rustc -V: 无法执行或失败\n");
        }
        if let Some(s) = capture_trimmed("cargo", &["-V"]) {
            out.push_str(&format!("  cargo -V: {}\n", s));
        } else {
            out.push_str("  cargo -V: 无法执行或失败\n");
        }
        if let Some(text) = capture_trimmed("rustc", &["-vV"]) {
            let (host, release) = parse_rustc_vv(&text);
            if let Some(h) = host {
                out.push_str(&format!("  host triple: {}\n", h));
            }
            if let Some(r) = release {
                out.push_str(&format!("  rustc release: {}\n", r));
            }
        }
        if let Some(s) = capture_trimmed("rustup", &["default"]) {
            let line = s.lines().next().unwrap_or(&s).trim();
            out.push_str(&format!("  rustup default: {}\n", line));
        } else {
            out.push_str("  rustup default: 不可用或未安装\n");
        }
        match Command::new("bc").arg("--version").output() {
            Ok(o) if o.status.success() => {
                let v = String::from_utf8_lossy(&o.stdout);
                let line = v.lines().next().unwrap_or("").trim();
                out.push_str(&format!(
                    "  bc(calc 工具): 可用{}\n",
                    if line.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", line)
                    }
                ));
            }
            _ => out.push_str("  bc(calc 工具): 不可用（/health 可能报 dep_bc 降级）\n"),
        }
        if let Some(s) = capture_trimmed("gh", &["version"]) {
            let line = s.lines().next().unwrap_or(&s).trim();
            out.push_str(&format!("  gh(GitHub CLI): 可用 ({line})\n"));
        } else {
            out.push_str(
                "  gh(GitHub CLI): 不可用（/health 可能报 dep_gh 降级；默认 run_command 白名单含 gh）\n",
            );
        }
        out.push_str(&format!(
            "  平台: {} / {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        out.push('\n');
    }

    if inc_env {
        out.push_str("【环境变量（仅状态）】\n");
        out.push_str(
            "  说明: 密钥类变量不报告长度；其余变量仅报告是否已设置及字符数，永不输出内容。\n",
        );
        for name in TRACKED_ENV_VARS {
            out.push_str(&env_presence_line(name, std::env::var(name)));
            out.push('\n');
        }
        let mut seen: BTreeSet<String> = TRACKED_ENV_VARS.iter().map(|s| s.to_string()).collect();
        for name in extra {
            if seen.insert(name.clone()) {
                out.push_str(&env_presence_line(&name, std::env::var(&name)));
                out.push('\n');
            }
        }
        out.push('\n');
    }

    out.push_str("提示: 与 AGENTS.md 中「API_KEY 未设置时 chat 失败」等说明对照。\n");
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names() {
        assert!(is_strict_secret_env("API_KEY"));
        assert!(is_strict_secret_env("AGENT_WEB_SEARCH_API_KEY"));
        assert!(is_strict_secret_env("FOO_API_KEY"));
        assert!(is_strict_secret_env("HTTPS_PROXY"));
        assert!(!is_strict_secret_env("NO_PROXY"));
        assert!(!is_strict_secret_env("AGENT_MODEL"));
    }

    #[test]
    fn extra_name_filter() {
        assert!(!is_safe_extra_env_name("API-KEY"));
        assert!(is_safe_extra_env_name("MY_VAR_1"));
    }
}
