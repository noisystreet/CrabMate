//! 根据**已脱敏**的构建/测试错误输出，做启发式归类并给出 2～3 条可经 `run_command` 执行的排查命令建议。
//! [`error_output_playbook`] 仅输出文本；[`playbook_run_commands`] 按序真实执行这些命令（仍受白名单与 `run_command` 规则约束）。

use std::collections::HashSet;

use super::ToolContext;
use super::command;

use log::warn;
use regex::Regex;

const DEFAULT_MAX_CHARS: usize = 24_000;
const ABS_MAX_CHARS: usize = 100_000;
const MAX_SNIPPET_LINES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ecosystem {
    Auto,
    Rust,
    Node,
    Python,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    Dependency,
    Network,
    Permission,
    Disk,
    SyntaxOrType,
    TestFailure,
    Other,
}

impl Ecosystem {
    fn as_label_zh(self) -> &'static str {
        match self {
            Self::Auto => "auto（自动推断）",
            Self::Rust => "rust",
            Self::Node => "node/npm",
            Self::Python => "python",
            Self::Generic => "generic",
        }
    }
}

impl Category {
    fn as_str_zh(self) -> &'static str {
        match self {
            Self::Dependency => "依赖解析 / 包管理",
            Self::Network => "网络 / 下载 / 仓库连通",
            Self::Permission => "权限 / 文件系统",
            Self::Disk => "磁盘空间",
            Self::SyntaxOrType => "语法 / 类型 / 编译",
            Self::TestFailure => "测试失败",
            Self::Other => "未细分（其它）",
        }
    }
}

fn parse_ecosystem(s: &str) -> Result<Ecosystem, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(Ecosystem::Auto),
        "rust" | "cargo" | "rustc" => Ok(Ecosystem::Rust),
        "node" | "npm" | "javascript" | "typescript" => Ok(Ecosystem::Node),
        "python" | "pytest" | "pip" | "uv" => Ok(Ecosystem::Python),
        "generic" => Ok(Ecosystem::Generic),
        _ => Err(format!(
            "未知 ecosystem：{}（允许 auto/rust/node/python/generic）",
            s.trim()
        )),
    }
}

fn clamp_max_chars(n: u64) -> usize {
    let n = n as usize;
    if n == 0 {
        DEFAULT_MAX_CHARS
    } else {
        n.min(ABS_MAX_CHARS)
    }
}

/// 轻度脱敏：避免把明显像「键=值」的凭证原样回显（用户仍应先自行脱敏）。
fn light_redact(text: &str) -> String {
    static RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(
            r"(?i)(?P<prefix>\b(?:api[_-]?key|secret|token|password|authorization|bearer)\b\s*[:=]\s*)(?P<val>\S+)",
        )
        .expect("error_playbook redact regex")
    });
    RE.replace_all(text, "${prefix}[已省略]").to_string()
}

fn detect_ecosystem_auto(text: &str) -> Ecosystem {
    let t = text.to_ascii_lowercase();
    if t.contains("pytest")
        || t.contains("python ")
        || t.contains("pip install")
        || t.contains("modulenotfounderror")
        || t.contains("traceback (most recent call last)")
    {
        return Ecosystem::Python;
    }
    if t.contains("npm err")
        || t.contains("yarn error")
        || t.contains("pnpm")
        || t.contains("eslint")
        || t.contains("typescript error ts")
    {
        return Ecosystem::Node;
    }
    if t.contains("error[e")
        || t.contains("cargo:")
        || t.contains("rustc ")
        || t.contains("could not compile")
    {
        return Ecosystem::Rust;
    }
    Ecosystem::Generic
}

fn effective_ecosystem(requested: Ecosystem, text: &str) -> Ecosystem {
    match requested {
        Ecosystem::Auto => detect_ecosystem_auto(text),
        e => e,
    }
}

fn detect_category(text: &str, eco: Ecosystem) -> Category {
    let t = text.to_ascii_lowercase();
    if t.contains("no space left on device")
        || t.contains("enospc")
        || t.contains("disk quota exceeded")
    {
        return Category::Disk;
    }
    if t.contains("permission denied")
        || t.contains("eacces")
        || t.contains("eperm")
        || t.contains("operation not permitted")
    {
        return Category::Permission;
    }
    if t.contains("connection refused")
        || t.contains("econnrefused")
        || t.contains("connection timed out")
        || t.contains("etimedout")
        || t.contains("could not resolve host")
        || t.contains("enotfound")
        || t.contains("tls handshake")
        || t.contains("ssl certificate")
        || t.contains("certificate verify failed")
    {
        return Category::Network;
    }
    if matches!(eco, Ecosystem::Rust | Ecosystem::Generic | Ecosystem::Auto)
        && (t.contains("could not find `")
            || t.contains("failed to select a version")
            || t.contains("no matching package named")
            || t.contains("failed to resolve")
            || t.contains("unresolved import"))
    {
        return Category::Dependency;
    }
    if t.contains("modulenotfounderror")
        || t.contains("no module named")
        || t.contains("cannot find module")
        || (t.contains("npm err!")
            && (t.contains("enoent") || t.contains("not found") || t.contains("missing")))
    {
        return Category::Dependency;
    }
    if t.contains("failed") && (t.contains("assertion") || t.contains("test result:"))
        || t.contains("assertionerror")
        || t.contains("==== failures ====")
    {
        return Category::TestFailure;
    }
    if t.contains("error[e")
        || t.contains("error: could not compile")
        || t.contains("syntax error")
        || t.contains("parse error")
        || t.contains("unexpected token")
        || t.contains("typeerror")
        || t.contains("referenceerror")
    {
        return Category::SyntaxOrType;
    }
    Category::Other
}

fn first_rust_error_code(text: &str) -> Option<String> {
    static RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"error\[E([0-9]{4,5})\]").expect("E-code regex"));
    RE.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| format!("E{}", m.as_str()))
}

fn cmd_allowed(cmd: &str, allowed: &HashSet<String>) -> bool {
    let bin = cmd
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if bin.is_empty() {
        return false;
    }
    allowed.contains(&bin)
}

fn push_suggestion(out: &mut Vec<String>, allowed: &HashSet<String>, s: &str) {
    if out.len() < 3 && cmd_allowed(s, allowed) && !out.iter().any(|x| x == s) {
        out.push(s.to_string());
    }
}

fn collect_suggestions(
    eco: Ecosystem,
    cat: Category,
    text: &str,
    allowed: &HashSet<String>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let ecode = first_rust_error_code(text);

    match eco {
        Ecosystem::Rust | Ecosystem::Generic => match cat {
            Category::Dependency => {
                push_suggestion(&mut out, allowed, "cargo tree -i");
                push_suggestion(&mut out, allowed, "cargo update");
                push_suggestion(&mut out, allowed, "cargo metadata --format-version 1");
            }
            Category::Network => {
                push_suggestion(&mut out, allowed, "cargo fetch");
                push_suggestion(&mut out, allowed, "git remote -v");
            }
            Category::SyntaxOrType | Category::Other => {
                push_suggestion(&mut out, allowed, "cargo check --message-format=short");
                push_suggestion(&mut out, allowed, "cargo build --message-format=short");
                if let Some(ref e) = ecode {
                    let explain = format!("rustc --explain {}", e);
                    push_suggestion(&mut out, allowed, &explain);
                }
            }
            Category::TestFailure => {
                push_suggestion(&mut out, allowed, "cargo test -- --nocapture");
                push_suggestion(&mut out, allowed, "cargo test");
            }
            Category::Permission | Category::Disk => {
                push_suggestion(&mut out, allowed, "ls -la");
                push_suggestion(&mut out, allowed, "df -h");
            }
        },
        Ecosystem::Python => match cat {
            Category::Dependency => {
                push_suggestion(&mut out, allowed, "python3 -m pip list");
            }
            Category::TestFailure => {
                push_suggestion(&mut out, allowed, "python3 -m pytest --tb=short -q");
            }
            _ => {
                push_suggestion(&mut out, allowed, "python3 -m pytest --tb=short -q");
                push_suggestion(&mut out, allowed, "python3 -m pip list");
            }
        },
        Ecosystem::Node => match cat {
            Category::Dependency => {
                push_suggestion(&mut out, allowed, "npm ls");
            }
            _ => {
                push_suggestion(&mut out, allowed, "npm run build");
                push_suggestion(&mut out, allowed, "npm ls");
            }
        },
        // `effective_ecosystem` 在调用方已将 Auto 解析为具体栈
        Ecosystem::Auto => {}
    }

    // 通用兜底（若白名单有 cargo/git）
    if out.is_empty() {
        if matches!(cat, Category::Permission | Category::Disk) {
            push_suggestion(&mut out, allowed, "ls -la");
            push_suggestion(&mut out, allowed, "df -h");
        } else {
            push_suggestion(&mut out, allowed, "cargo check --message-format=short");
            push_suggestion(&mut out, allowed, "git status");
        }
    }

    out.truncate(3);
    out
}

fn snippet_lines(text: &str, max_lines: usize) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 从参数 JSON 解析并截断错误文本，返回（截断后正文、有效生态、请求生态、归类）。
fn playbook_prepare(
    v: &serde_json::Value,
) -> Result<(String, Ecosystem, Ecosystem, Category), String> {
    let raw = match v.get("error_text").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return Err(
                "错误：缺少非空 error_text（请先脱敏，勿粘贴 API Key、token、完整 Authorization 等）"
                    .to_string(),
            );
        }
    };
    let max_chars = v
        .get("max_chars")
        .and_then(|x| x.as_u64())
        .map(clamp_max_chars)
        .unwrap_or(DEFAULT_MAX_CHARS);
    let eco_req = match v.get("ecosystem").and_then(|x| x.as_str()) {
        Some(s) => parse_ecosystem(s)?,
        None => Ecosystem::Auto,
    };

    let text = light_redact(raw);
    let truncated = if text.len() > max_chars {
        format!(
            "{}…\n\n（输入已截断至约 {} 字符；可增大 max_chars 或缩短粘贴）",
            &text[..max_chars],
            max_chars
        )
    } else {
        text
    };

    let eco = effective_ecosystem(eco_req, &truncated);
    let cat = detect_category(&truncated, eco);
    Ok((truncated, eco, eco_req, cat))
}

/// 参数：`error_text`（必填）、`ecosystem`（可选，默认 auto）、`max_chars`（可选，默认 24000，上限 100000）
pub fn error_output_playbook(args_json: &str, allowed_commands: &[String]) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (truncated, eco, eco_req, cat) = match playbook_prepare(&v) {
        Ok(x) => x,
        Err(msg) => return msg,
    };

    let allowed_set: HashSet<String> = allowed_commands
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let suggestions = collect_suggestions(eco, cat, &truncated, &allowed_set);

    let mut out = String::new();
    out.push_str("错误输出排障建议（只读启发式，**非**执行结果；请先自行脱敏粘贴）\n\n");
    out.push_str(&format!(
        "【推断生态】{}（请求: {}）\n",
        eco.as_label_zh(),
        eco_req.as_label_zh()
    ));
    out.push_str(&format!("【归类】{}\n\n", cat.as_str_zh()));
    out.push_str("【摘录（前几行非空行）】\n");
    out.push_str(&snippet_lines(&truncated, MAX_SNIPPET_LINES));
    out.push_str("\n\n【建议下一步（经 run_command 白名单过滤；未列出的命令请用 run_command 或本机终端）】\n");
    if suggestions.is_empty() {
        out.push_str("- （当前白名单下无匹配的可建议命令；可扩大 allowed_commands 或使用 diagnostic_summary）\n");
    } else {
        for s in &suggestions {
            out.push_str(&format!("- `{}`\n", s));
        }
    }
    if let Some(e) = first_rust_error_code(&truncated) {
        out.push_str(&format!(
            "\n（检测到 Rust 错误码 {}；若白名单含 rustc，可 `rustc --explain {}`）\n",
            e, e
        ));
    }
    out.push_str("\n免责声明：以上为常见排查路径模板，不构成对具体错误的完整诊断。\n");
    out
}

fn clamp_playbook_max_commands(n: u64) -> usize {
    let n = n as usize;
    if n == 0 { 3 } else { n.min(3) }
}

/// 按 [`error_output_playbook`] 的同一启发式得到建议命令，并**依次**经 `run_command` 执行（白名单、无 `..`/绝对路径参数等规则与 `run_command` 一致）。
///
/// 参数：与 `error_output_playbook` 相同的 `error_text` / `ecosystem` / `max_chars`，另可选 `max_commands`（默认 3，范围 1～3）。
pub fn playbook_run_commands(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let max_run = v
        .get("max_commands")
        .and_then(|x| x.as_u64())
        .map(clamp_playbook_max_commands)
        .unwrap_or(3);

    let (truncated, eco, _, cat) = match playbook_prepare(&v) {
        Ok(x) => x,
        Err(msg) => return msg,
    };
    let allowed_set: HashSet<String> = ctx
        .allowed_commands
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let suggestions = collect_suggestions(eco, cat, &truncated, &allowed_set);
    if suggestions.is_empty() {
        return "（当前白名单下无可用诊断命令；可扩大 allowed_commands 或先使用 error_output_playbook 查看归类）".to_string();
    }
    let to_run: Vec<&String> = suggestions.iter().take(max_run).collect();

    let mut out = String::new();
    out.push_str("playbook_run_commands：已按 error_output_playbook 启发式顺序执行下列命令（每条均为独立 run_command）。\n\n");

    for (i, line) in to_run.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let mut parts = t.split_whitespace();
        let Some(bin) = parts.next() else {
            continue;
        };
        let args: Vec<String> = parts.map(String::from).collect();
        let payload = serde_json::json!({
            "command": bin,
            "args": args,
        });
        let args_s = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(e) => {
                out.push_str(&format!("—— 步骤 {} ——\n序列化失败: {}\n\n", i + 1, e));
                continue;
            }
        };
        out.push_str(&format!("—— 步骤 {}: `{}` ——\n", i + 1, t));
        let r = match command::run_checked(
            &args_s,
            ctx.command_max_output_len,
            ctx.allowed_commands,
            ctx.working_dir,
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    target: "crabmate",
                    "playbook_run_commands step={} kind={} err={}",
                    i + 1,
                    e.kind(),
                    e
                );
                e.user_message()
            }
        };
        out.push_str(&r);
        out.push_str("\n\n");
    }
    out.push_str("提示：以上为真实命令输出；若需仅看建议不执行，请用 error_output_playbook。\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev_allowlist() -> Vec<String> {
        vec![
            "cargo".into(),
            "rustc".into(),
            "git".into(),
            "ls".into(),
            "df".into(),
            "python3".into(),
            "npm".into(),
            "echo".into(),
        ]
    }

    fn ctx_with_allow<'a>(allowed: &'a [String]) -> super::super::ToolContext<'a> {
        super::super::ToolContext {
            codebase_semantic: None,
            command_max_output_len: 4096,
            weather_timeout_secs: 15,
            allowed_commands: allowed,
            working_dir: std::path::Path::new("."),
            web_search_timeout_secs: 15,
            web_search_provider: crate::config::WebSearchProvider::Brave,
            web_search_api_key: "",
            web_search_max_results: 5,
            http_fetch_allowed_prefixes: &[] as &[String],
            http_fetch_timeout_secs: 30,
            http_fetch_max_response_bytes: 8192,
            command_timeout_secs: 30,
            read_file_turn_cache: None,
            workspace_changelist: None,
            test_result_cache_enabled: false,
            test_result_cache_max_entries: 8,
        }
    }

    #[test]
    fn rust_e0599_suggests_cargo_check() {
        let text = "error[E0599]: no method named `foo` found\n  --> src/lib.rs:1:1";
        let out = error_output_playbook(
            &serde_json::json!({"error_text": text}).to_string(),
            &dev_allowlist(),
        );
        assert!(out.contains("语法 / 类型 / 编译"));
        assert!(out.contains("cargo check"));
    }

    #[test]
    fn pytest_failure_category() {
        let text = "FAILED tests/test_x.py::test_a - AssertionError: 1 != 2";
        let out = error_output_playbook(
            &serde_json::json!({"error_text": text, "ecosystem": "python"}).to_string(),
            &dev_allowlist(),
        );
        assert!(out.contains("测试失败"));
        assert!(out.contains("pytest"));
    }

    #[test]
    fn redacts_api_key_like_line() {
        let text = "x API_KEY=sk-fake-secret-value more";
        let r = light_redact(text);
        assert!(!r.contains("sk-fake"));
        assert!(r.contains("[已省略]"));
    }

    #[test]
    fn playbook_run_commands_runs_first_suggested_command() {
        let allowed = vec!["ls".into(), "df".into()];
        let ctx = ctx_with_allow(&allowed);
        let out = super::playbook_run_commands(
            r#"{"error_text":"permission denied opening ./foo","max_commands":1}"#,
            &ctx,
        );
        assert!(out.contains("步骤 1"));
        assert!(out.contains("ls"));
    }
}
