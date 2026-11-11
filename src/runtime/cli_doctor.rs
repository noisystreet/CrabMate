//! `crabmate doctor` / `models` / `probe`：面向终端的一页诊断与网关探测（输出脱敏，不打印密钥）。
//! 子命令 [`print_doctor_report`]、[`run_probe_cli`]、[`run_models_cli`] 供 CLI 入口调用（进程内 REPL slash 已移除）。

use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::AgentConfig;
use crate::config::{ExposeSecret, LlmHttpAuthMode};
use crate::config::layer_precedence::LayerTrace;
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
    if crate::cm_tools::tools::allowlist_allows_all(&cfg.command_exec.allowed_commands) {
        println!(
            "  allowed_commands: 全部放行（`*`；命令名不做校验，通常来自 `serve --yes` 或 CM_ALLOWED_COMMANDS=*）"
        );
    } else {
        println!(
            "  allowed_commands: {} 条（默认见 config/tools.toml；可被覆盖配置或 CM_ALLOWED_COMMANDS 替换）",
            cfg.command_exec.allowed_commands.len()
        );
    }
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
    println!("  orchestration_profile: react（运行时固定；对应配置键已移除）");
    println!(
        "  有效编排路径（静态）: react outer loop（session_mode / Act 句启发式 → ReAct 外循环）"
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
    // 与 `crabmate workflow validate` / `workflow_execute` 加载期同源：本函数问的就是
    // 「这个工作流文件能加载吗」，拼错的 `tool_name` 与未知依赖都须在这里暴露。
    // `workflow_validate_dag` 已含未知依赖与环检测，故不再单独跑 `topo_layers`。
    crate::agent::workflow::workflow_validate_tool_names(&spec)?;
    crate::agent::workflow::workflow_validate_dag(&spec.nodes)?;
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

/// 提示 UI 托管边界：`serve` 永远纯 API，UI 由 Client 仓自行托管。
fn print_doctor_frontend_block() {
    println!("【Web UI（由 Client 仓托管）】");
    println!("  官方 UI：同级 crabmate-client（cd ../crabmate-client && make frontend）");
    println!("  serve：永远纯 API，不托管 SPA；Client 经 CORS（web_cors_allowed_origins）接入");
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
    let prefixes = &cfg.http_fetch.http_fetch_allowed_prefixes;
    let allow_any = crate::tools::http_fetch::prefixes_include_allow_any(prefixes);
    println!(
        "  http_fetch_allowed_prefixes: {} 条{}（未匹配时需 Web SSE 审批通道）",
        prefixes.len(),
        if allow_any {
            "（含 `*`：任意 http/https 直接执行）"
        } else {
            ""
        }
    );
    println!(
        "  退出码与 JSON 行协议摘要：**docs/命令行契约.md**；SSE 流错误码：**docs/SSE协议.md**。"
    );
}

/// 同步打印一页诊断（不要求 API_KEY）。
///
/// `trace` 为配置层溯源（#1342 P2），由调用方用
/// `cm_cli::config_load::load_cli_agent_config_traced` 一并取回。
pub fn print_doctor_report(cfg: &AgentConfig, workspace_cli: Option<&str>, trace: &LayerTrace) {
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

    print_doctor_frontend_block();
    println!();

    print_doctor_user_data_block();
    println!();

    print_doctor_context_gates_block(cfg);
    println!();
    print_doctor_conversation_layout_block(cfg);
    println!();

    print_doctor_config_layer_block(cfg, trace);
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

/// 上下文「双闸」对照（#1342）：只读打印 token 预算与条数闸的等效水位，并指出先到者。
///
/// 目的：让「为什么上下文才两三成就丢历史」不必读源码就能回答（#1339）。
/// 假定 token/条 的取值与配置自检同源（`cm_config::context_gates`），不参与任何裁剪判定。
fn print_doctor_context_gates_block(cfg: &AgentConfig) {
    println!("【上下文双闸对照（#1342）】");
    for line in doctor_context_gates_lines(cfg) {
        println!("{line}");
    }
}

/// 双闸对照块正文（**不含**标题）：拆出来是为了让测试直接断言两条线的数值与「先到者」，
/// 不必抓 stdout（`cargo test` 默认吞掉 `println!`）。
fn doctor_context_gates_lines(cfg: &AgentConfig) -> Vec<String> {
    let Some(alignment) = crate::cm_config::context_gates::context_gate_alignment(cfg) else {
        return vec![
            "  llm_context_tokens = 0（token 预算未启用，走 context_char_budget 降级路径）".to_string(),
            "  口径：预算推导，非回合实测（回合用量看 `context_trim` 与 SSE `tiktokenPromptTokens`）"
                .to_string(),
        ];
    };
    let b = alignment.budget;
    let mut lines = vec![
        "  口径：预算推导，非回合实测（回合用量看 `context_trim` 与 SSE `tiktokenPromptTokens`）"
            .to_string(),
        format!(
            "  窗口 {} → 输出预留 {} − 安全余量 {} = 输入预算 {}",
            b.context_window_tokens,
            b.reserved_output_tokens,
            b.safety_margin_tokens,
            b.max_input_tokens
        ),
        format!(
            "  token 闸: trigger {} / target {}",
            b.trigger_tokens, b.target_tokens
        ),
        format!(
            "  条数闸: max_message_history={} 条 × 假定 {} token/条 ≈ {} token",
            alignment.max_message_history,
            alignment.assumed_tokens_per_message,
            alignment.count_gate_tokens
        ),
    ];
    if alignment.count_gate_wins() {
        let suggested = alignment.suggested_max_message_history();
        lines.push(format!(
            "  先到: 条数闸 [!] 上下文水位还很低就会按条数裁剪；建议 max_message_history ≥ {suggested}"
        ));
        if let Some(hint) = crate::cm_config::context_gates::unreachable_suggestion_hint(suggested) {
            lines.push(format!("  {hint}"));
        }
    } else {
        lines.push("  先到: token 闸（条数闸为高位兜底）".to_string());
    }
    lines
}

/// 配置层溯源（#1342 P2）：关键键最终被哪一层改动，以及层序表本身。
///
/// 目的：回答「这个值到底谁定的」不必再读调用序列（#1340）。密钥类键只显示
/// 「已设置/未设置」，永不打印明文。
fn print_doctor_config_layer_block(cfg: &AgentConfig, trace: &LayerTrace) {
    println!("【配置层溯源（#1342）】");
    for line in doctor_config_layer_lines(cfg, trace) {
        println!("{line}");
    }
}

/// 溯源块正文（不含标题），测试直接断言内容而不抓 stdout。
fn doctor_config_layer_lines(cfg: &AgentConfig, trace: &LayerTrace) -> Vec<String> {
    let layers: Vec<&str> = crate::config::layer_precedence::LAYERS
        .iter()
        .map(|layer| layer.as_str())
        .collect();
    let mut lines = vec![format!("  层序（低 → 高）: {}", layers.join(" → "))];
    lines.extend(trace.doctor_lines(cfg));
    lines
}

/// 会话库 `layout` 覆盖率（B4/P4 门禁）：**只读**统计 legacy 行占比，不建表、不写盘。
///
/// 用途是给「删 legacy 读路径」提供数据（Issue #1333 关闭条件 4）。阈值**不**在代码里硬编码：
/// 数据出来后在 Issue 与 `docs/Turn布局设计.md` §16.5 回填，避免把一次拍板写死进程序。
fn print_doctor_conversation_layout_block(cfg: &AgentConfig) {
    println!("【会话布局覆盖率（B4/P4 门禁）】");
    let raw = cfg
        .conversation_persistence
        .conversation_store_sqlite_path
        .trim();
    if raw.is_empty() {
        println!("  conversation_store_sqlite_path 未配置（纯内存后端，无持久化行可统计）");
        return;
    }
    let path = std::path::Path::new(raw);
    if !path.exists() {
        println!("  会话库尚不存在: {}（首次落盘后才有数据）", path.display());
        return;
    }
    match crate::conversation_store::layout_coverage(path) {
        Ok(cov) => match cov.legacy_ratio() {
            Some(ratio) => println!(
                "  会话行 {} / 无 layout 行 {}（{:.1}% 只能走 legacy 读路径）",
                cov.total,
                cov.without_layout,
                ratio * 100.0
            ),
            None => println!("  会话行 0（空库，占比无意义）"),
        },
        Err(e) => println!("  [!] 无法读取会话库 {}: {e}", path.display()),
    }
    println!(
        "  口径: layout_meta_json 为 NULL 或空白；删 legacy 读路径前须低于既定阈值（见 Issue #1333）"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(window: u32, history: usize) -> AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = window;
        cfg.llm_sampling.max_tokens = 16_384;
        cfg.session_ui.max_message_history = history;
        cfg
    }

    /// 1M 窗口 + 256 条：块里必须同时出现两条线的数值，并指出条数闸先到。
    #[test]
    fn gates_block_reports_both_values_and_winner() {
        let lines = doctor_context_gates_lines(&cfg_with(1_000_000, 256)).join("\n");
        assert!(lines.contains("trigger 834332"), "{lines}");
        assert!(lines.contains("≈ 307200 token"), "{lines}");
        assert!(lines.contains("先到: 条数闸 [!]"), "{lines}");
        assert!(lines.contains("max_message_history ≥ 696"), "{lines}");
        assert!(!lines.contains("上界"), "696 可达成，不该提上界: {lines}");
    }

    /// 默认（1024 条 × 262,144 窗口）：token 闸先到，条数闸只是高位兜底。
    #[test]
    fn gates_block_reports_token_gate_winner_at_default() {
        let lines = doctor_context_gates_lines(&cfg_with(262_144, 1_024)).join("\n");
        assert!(lines.contains("先到: token 闸"), "{lines}");
        assert!(!lines.contains("[!]"), "{lines}");
    }

    /// 超大窗口 + 上界条数：建议值不可达，块里要给出替代路径。
    #[test]
    fn gates_block_flags_unreachable_suggestion() {
        let lines = doctor_context_gates_lines(&cfg_with(2_000_000, 1_024)).join("\n");
        assert!(lines.contains("先到: 条数闸 [!]"), "{lines}");
        assert!(lines.contains("上界"), "{lines}");
        assert!(lines.contains("context_token_trigger_percent"), "{lines}");
    }

    /// `llm_context_tokens = 0`：明确说明走字符预算降级，而不是静默省略整块。
    #[test]
    fn gates_block_states_disabled_budget() {
        let lines = doctor_context_gates_lines(&cfg_with(0, 1_024)).join("\n");
        assert!(lines.contains("token 预算未启用"), "{lines}");
        assert!(lines.contains("context_char_budget"), "{lines}");
    }

    /// #1347 S3b：doctor 是离线命令、没有回合，块里必须自报「预算推导，非回合实测」，
    /// 否则很容易被当成「本会话用了多少」的权威数字（issue 原验收曾这样要求过）。
    #[test]
    fn gates_block_declares_estimate_not_turn_measurement() {
        for lines in [
            doctor_context_gates_lines(&cfg_with(262_144, 1_024)).join("\n"),
            doctor_context_gates_lines(&cfg_with(0, 1_024)).join("\n"),
        ] {
            assert!(lines.contains("非回合实测"), "{lines}");
            assert!(lines.contains("context_trim"), "应指向真正的用量出口: {lines}");
        }
    }

    /// 溯源块必须同时给出层序表与「谁定的」，且 bearer 只显示状态（#1342 P2）。
    #[test]
    fn config_layer_block_reports_order_and_sources() {
        use crate::config::layer_precedence::LayerTrace;

        let cfg = cfg_with(262_144, 1_024);
        let trace = LayerTrace::default();
        let lines = doctor_config_layer_lines(&cfg, &trace).join("\n");
        assert!(
            lines.contains("embedded → user_toml → env → user_data → cli"),
            "{lines}"
        );
        assert!(lines.contains("llm_context_tokens = 262144"), "{lines}");
        assert!(lines.contains("沿用内嵌默认"), "{lines}");
        let bearer_line = lines
            .lines()
            .find(|line| line.contains("web_api_bearer_token"))
            .expect("溯源块必须列出 bearer 一行");
        assert!(
            bearer_line.contains("未设置") || bearer_line.contains("已设置（值已隐藏）"),
            "bearer 只允许这两种展示: {bearer_line}"
        );
        // 值描述与外层括号曾叠成 `（未设置）（未覆盖（沿用内嵌默认））`——这里正面守住。
        assert!(!lines.contains("（（"), "括号不得叠: {lines}");
        assert!(!lines.contains("））"), "括号不得叠: {lines}");
    }
}
