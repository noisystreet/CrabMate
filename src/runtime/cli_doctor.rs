//! `crabmate doctor` / `models` / `probe`：面向终端的一页诊断与网关探测（输出脱敏，不打印密钥）。

use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::AgentConfig;
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

fn api_key_line() -> &'static str {
    match std::env::var("API_KEY") {
        Err(std::env::VarError::NotPresent) => "API_KEY: 未设置（chat / models / probe 不可用）",
        Err(std::env::VarError::NotUnicode(_)) => "API_KEY: 已设置(非 Unicode，不展示)",
        Ok(s) if s.trim().is_empty() => "API_KEY: 已设置但为空",
        Ok(_) => "API_KEY: 已设置(非空，值已隐藏)",
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
    println!(
        "  allowed_commands: {} 条（dev/prod 由配置 [agent] env 在加载时选定）",
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
    println!("  api_timeout_secs: {}", cfg.api_timeout_secs);
    println!(
        "  web_api_bearer_token: {}",
        if cfg.web_api_bearer_token.trim().is_empty() {
            "未配置"
        } else {
            "已配置（值已隐藏）"
        }
    );
    println!();
    println!("【密钥状态】");
    println!("  {}", api_key_line());
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
         **models** / **probe** 依赖 API_KEY，且部分网关不提供 OpenAI 兼容 GET /models。"
    );
}

/// `crabmate models`：打印模型 id 列表。
pub async fn run_models_cli(
    client: &Client,
    cfg: &AgentConfig,
    api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let r = fetch_models_report(client, cfg.api_base.trim(), api_key.trim())
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
    let r = fetch_models_report(client, cfg.api_base.trim(), api_key.trim())
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
