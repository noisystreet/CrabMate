//! `crabmate doctor` / `models` / `probe`：面向终端的一页诊断与网关探测（输出脱敏，不打印密钥）。
//! 子命令 [`print_doctor_report`]、[`run_probe_cli`]、[`run_models_cli`] 供 CLI 入口调用（进程内 REPL slash 已移除）。

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
        .unwrap_or_else(|| PathBuf::from(cfg.command_exec.run_command_working_dir.trim()));
    raw.canonicalize().unwrap_or(raw)
}

fn api_key_line(cfg: &AgentConfig) -> String {
    match std::env::var("API_KEY") {
        Err(std::env::VarError::NotPresent) => {
            if cfg.llm.llm_http_auth_mode == LlmHttpAuthMode::None {
                "API_KEY: 未设置（llm_http_auth_mode=none 时 chat / models / probe 可不依赖密钥）"
                    .to_string()
            } else {
                "API_KEY: 未设置（llm_http_auth_mode=bearer 时 chat / models / probe 不可用）"
                    .to_string()
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => "API_KEY: 已设置(非 Unicode，不展示)".to_string(),
        Ok(s) if s.trim().is_empty() => {
            if cfg.llm.llm_http_auth_mode == LlmHttpAuthMode::None {
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

fn print_doctor_config_block(cfg: &AgentConfig) {
    println!("【配置摘要】");
    println!("  api_base: {}", cfg.llm.api_base.trim());
    println!("  model: {}", cfg.llm.model.trim());
    println!(
        "  llm_http_auth_mode: {}",
        cfg.llm.llm_http_auth_mode.as_str()
    );
    println!(
        "  allowed_commands: {} 条（默认见 config/tools.toml；可被覆盖配置或 CM_ALLOWED_COMMANDS 替换）",
        cfg.command_exec.allowed_commands.len()
    );
    println!(
        "  run_command_working_dir: {}",
        cfg.command_exec.run_command_working_dir.trim()
    );
    println!(
        "  command_timeout_secs / command_max_output_len: {} / {}",
        cfg.command_exec.command_timeout_secs, cfg.command_exec.command_max_output_len
    );
    println!(
        "  mcp_enabled: {}  mcp_tool_timeout_secs: {}",
        cfg.mcp_client.mcp_enabled, cfg.mcp_client.mcp_tool_timeout_secs
    );
    println!(
        "  mcp_command: {}",
        if cfg.mcp_client.mcp_command.trim().is_empty() {
            "（未配置）".to_string()
        } else {
            format!(
                "已配置（{} 字符，内容已隐藏）",
                cfg.mcp_client.mcp_command.len()
            )
        }
    );
    println!(
        "  api_timeout_secs: {}",
        cfg.llm_http_retry.api_timeout_secs
    );
    println!(
        "  web_api_bearer_token: {}",
        if cfg
            .web_api
            .web_api_bearer_token
            .expose_secret()
            .trim()
            .is_empty()
        {
            "未配置"
        } else {
            "已配置（值已隐藏）"
        }
    );
    println!(
        "  web_api_require_bearer: {}",
        if cfg.web_api.web_api_require_bearer {
            "true（serve 须配非空 web_api_bearer_token）"
        } else {
            "false"
        }
    );
    println!(
        "  orchestration_profile: {}（运行时固定；旧 TOML 别名忽略）",
        cfg.per_plan_policy.orchestration_profile.as_str()
    );
    println!(
        "  planner_executor_mode: {}（仅 single_agent）",
        cfg.per_plan_policy.planner_executor_mode.as_str()
    );
    println!(
        "  有效编排路径（静态）: {}（session_mode / Act 句启发式 → ReAct 外循环）",
        crate::cm_config::effective_orchestration_path_summary(
            cfg.per_plan_policy.planner_executor_mode.as_str(),
            cfg.per_plan_policy.orchestration_profile,
        )
    );
}

fn print_doctor_serve_deployment_block(cfg: &AgentConfig) {
    println!("【serve 单独部署（安全）】");
    let bearer_set = !cfg
        .web_api
        .web_api_bearer_token
        .expose_secret()
        .trim()
        .is_empty();
    let require = cfg.web_api.web_api_require_bearer;
    let audit = cfg.web_api.web_audit_log_write_tools;
    println!(
        "  web_api_bearer_token: {}",
        if bearer_set {
            "已配置（值已隐藏）"
        } else {
            "未配置"
        }
    );
    println!("  web_api_require_bearer: {}", require);
    println!("  web_audit_log_write_tools: {}", audit);
    if !bearer_set {
        println!(
            "  [!] 非本机部署：须配置 CM_WEB_API_BEARER_TOKEN，并建议 web_api_require_bearer=true"
        );
        println!(
            "  [!] TLS 反代 + systemd 示例：docs/个人VPS部署指南.md；按用户账号/配额在网关/BFF（docs/未来规划功能.md）"
        );
    } else if !require {
        println!("  [i] 已配共享密钥但未强制 require_bearer；生产建议 CM_WEB_API_REQUIRE_BEARER=1");
    } else {
        println!("  OK  Bearer 启动校验已启用（进程内为共享密钥，非按用户账号）");
    }
    if !audit {
        println!("  [!] 建议开启 web_audit_log_write_tools 记录写副作用工具");
    }
}

fn print_doctor_workspace_block(ws: &Path) {
    println!("【工作区路径】");
    println!("  当前目录: {}", ws.display());
    path_status_line("Cargo.toml", &ws.join("Cargo.toml"));
    path_status_line(
        "UI dist (optional)",
        &std::env::var("CM_WEB_STATIC_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ws.join("frontend/dist")),
    );
    path_status_line("target", &ws.join("target"));
    path_status_line(".crabmate/workflows", &ws.join(".crabmate/workflows"));
    if let Ok(root) = canonical_workspace_root(ws)
        && root != *ws
    {
        println!("  （解析到的仓库根）: {}", root.display());
    }
}

fn doctor_validate_workflow_file(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {e}"))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let yaml = if ext == "md" || ext == "markdown" {
        crate::agent::workflow::extract_first_crabmate_workflow_block(&text)?
    } else {
        text
    };
    let compiled = crate::agent::workflow::compile_workflow_author_yaml(&yaml)?;
    let args_json =
        serde_json::to_string(&compiled).map_err(|e| format!("JSON 序列化失败: {e}"))?;
    let spec = crate::agent::workflow::parse_workflow_spec_from_json(&args_json)?;
    crate::agent::workflow::workflow_topo_layers(&spec.nodes)?;
    Ok(())
}

fn print_doctor_workflows_block(ws: &Path) {
    println!("【工作流作者层（.crabmate/workflows）】");
    let wf_dir = ws.join(".crabmate/workflows");
    if !wf_dir.is_dir() {
        println!("  目录不存在（可选；可添加 .crabmate/workflows/*.yaml，见 docs/工具说明.md）");
        return;
    }

    let mut files: Vec<PathBuf> = match std::fs::read_dir(&wf_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "md"))
            })
            .collect(),
        Err(e) => {
            println!("  无法读取目录: {e}");
            return;
        }
    };
    files.sort();

    if files.is_empty() {
        println!("  （目录为空）");
        return;
    }

    let mut ok = 0usize;
    let mut fail = 0usize;
    for path in &files {
        let rel = path
            .strip_prefix(ws)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        match doctor_validate_workflow_file(path) {
            Ok(()) => {
                ok += 1;
                println!("  OK  {rel}");
            }
            Err(e) => {
                fail += 1;
                println!("  FAIL {rel}: {e}");
            }
        }
    }
    println!("  合计: {ok} 通过, {fail} 失败（compile + DAG 校验）");
}

fn print_doctor_rust_toolchain_block() {
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
}

fn print_doctor_frontend_block(_ws: &Path) {
    println!("【Web UI（可选静态资源）】");
    println!("  官方 UI：同级 crabmate-client（cd ../crabmate-client && make frontend）");
    println!("  serve：默认纯 API；托管 SPA 用 --with-web 与 CM_WEB_STATIC_DIR=…/frontend/dist");
    if let Ok(dir) = std::env::var("CM_WEB_STATIC_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            path_status_line("CM_WEB_STATIC_DIR", Path::new(trimmed));
        }
    }
}

fn print_doctor_tty_approval_block(cfg: &AgentConfig) {
    println!(
        "【说明】模型侧自动排障请用工具 **diagnostic_summary**（与本命令互补）。\
         **models** / **probe**：`llm_http_auth_mode=bearer` 时需有效 **API_KEY**；`none` 时可不设。部分网关不提供 OpenAI 兼容 GET /models。"
    );
    println!();
    println!("【工具审批（Web SSE）】");
    println!(
        "  非白名单 **run_command** 与未匹配前缀的 **http_fetch** / **http_request**：仅 **Web** `/chat/stream`（`approval_session_id`）经 SSE 人工审批；运维 CLI 无同进程对话审批。"
    );
    println!(
        "  官方对话请用 Client **crabmate-tui** / Web；或扩大 **allowed_commands** / 匹配 **http_fetch_allowed_prefixes**（仅可信环境）。同进程 **chat** / 终端审批已移除（D2.2）。"
    );
    let n_prefix = cfg.http_fetch.http_fetch_allowed_prefixes.len();
    println!(
        "  http_fetch_allowed_prefixes: {} 条（未匹配时需 Web SSE 审批通道）",
        n_prefix
    );
    println!(
        "  退出码与 JSON 行协议摘要：**docs/命令行契约.md**；SSE 流错误码：**docs/SSE协议.md**。"
    );
}

/// 同步打印一页诊断（不要求 API_KEY）。
pub fn print_doctor_report(cfg: &AgentConfig, workspace_cli: Option<&str>) {
    println!("CrabMate doctor（人读摘要；密钥与令牌永不打印）");
    println!("版本: {}", env!("CARGO_PKG_VERSION"));
    println!();
    print_doctor_config_block(cfg);
    println!();
    print_doctor_serve_deployment_block(cfg);
    println!();
    println!("【密钥状态】");
    println!("  {}", api_key_line(cfg));
    println!();

    let ws = resolve_workspace_dir(cfg, workspace_cli);
    print_doctor_workspace_block(&ws);
    println!();

    print_doctor_workflows_block(&ws);
    println!();

    print_doctor_rust_toolchain_block();
    println!();

    print_doctor_frontend_block(&ws);
    println!();

    print_doctor_user_data_block();
    println!();

    print_doctor_tty_approval_block(cfg);
}

fn print_doctor_user_data_block() {
    use crate::user_data::{load_meta, secrets_status, user_data_root};
    println!("【本机用户数据】");
    let root = user_data_root();
    println!("  根目录: {}", root.display());
    let meta = load_meta();
    if meta.migrated_from.is_empty() {
        println!(
            "  meta: schema_version={}（尚未记录迁移来源）",
            meta.schema_version
        );
    } else {
        println!(
            "  meta: schema_version={} migrated_from={:?}",
            meta.schema_version, meta.migrated_from
        );
    }
    let st = secrets_status();
    println!(
        "  system-keyring/web_api_bearer: {}",
        if st.web_api_bearer.set {
            "已设置（值已隐藏）"
        } else {
            "未设置"
        }
    );
    println!(
        "  模型密钥：权威在 Client（请求体 client_llm.api_key）；服务端不再持有 client_llm/executor_llm/saved_model 钥匙串槽"
    );
    println!("  详见 docs/design/user_data_dir.md");
}

/// `crabmate models`：打印模型 id 列表。
pub async fn run_models_cli(
    client: &Client,
    cfg: &AgentConfig,
    api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let r = fetch_models_report(
        client,
        cfg.llm.api_base.trim(),
        api_key.trim(),
        cfg.llm.llm_http_auth_mode,
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
        cfg.llm.api_base.trim(),
        api_key.trim(),
        cfg.llm.llm_http_auth_mode,
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
