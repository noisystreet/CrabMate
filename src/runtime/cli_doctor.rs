//! `crabmate doctor` / `models` / `probe`：面向终端的一页诊断与网关探测（输出脱敏，不打印密钥）。
//! REPL 内建 **`/doctor`**、**`/probe`**、**`/models`** 分别复用 [`print_doctor_report`]、[`run_probe_cli`]、[`run_models_cli`]（与上述子命令对齐）。

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::AgentConfig;
use crate::config::{ExposeSecret, LlmHttpAuthMode};
use crate::llm::fetch_models_report;
use crate::tools::{canonical_workspace_root, capture_trimmed};

fn resolve_workspace_dir(cfg: &AgentConfig, workspace_cli: Option<&str>) -> PathBuf {
    let raw = workspace_cli
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(cfg.run_command_working_dir.trim()));
    raw.canonicalize().unwrap_or(raw)
}

fn api_key_line(cfg: &AgentConfig) -> String {
    match std::env::var("API_KEY") {
        Err(std::env::VarError::NotPresent) => {
            if cfg.llm_http_auth_mode == LlmHttpAuthMode::None {
                "API_KEY: 未设置（llm_http_auth_mode=none 时 chat / models / probe 可不依赖密钥）"
                    .to_string()
            } else {
                "API_KEY: 未设置（llm_http_auth_mode=bearer 时 chat / models / probe 不可用）"
                    .to_string()
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => "API_KEY: 已设置(非 Unicode，不展示)".to_string(),
        Ok(s) if s.trim().is_empty() => {
            if cfg.llm_http_auth_mode == LlmHttpAuthMode::None {
                "API_KEY: 已设置但为空（llm_http_auth_mode=none 时可继续）".to_string()
            } else {
                "API_KEY: 已设置但为空".to_string()
            }
        }
        Ok(_) => "API_KEY: 已设置(非空，值已隐藏)".to_string(),
    }
}

fn path_status_line(label: &str, p: &Path) {
    let st = if p.is_file() {
        "文件存在"
    } else if p.is_dir() {
        "目录存在"
    } else {
        "不存在"
    };
    println!("  {}: {} ({})", label, st, p.display());
}

/// 同步打印一页诊断（不要求 API_KEY）。
pub fn print_doctor_report(cfg: &AgentConfig, workspace_cli: Option<&str>) {
    println!("CrabMate doctor（人读摘要；密钥与令牌永不打印）");
    println!("版本: {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("【配置摘要】");
    println!("  api_base: {}", cfg.api_base.trim());
    println!("  model: {}", cfg.model.trim());
    println!("  llm_http_auth_mode: {}", cfg.llm_http_auth_mode.as_str());
    println!(
        "  allowed_commands: {} 条（默认见 config/tools.toml；可被覆盖配置或 AGENT_ALLOWED_COMMANDS 替换）",
        cfg.allowed_commands.len()
    );
    println!(
        "  run_command_working_dir: {}",
        cfg.run_command_working_dir.trim()
    );
    println!(
        "  command_timeout_secs / command_max_output_len: {} / {}",
        cfg.command_timeout_secs, cfg.command_max_output_len
    );
    println!(
        "  mcp_enabled: {}  mcp_tool_timeout_secs: {}",
        cfg.mcp_enabled, cfg.mcp_tool_timeout_secs
    );
    println!(
        "  mcp_command: {}",
        if cfg.mcp_command.trim().is_empty() {
            "（未配置）".to_string()
        } else {
            format!("已配置（{} 字符，内容已隐藏）", cfg.mcp_command.len())
        }
    );
    println!("  api_timeout_secs: {}", cfg.api_timeout_secs);
    println!(
        "  web_api_bearer_token: {}",
        if cfg.web_api_bearer_token.expose_secret().trim().is_empty() {
            "未配置"
        } else {
            "已配置（值已隐藏）"
        }
    );
    println!();
    println!("【密钥状态】");
    println!("  {}", api_key_line(cfg));
    println!();

    let ws = resolve_workspace_dir(cfg, workspace_cli);
    println!("【工作区路径】");
    println!("  当前目录: {}", ws.display());
    path_status_line("Cargo.toml", &ws.join("Cargo.toml"));
    path_status_line("frontend/package.json", &ws.join("frontend/package.json"));
    path_status_line("frontend/node_modules", &ws.join("frontend/node_modules"));
    path_status_line("frontend/dist", &ws.join("frontend/dist"));
    path_status_line("target", &ws.join("target"));
    if let Ok(root) = canonical_workspace_root(&ws)
        && root != ws
    {
        println!("  （解析到的仓库根）: {}", root.display());
    }
    println!();

    println!("【Rust 工具链】");
    if let Some(s) = capture_trimmed("rustc", &["-V"]) {
        println!("  rustc -V: {}", s);
    } else {
        println!("  rustc -V: 无法执行或失败");
    }
    if let Some(s) = capture_trimmed("cargo", &["-V"]) {
        println!("  cargo -V: {}", s);
    } else {
        println!("  cargo -V: 无法执行或失败");
    }
    if let Some(s) = capture_trimmed("rustup", &["default"]) {
        let line = s.lines().next().unwrap_or(s.as_str()).trim();
        println!("  rustup default: {}", line);
    } else {
        println!("  rustup default: 不可用或未安装");
    }
    println!();

    let pkg_json = ws.join("frontend/package.json");
    println!("【Node / npm（若存在 frontend/package.json）】");
    if pkg_json.is_file() {
        if let Some(s) = capture_trimmed("npm", &["--version"]) {
            println!("  npm --version: {}", s);
        } else {
            println!("  npm: 未找到或执行失败");
        }
    } else {
        println!("  （跳过：无 frontend/package.json）");
    }
    println!();

    println!(
        "【说明】模型侧自动排障请用工具 **diagnostic_summary**（与本命令互补）。\
         **models** / **probe**：`llm_http_auth_mode=bearer` 时需有效 **API_KEY**；`none` 时可不设。部分网关不提供 OpenAI 兼容 GET /models。"
    );
    println!();
    println!("【终端与工具审批（repl / chat）】");
    let stdin_tty = io::stdin().is_terminal();
    let stderr_tty = io::stderr().is_terminal();
    println!(
        "  stdin 为 TTY: {}  stderr 为 TTY: {}",
        if stdin_tty { "是" } else { "否" },
        if stderr_tty { "是" } else { "否" },
    );
    if stdin_tty && stderr_tty {
        println!(
            "  非白名单 **run_command** 与未匹配前缀的 **http_fetch** / **http_request** 可使用 **dialoguer** 箭头菜单（stderr）。"
        );
    } else {
        println!(
            "  **非交互模式**：上述工具将打印说明到 stderr 并从 **stdin** 读一行（y / a / n）；管道或 CI 中 stdin 非 TTY 时易阻塞或默认拒绝。"
        );
        println!(
            "  建议：脚本使用 **chat --yes**（极危险，仅可信环境）或 **--approve-commands**（仅扩展 run_command 命令名）；HTTP 工具须匹配 **http_fetch_allowed_prefixes** 或改用 Web 审批。"
        );
    }
    let n_prefix = cfg.http_fetch_allowed_prefixes.len();
    println!(
        "  http_fetch_allowed_prefixes: {} 条（未匹配的 HTTP 工具在 CLI 与 TTY 路径下可能需审批）",
        n_prefix
    );
    println!(
        "  退出码与 JSON 行协议摘要：**docs/CLI_CONTRACT.md**；SSE 流错误码：**docs/SSE_PROTOCOL.md**。"
    );
}

/// `crabmate models`：打印模型 id 列表。
pub async fn run_models_cli(
    client: &Client,
    cfg: &AgentConfig,
    api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let r = fetch_models_report(
        client,
        cfg.api_base.trim(),
        api_key.trim(),
        cfg.llm_http_auth_mode,
    )
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    println!("请求: {}", r.url_display);
    println!("HTTP {}  耗时 {} ms", r.http_status, r.elapsed_ms);
    if let Some(ref n) = r.note {
        println!("{}", n);
    }
    if r.model_ids.is_empty() {
        if r.note.is_none() {
            println!("（无模型 id；响应可能非标准）");
        }
    } else {
        for id in &r.model_ids {
            println!("  {}", id);
        }
        println!("共 {} 个模型 id", r.model_ids.len());
    }
    Ok(())
}

/// `crabmate probe`：仅报告连通性与 HTTP 状态。
pub async fn run_probe_cli(
    client: &Client,
    cfg: &AgentConfig,
    api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let r = fetch_models_report(
        client,
        cfg.api_base.trim(),
        api_key.trim(),
        cfg.llm_http_auth_mode,
    )
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    println!("探测 URL: {}", r.url_display);
    println!("HTTP {}  耗时 {} ms", r.http_status, r.elapsed_ms);
    match r.http_status {
        200..=299 => {
            if r.model_ids.is_empty() {
                println!("连通性: 可达（成功响应，但未解析出模型列表）");
            } else {
                println!(
                    "连通性: 可达（成功解析 {} 个模型 id，详表请用 crabmate models）",
                    r.model_ids.len()
                );
            }
        }
        401 | 403 => println!("连通性: 鉴权失败（请检查 API_KEY 是否有效）"),
        404 => {
            println!("连通性: 404 — 部分供应商不提供 OpenAI 兼容 /models，可改用实际 chat 请求验证")
        }
        _ => println!("连通性: 非 2xx，请核对 api_base 与网络"),
    }
    if let Some(n) = r.note {
        println!("{}", n);
    }
    Ok(())
}
