use crate::config::cli::{ChatCliArgs, SaveSessionCli, SaveSessionFormat, ToolReplayCli};
use crate::config::{AgentConfig, SharedAgentConfig};
use crate::project_profile::build_first_turn_user_context_markdown;
use crate::redact;
use crate::runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_TOOL_REPLAY_MISMATCH, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED,
    EXIT_USAGE, classify_model_error_message,
};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::repl_reedline::{ReplLineEditor, ReplReadLine, read_repl_line_with_editor};
use crate::tool_registry::{CliCommandTurnStats, CliToolRuntime};
use crate::types::{Message, messages_chat_seed, normalize_messages_for_openai_compatible_request};
use crate::{LlmSeedOverride, RunAgentTurnParams, run_agent_turn};
use log::debug;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

/// 长期记忆库打开失败时，仅向 stderr 打印**一次**用户可见说明（避免每轮 REPL/chat 重复刷屏）。
static CLI_LTM_OPEN_FAILURE_NOTIFIED: AtomicBool = AtomicBool::new(false);

/// `chat` / REPL 首轮在 `[system, user]` 之间插入项目画像 + 依赖摘要（与 Web 同源）；`--messages-json-file` 等已带完整 transcript 时不调用。
async fn prepend_cli_first_turn_injection(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    messages: &mut Vec<Message>,
) {
    if messages.len() < 2 {
        return;
    }
    if !messages[0].role.trim().eq_ignore_ascii_case("system")
        || !messages[1].role.trim().eq_ignore_ascii_case("user")
    {
        return;
    }
    let cfg = cfg_holder.read().await.clone();
    let want_heavy = (cfg.project_profile_inject_enabled
        && cfg.project_profile_inject_max_chars > 0)
        || (cfg.project_dependency_brief_inject_enabled
            && cfg.project_dependency_brief_inject_max_chars > 0);
    let ctx: Option<String> = if want_heavy {
        let wd = work_dir.to_path_buf();
        let cfg_c = cfg.clone();
        tokio::task::spawn_blocking(move || {
            build_first_turn_user_context_markdown(&wd, &cfg_c, None)
        })
        .await
        .unwrap_or_default()
    } else {
        build_first_turn_user_context_markdown(work_dir, &cfg, None)
    };
    if let Some(body) = ctx {
        messages.insert(1, Message::user_only(body));
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReplExportKind {
    Json,
    Markdown,
    Both,
}

#[derive(Debug, PartialEq, Eq)]
enum ReplBuiltIn<'a> {
    Clear,
    Model,
    /// `arg` 为命令名后的剩余文本；非空表示用户传了多余参数，应提示用法。
    Config(&'a str),
    /// 与 `crabmate doctor` 一致；`arg` 非空则报错。
    Doctor(&'a str),
    /// 与 `crabmate probe` 一致；`arg` 非空则报错；由 REPL 循环异步执行探测。
    Probe(&'a str),
    /// 与 `crabmate models` 一致；`arg` 非空则报错；由 REPL 循环异步拉取模型列表。
    Models(&'a str),
    WorkspaceShow,
    WorkspaceSet(&'a str),
    Tools,
    Help,
    Export(&'a str),
    /// 与 `crabmate save-session` 一致：从磁盘会话文件导出（非当前内存）。
    SaveSession(&'a str),
    /// `/mcp` · `/mcp list` · `/mcp list probe` · `/mcp probe`（同 `crabmate mcp list`）
    McpList {
        probe: bool,
    },
    /// `/mcp …` 无法解析的子命令
    McpUnknown(String),
    /// `/version`：二进制与平台信息（不含密钥）
    Version,
    Unknown(&'a str),
    BareSlash,
}

/// [`try_handle_repl_slash_command`] 的返回值：`RunProbe` / `RunModels` 需在异步上下文中分别调用
/// [`crate::runtime::cli_doctor::run_probe_cli`]、[`crate::runtime::cli_doctor::run_models_cli`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplSlashHandled {
    NotSlash,
    Handled,
    RunProbe,
    RunModels,
    /// 同 `crabmate mcp list`（`probe` 会启动 MCP 子进程）
    RunMcpList {
        probe: bool,
    },
    /// `/config reload`：磁盘+环境变量热更（见 `apply_hot_reload_config_subset`）
    RunConfigReload,
}

const REPL_SHELL_USAGE: &str = "bash#: <命令>  在当前工作区执行一行 shell（不发给模型；无交互 stdin）。等同本机 `sh -c` / `cmd /C`，不受模型 `run_command` 白名单约束，仅应在可信环境使用。交互 TTY：空行按 `$` 即切换「我:」/ bash#:（也可单独一行 `$` 后 Enter）；管道/非 TTY 仍可用行内 `$ <命令>`。历史保存在工作区 `.crabmate/repl_history.txt`。示例: ls  pwd  git status";

/// 执行 REPL 本地 shell 一行：`parsed` 为 `repl_reedline::parse_repl_dollar_shell_line` 的 `Some(...)` 内层；`None` 表示仅 `$` 或空命令，打印用法。
fn repl_execute_shell(
    parsed: Option<&str>,
    work_dir: &Path,
    style: &CliReplStyle,
) -> io::Result<()> {
    let cmd = match parsed {
        None => None,
        Some(c) => {
            let t = c.trim();
            if t.is_empty() { None } else { Some(t) }
        }
    };
    let Some(cmd) = cmd else {
        let _ = style.print_line(REPL_SHELL_USAGE);
        return Ok(());
    };
    if cmd.contains('\0') {
        let _ = style.eprint_error("命令含空字节，已拒绝执行。");
        return Ok(());
    }
    let code = run_repl_shell_line_sync(cmd, work_dir)?;
    if code != 0 {
        let _ = style.print_line(&format!("退出码: {code}"));
    }
    Ok(())
}

fn run_repl_shell_line_sync(cmd: &str, work_dir: &Path) -> io::Result<i32> {
    let status = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    } else {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    };
    Ok(status
        .code()
        .unwrap_or(if status.success() { 0 } else { -1 }))
}

/// 解析 REPL 行首 `/` 内建命令；非内建前缀返回 `None`。
fn classify_repl_slash_command(input: &str) -> Option<ReplBuiltIn<'_>> {
    let s = input.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = s[1..].trim();
    if rest.is_empty() {
        return Some(ReplBuiltIn::BareSlash);
    }
    let head = rest.split_whitespace().next().unwrap_or("");
    let cmd = head.to_ascii_lowercase();
    let arg = rest[head.len()..].trim();
    Some(match cmd.as_str() {
        "clear" => ReplBuiltIn::Clear,
        "model" => ReplBuiltIn::Model,
        "config" => ReplBuiltIn::Config(arg),
        "doctor" => ReplBuiltIn::Doctor(arg),
        "probe" => ReplBuiltIn::Probe(arg),
        "models" => ReplBuiltIn::Models(arg),
        "workspace" | "cd" => {
            if arg.is_empty() {
                ReplBuiltIn::WorkspaceShow
            } else {
                ReplBuiltIn::WorkspaceSet(arg)
            }
        }
        "tools" => ReplBuiltIn::Tools,
        "help" | "?" => ReplBuiltIn::Help,
        "export" => ReplBuiltIn::Export(arg),
        "save-session" => ReplBuiltIn::SaveSession(arg),
        "mcp" => {
            let tail = arg.trim();
            if tail.is_empty() {
                ReplBuiltIn::McpList { probe: false }
            } else {
                let mut parts = tail.split_whitespace();
                let a = parts.next().unwrap_or("").to_ascii_lowercase();
                let b = parts.next();
                if parts.next().is_some() {
                    ReplBuiltIn::McpUnknown(tail.to_string())
                } else if a == "list" {
                    match b {
                        None => ReplBuiltIn::McpList { probe: false },
                        Some(x) if x.eq_ignore_ascii_case("probe") => {
                            ReplBuiltIn::McpList { probe: true }
                        }
                        Some(_) => ReplBuiltIn::McpUnknown(tail.to_string()),
                    }
                } else if a == "probe" && b.is_none() {
                    ReplBuiltIn::McpList { probe: true }
                } else {
                    ReplBuiltIn::McpUnknown(tail.to_string())
                }
            }
        }
        "version" => ReplBuiltIn::Version,
        _ => ReplBuiltIn::Unknown(head),
    })
}

fn print_repl_version_line() {
    println!(
        "crabmate {} ({}/{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

fn repl_export_kind_from_arg(arg: &str) -> Result<ReplExportKind, ()> {
    let a = arg.trim().to_ascii_lowercase();
    match a.as_str() {
        "" | "both" => Ok(ReplExportKind::Both),
        "json" => Ok(ReplExportKind::Json),
        "markdown" | "md" => Ok(ReplExportKind::Markdown),
        _ => Err(()),
    }
}

/// 将内存中的消息导出到工作区 `.crabmate/exports/`（与 Web 及 `save-session` 落盘形状同形）。
fn repl_export_current_messages(
    work_dir: &Path,
    messages: &[Message],
    kind: ReplExportKind,
    style: &CliReplStyle,
) -> io::Result<()> {
    match kind {
        ReplExportKind::Json => {
            let p = crate::runtime::workspace_session::export_json(work_dir, messages)?;
            style.print_success(&format!("已导出 JSON: {}", p.display()))?;
        }
        ReplExportKind::Markdown => {
            let p = crate::runtime::workspace_session::export_markdown(work_dir, messages)?;
            style.print_success(&format!("已导出 Markdown: {}", p.display()))?;
        }
        ReplExportKind::Both => {
            let pj = crate::runtime::workspace_session::export_json(work_dir, messages)?;
            let pm = crate::runtime::workspace_session::export_markdown(work_dir, messages)?;
            style.print_success(&format!("已导出 JSON: {}", pj.display()))?;
            style.print_success(&format!("已导出 Markdown: {}", pm.display()))?;
        }
    }
    Ok(())
}

/// `crabmate save-session`：从磁盘会话文件读取并写入导出目录（兼容别名 `export-session`）。
/// `crabmate tool-replay export|run`（不要求 API_KEY；重放路径与对话相同执行真实工具，须在可信工作区）。
pub fn run_tool_replay_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cmd: ToolReplayCli,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::ErrorKind;

    let workspace = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    match cmd {
        ToolReplayCli::Export {
            session_file,
            output,
            note,
        } => {
            let session_path = match session_file
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                Some(p) => PathBuf::from(p),
                None => crate::runtime::workspace_session::session_file_path(&workspace),
            };
            if !session_path.is_file() {
                eprintln!("会话文件不存在: {}", session_path.display());
                return Err(std::io::Error::new(ErrorKind::NotFound, "会话文件不存在").into());
            }
            let out_path = output
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let note_ref = note.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let written = crate::runtime::tool_replay::export_tool_replay_fixture(
                &session_path,
                &workspace,
                out_path.as_deref(),
                note_ref,
            )?;
            println!("{}", written.display());
        }
        ToolReplayCli::Run {
            fixture,
            compare_recorded,
        } => {
            let f = fixture.trim();
            if f.is_empty() {
                return Err(
                    CliExitError::new(EXIT_USAGE, "tool-replay run：--fixture 不能为空").into(),
                );
            }
            let fixture_path = PathBuf::from(f);
            if !fixture_path.is_file() {
                eprintln!("fixture 不存在: {}", fixture_path.display());
                return Err(std::io::Error::new(ErrorKind::NotFound, "fixture 不存在").into());
            }
            let mut buf = Vec::new();
            let (n_steps, mismatches) = crate::runtime::tool_replay::run_tool_replay_fixture(
                &fixture_path,
                cfg,
                &workspace,
                compare_recorded,
                &mut buf,
            )?;
            let text = String::from_utf8_lossy(&buf);
            print!("{text}");
            if compare_recorded && mismatches > 0 {
                return Err(
                    CliExitError::new(
                        EXIT_TOOL_REPLAY_MISMATCH,
                        format!(
                            "tool-replay：{mismatches} 条步骤与 recorded_output 不一致（共 {n_steps} 步）"
                        ),
                    )
                    .into(),
                );
            }
        }
    }
    Ok(())
}

pub fn run_save_session_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    args: SaveSessionCli,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::ErrorKind;

    let workspace = cli_effective_work_dir(workspace_cli, &cfg.run_command_working_dir);
    let session_path = match args
        .session_file
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(p) => PathBuf::from(p),
        None => crate::runtime::workspace_session::session_file_path(&workspace),
    };
    if !session_path.is_file() {
        eprintln!("会话文件不存在: {}", session_path.display());
        return Err(std::io::Error::new(ErrorKind::NotFound, "会话文件不存在").into());
    }
    let data = std::fs::read_to_string(&session_path)?;
    let parsed: crate::runtime::chat_export::ChatSessionFile = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, format!("会话 JSON 无效: {e}")))?;
    let fmt = match args.format {
        SaveSessionFormat::Json => ReplExportKind::Json,
        SaveSessionFormat::Markdown => ReplExportKind::Markdown,
        SaveSessionFormat::Both => ReplExportKind::Both,
    };
    match fmt {
        ReplExportKind::Json => {
            let p = crate::runtime::workspace_session::export_json(&workspace, &parsed.messages)?;
            println!("{}", p.display());
        }
        ReplExportKind::Markdown => {
            let p =
                crate::runtime::workspace_session::export_markdown(&workspace, &parsed.messages)?;
            println!("{}", p.display());
        }
        ReplExportKind::Both => {
            let pj = crate::runtime::workspace_session::export_json(&workspace, &parsed.messages)?;
            let pm =
                crate::runtime::workspace_session::export_markdown(&workspace, &parsed.messages)?;
            println!("{}", pj.display());
            println!("{}", pm.display());
        }
    }
    Ok(())
}

/// REPL 中以 `/` 开头的内建命令；[`ReplSlashHandled::NotSlash`] 时应将输入交给模型。
async fn try_handle_repl_slash_command(
    input: &str,
    cfg_holder: &SharedAgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &mut PathBuf,
    style: &CliReplStyle,
    no_stream: bool,
) -> ReplSlashHandled {
    let Some(builtin) = classify_repl_slash_command(input) else {
        return ReplSlashHandled::NotSlash;
    };
    match builtin {
        ReplBuiltIn::BareSlash => {
            let _ = style.print_line(
                "输入 /help 查看内建命令；若以 / 开头的文字要发给模型，请避免仅输入一个 /。",
            );
        }
        ReplBuiltIn::Unknown(head) => {
            let _ = style.eprint_error(&format!("未知命令 /{head}。输入 /help 查看列表。"));
        }
        ReplBuiltIn::Clear => {
            let cfg = cfg_holder.read().await.clone();
            let system_prompt = cfg.system_prompt.clone();
            let system_prompt_fb = system_prompt.clone();
            let wd = work_dir.clone();
            let want_heavy = (cfg.project_profile_inject_enabled
                && cfg.project_profile_inject_max_chars > 0)
                || (cfg.project_dependency_brief_inject_enabled
                    && cfg.project_dependency_brief_inject_max_chars > 0);
            *messages = if want_heavy {
                match tokio::task::spawn_blocking(move || {
                    if let Some(ctx) = build_first_turn_user_context_markdown(&wd, &cfg, None) {
                        vec![
                            Message::system_only(system_prompt.clone()),
                            Message::user_only(ctx),
                        ]
                    } else {
                        vec![Message::system_only(system_prompt)]
                    }
                })
                .await
                {
                    Ok(v) => v,
                    Err(_) => vec![Message::system_only(system_prompt_fb)],
                }
            } else if let Some(ctx) = build_first_turn_user_context_markdown(work_dir, &cfg, None) {
                vec![
                    Message::system_only(cfg.system_prompt.clone()),
                    Message::user_only(ctx),
                ]
            } else {
                vec![Message::system_only(cfg.system_prompt.clone())]
            };
            let _ = style.print_success(&format!(
                "已清空对话（保留当前 system 提示词），共 {} 条消息。",
                messages.len()
            ));
        }
        ReplBuiltIn::Model => {
            let cfg = cfg_holder.read().await;
            let _ = style.print_line(&format!("model: {}", cfg.model));
            let _ = style.print_line(&format!("api_base: {}", cfg.api_base));
            let _ = style.print_line(&format!(
                "temperature: {}（配置文件；Web chat 可单条覆盖）",
                cfg.temperature
            ));
            if let Some(seed) = cfg.llm_seed {
                let _ = style.print_line(&format!("llm_seed: {seed}"));
            } else {
                let _ = style.print_line("llm_seed: （未设置，请求不带 seed）");
            }
        }
        ReplBuiltIn::Config(extra) => {
            let e = extra.trim();
            if e.eq_ignore_ascii_case("reload") {
                return ReplSlashHandled::RunConfigReload;
            }
            if !e.is_empty() {
                let _ = style.eprint_error("用法: /config · /config reload（热重载，见文档）");
            } else {
                let cfg = cfg_holder.read().await;
                if let Err(err) = style.print_repl_config_summary(
                    &cfg,
                    work_dir.as_path(),
                    tools.len(),
                    no_stream,
                ) {
                    let _ = style.eprint_error(&err.to_string());
                }
            }
        }
        ReplBuiltIn::Doctor(extra) => {
            if !extra.is_empty() {
                let _ = style.eprint_error("用法: /doctor（无额外参数；同 crabmate doctor）");
            } else {
                let ws = work_dir.to_str();
                let cfg = cfg_holder.read().await;
                crate::runtime::cli_doctor::print_doctor_report(&cfg, ws);
            }
        }
        ReplBuiltIn::Probe(extra) => {
            if !extra.is_empty() {
                let _ = style.eprint_error("用法: /probe（无额外参数；同 crabmate probe）");
            } else {
                return ReplSlashHandled::RunProbe;
            }
        }
        ReplBuiltIn::Models(extra) => {
            if !extra.is_empty() {
                let _ = style.eprint_error("用法: /models（无额外参数；同 crabmate models）");
            } else {
                return ReplSlashHandled::RunModels;
            }
        }
        ReplBuiltIn::WorkspaceShow => match work_dir.canonicalize() {
            Ok(p) => {
                let _ = style.print_line(&format!("当前工作区: {}", p.display()));
            }
            Err(_) => {
                let _ = style.print_line(&format!("当前工作区: {}", work_dir.display()));
            }
        },
        ReplBuiltIn::WorkspaceSet(arg) => {
            let candidate = PathBuf::from(arg);
            let resolved = match candidate.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    let _ = style.eprint_error(&format!("无法解析路径 {arg:?}: {e}"));
                    return ReplSlashHandled::Handled;
                }
            };
            if !resolved.is_dir() {
                let _ = style.eprint_error(&format!("不是目录: {}", resolved.display()));
                return ReplSlashHandled::Handled;
            }
            *work_dir = resolved;
            let _ = style.print_success(&format!("工作区已切换为: {}", work_dir.display()));
        }
        ReplBuiltIn::Tools => {
            if tools.is_empty() {
                let _ = style.print_line("当前未加载工具（可能使用了 --no-tools）。");
            } else {
                let _ = style.print_line(&format!("当前 {} 个工具:", tools.len()));
                for t in tools {
                    let _ = style.print_line(&format!("  · {}", t.function.name));
                }
            }
        }
        ReplBuiltIn::Help => {
            let _ = style.print_help();
        }
        ReplBuiltIn::Export(arg) => {
            let kind = match repl_export_kind_from_arg(arg) {
                Ok(k) => k,
                Err(()) => {
                    let _ = style.eprint_error("用法: /export 或 /export json | markdown | both");
                    return ReplSlashHandled::Handled;
                }
            };
            if let Err(e) = repl_export_current_messages(work_dir, messages, kind, style) {
                let _ = style.eprint_error(&e.to_string());
            }
        }
        ReplBuiltIn::SaveSession(arg) => {
            let kind = match repl_export_kind_from_arg(arg) {
                Ok(k) => k,
                Err(()) => {
                    let _ = style.eprint_error(
                        "用法: /save-session 或 /save-session json | markdown | both",
                    );
                    return ReplSlashHandled::Handled;
                }
            };
            let format = match kind {
                ReplExportKind::Json => SaveSessionFormat::Json,
                ReplExportKind::Markdown => SaveSessionFormat::Markdown,
                ReplExportKind::Both => SaveSessionFormat::Both,
            };
            let cli = SaveSessionCli {
                format,
                session_file: None,
            };
            let ws = Some(work_dir.to_string_lossy().into_owned());
            let cfg = cfg_holder.read().await;
            if let Err(e) = run_save_session_command(&cfg, &ws, cli) {
                let _ = style.eprint_error(&e.to_string());
            }
        }
        ReplBuiltIn::McpList { probe } => {
            return ReplSlashHandled::RunMcpList { probe };
        }
        ReplBuiltIn::McpUnknown(tail) => {
            let _ = style.eprint_error(&format!(
                "未知 /mcp 子命令: {tail}。用法: /mcp · /mcp list · /mcp probe · /mcp list probe"
            ));
        }
        ReplBuiltIn::Version => {
            print_repl_version_line();
        }
    }
    ReplSlashHandled::Handled
}

fn cli_effective_work_dir(workspace_cli: &Option<String>, default: &str) -> PathBuf {
    PathBuf::from(
        workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default),
    )
}

/// CLI（无 SSE、`workspace_is_set` 恒为真）下调用 [`run_agent_turn`] 的固定参数封装。
#[allow(clippy::too_many_arguments)] // CLI 与可选 cli_tool_ctx 并列，聚合为结构体收益有限
async fn run_agent_turn_for_cli(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &Arc<AgentConfig>,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &std::path::Path,
    no_stream: bool,
    cli_tool_ctx: Option<&CliToolRuntime>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ltm, scope) = cli_long_term_memory_handles(cfg);
    run_agent_turn(RunAgentTurnParams {
        client,
        api_key,
        cfg,
        tools,
        messages,
        out: None,
        effective_working_dir: work_dir,
        workspace_is_set: true,
        render_to_terminal: true,
        no_stream,
        cancel: None,
        per_flight: None,
        web_tool_ctx: None,
        cli_tool_ctx,
        plain_terminal_stream: true,
        llm_backend: None,
        temperature_override: None,
        seed_override: LlmSeedOverride::default(),
        long_term_memory: ltm,
        long_term_memory_scope_id: scope,
        read_file_turn_cache: None,
    })
    .await
}

fn cli_long_term_memory_handles(
    cfg: &Arc<AgentConfig>,
) -> (
    Option<std::sync::Arc<crate::long_term_memory::LongTermMemoryRuntime>>,
    Option<String>,
) {
    if !cfg.long_term_memory_enabled {
        return (None, None);
    }
    let path = cfg.long_term_memory_store_sqlite_path.trim();
    let p = if path.is_empty() {
        let base = std::path::Path::new(&cfg.run_command_working_dir).join(".crabmate");
        base.join("long_term_memory.db")
    } else {
        std::path::PathBuf::from(path)
    };
    match crate::long_term_memory::cli_runtime_lazy(&p) {
        Ok(r) => (Some(r), Some("cli".to_string())),
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "CLI 长期记忆库打开失败 path={} error={}",
                p.display(),
                e
            );
            if !CLI_LTM_OPEN_FAILURE_NOTIFIED.swap(true, Ordering::SeqCst) {
                let detail = e.to_string();
                let max = 240usize;
                let (head, tail) = if detail.chars().count() > max {
                    let head: String = detail.chars().take(max).collect();
                    (head, "…")
                } else {
                    (detail, "")
                };
                eprintln!(
                    "crabmate: 警告：配置中已启用长期记忆 (long_term_memory_enabled)，但本进程无法打开 SQLite；长期记忆在本进程中已禁用。\n\
                     路径: {}\n\
                     错误: {}{}\n\
                     请检查目录权限、磁盘空间或向量后端依赖（如 fastembed / ONNX）；若暂不需要可设 long_term_memory_enabled = false。详情见日志 (target=crabmate)。",
                    p.display(),
                    head,
                    tail
                );
            }
            (None, None)
        }
    }
}

fn map_turn_err(e: Box<dyn std::error::Error + Send + Sync>) -> Box<dyn std::error::Error> {
    let s = e.to_string();
    let code = classify_model_error_message(&s);
    Box::new(CliExitError::new(code, s))
}

fn build_cli_runtime(chat: &ChatCliArgs) -> CliToolRuntime {
    let extra: Vec<String> = chat
        .approve_commands
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    CliToolRuntime {
        persistent_allowlist_shared: Arc::new(Mutex::new(HashSet::new())),
        auto_approve_all_non_whitelist_run_command: chat.yes_run_command,
        extra_allowlist_commands: extra.into(),
        command_stats: Arc::new(std::sync::Mutex::new(CliCommandTurnStats::default())),
    }
}

fn resolve_system_prompt_for_chat(
    cfg: &Arc<AgentConfig>,
    chat: &ChatCliArgs,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = chat.system_prompt_file.as_deref() {
        let t = std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --system-prompt-file {p}: {e}"),
            )
        })?;
        return Ok(t);
    }
    Ok(cfg.system_prompt.clone())
}

fn resolve_user_body(chat: &ChatCliArgs) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(p) = chat.user_prompt_file.as_deref() {
        let t = std::fs::read_to_string(p).map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("无法读取 --user-prompt-file {p}: {e}"),
            )
        })?;
        let t = t.trim();
        if t.is_empty() {
            return Err(CliExitError::new(
                EXIT_USAGE,
                "--user-prompt-file 文件内容为空（去空白后）",
            )
            .into());
        }
        return Ok(t.to_string());
    }
    let Some(u) = chat.inline_user_text.as_deref() else {
        return Err(CliExitError::new(EXIT_USAGE, "缺少用户消息").into());
    };
    let u = u.trim();
    if u.is_empty() {
        return Err(
            CliExitError::new(EXIT_USAGE, "--query 或 --stdin 用户内容为空（去空白后）").into(),
        );
    }
    Ok(u.to_string())
}

fn load_messages_json_file(path: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        CliExitError::new(
            EXIT_GENERAL,
            format!("无法读取 --messages-json-file {path}: {e}"),
        )
    })?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| CliExitError::new(EXIT_USAGE, format!("{path}：顶层 JSON 解析失败: {e}")))?;
    let parsed: Vec<Message> = if let Some(a) = v.as_array() {
        serde_json::from_value(serde_json::Value::Array(a.clone())).map_err(|e| {
            CliExitError::new(EXIT_USAGE, format!("{path}：消息数组反序列化失败: {e}"))
        })?
    } else if let Some(m) = v.get("messages") {
        serde_json::from_value(m.clone()).map_err(|e| {
            CliExitError::new(
                EXIT_USAGE,
                format!("{path}：messages 字段反序列化失败: {e}"),
            )
        })?
    } else {
        return Err(CliExitError::new(
            EXIT_USAGE,
            format!("{path}：须为 JSON 数组，或对象 {{\"messages\":[...]}}"),
        )
        .into());
    };
    if parsed.is_empty() {
        return Err(CliExitError::new(EXIT_USAGE, format!("{path}：messages 不能为空")).into());
    }
    Ok(normalize_messages_for_openai_compatible_request(parsed))
}

fn print_json_reply_line(cfg: &Arc<AgentConfig>, messages: &[Message], batch_line: Option<usize>) {
    let reply = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.clone())
        .unwrap_or_default();
    let mut obj = serde_json::json!({
        "type": "crabmate_chat_cli_result",
        "v": 1u32,
        "reply": reply,
        "model": cfg.model,
    });
    if let Some(n) = batch_line {
        obj["batch_line"] = serde_json::json!(n);
    }
    println!("{}", obj);
}

fn ensure_all_run_commands_not_denied(
    cli_rt: &CliToolRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    if cli_rt.all_run_commands_were_denied() {
        return Err(Box::new(CliExitError::new(
            EXIT_TOOLS_ALL_RUN_COMMAND_DENIED,
            "本回合内所有 run_command 均在审批中被拒绝（或未自动批准）",
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_one_cli_turn(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &Arc<AgentConfig>,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    work_dir: &Path,
    no_stream: bool,
    cli_rt: &CliToolRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    run_agent_turn_for_cli(
        client,
        api_key,
        cfg,
        tools,
        messages,
        work_dir,
        no_stream,
        Some(cli_rt),
    )
    .await
    .map_err(map_turn_err)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_chat_batch_jsonl(
    cfg_holder: &SharedAgentConfig,
    _config_path: Option<&str>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    work_dir: &Path,
    no_stream: bool,
    cli_rt: &CliToolRuntime,
    json_out: bool,
    path: &str,
    chat: &ChatCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path).map_err(|e| {
        CliExitError::new(EXIT_GENERAL, format!("无法打开 --message-file {path}: {e}"))
    })?;
    let reader = std::io::BufReader::new(file);
    let system_seed = {
        let g = cfg_holder.read().await;
        resolve_system_prompt_for_chat(&Arc::new(g.clone()), chat)?
    };
    let mut messages: Vec<Message> = Vec::new();
    let mut line_no: usize = 0;
    for line in reader.lines() {
        line_no += 1;
        let line = line.map_err(|e| {
            CliExitError::new(
                EXIT_GENERAL,
                format!("读取 {path} 第 {line_no} 行失败: {e}"),
            )
        })?;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(t).map_err(|e| {
            CliExitError::new(
                EXIT_USAGE,
                format!("{path} 第 {line_no} 行 JSON 解析失败: {e}"),
            )
        })?;
        if let Some(u) = v.get("user").and_then(|x| x.as_str()) {
            let u = u.trim();
            if u.is_empty() {
                return Err(CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：user 为空"),
                )
                .into());
            }
            if messages.is_empty() {
                messages = messages_chat_seed(&system_seed, u);
                prepend_cli_first_turn_injection(cfg_holder, work_dir, &mut messages).await;
            } else {
                messages.push(Message::user_only(u.to_string()));
            }
        } else if let Some(m) = v.get("messages") {
            let parsed: Vec<Message> = serde_json::from_value(m.clone()).map_err(|e| {
                CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：messages 非法: {e}"),
                )
            })?;
            if parsed.is_empty() {
                return Err(CliExitError::new(
                    EXIT_USAGE,
                    format!("{path} 第 {line_no} 行：messages 为空"),
                )
                .into());
            }
            messages = normalize_messages_for_openai_compatible_request(parsed);
        } else {
            return Err(CliExitError::new(
                EXIT_USAGE,
                format!("{path} 第 {line_no} 行：需要字段 `user`（字符串）或 `messages`（数组）"),
            )
            .into());
        }

        let cfg_snap = {
            let g = cfg_holder.read().await;
            Arc::new(g.clone())
        };
        run_one_cli_turn(
            client,
            api_key,
            &cfg_snap,
            tools,
            &mut messages,
            work_dir,
            no_stream,
            cli_rt,
        )
        .await?;
        ensure_all_run_commands_not_denied(cli_rt)?;
        if json_out {
            print_json_reply_line(&cfg_snap, &messages, Some(line_no));
        }
    }
    Ok(())
}

/// `chat` 子命令：单轮、整表 JSON、或 `--message-file` 多轮批跑。
pub async fn run_chat_invocation(
    cfg_holder: &SharedAgentConfig,
    config_path: Option<&str>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    chat: &ChatCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let work_dir = {
        let g = cfg_holder.read().await;
        cli_effective_work_dir(workspace_cli, &g.run_command_working_dir)
    };
    let cli_rt = build_cli_runtime(chat);
    let json_out = chat.output.as_deref().is_some_and(|m| m == "json");

    if let Some(batch_path) = chat.message_file.as_deref() {
        return run_chat_batch_jsonl(
            cfg_holder,
            config_path,
            client,
            api_key,
            tools,
            work_dir.as_path(),
            chat.no_stream,
            &cli_rt,
            json_out,
            batch_path,
            chat,
        )
        .await;
    }

    if let Some(path) = chat.messages_json_file.as_deref() {
        let mut messages = load_messages_json_file(path)?;
        debug!(
            target: "crabmate::print",
            "messages-json-file 已加载 path={} count={}",
            path,
            messages.len()
        );
        let cfg_snap = {
            let g = cfg_holder.read().await;
            Arc::new(g.clone())
        };
        run_one_cli_turn(
            client,
            api_key,
            &cfg_snap,
            tools,
            &mut messages,
            work_dir.as_path(),
            chat.no_stream,
            &cli_rt,
        )
        .await?;
        ensure_all_run_commands_not_denied(&cli_rt)?;
        if json_out {
            print_json_reply_line(&cfg_snap, &messages, None);
        }
        return Ok(());
    }

    let system = {
        let g = cfg_holder.read().await;
        resolve_system_prompt_for_chat(&Arc::new(g.clone()), chat)?
    };
    let user = resolve_user_body(chat)?;
    let mut messages = messages_chat_seed(&system, &user);
    prepend_cli_first_turn_injection(cfg_holder, work_dir.as_path(), &mut messages).await;
    debug!(
        target: "crabmate::print",
        "chat 首轮已构造 system_len={} user_preview={}",
        system.len(),
        redact::preview_chars(&user, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let cfg_snap = {
        let g = cfg_holder.read().await;
        Arc::new(g.clone())
    };
    run_one_cli_turn(
        client,
        api_key,
        &cfg_snap,
        tools,
        &mut messages,
        work_dir.as_path(),
        chat.no_stream,
        &cli_rt,
    )
    .await?;
    ensure_all_run_commands_not_denied(&cli_rt)?;
    if json_out {
        print_json_reply_line(&cfg_snap, &messages, None);
    }
    Ok(())
}

/// 交互式 REPL 模式
pub async fn run_repl(
    cfg_holder: &SharedAgentConfig,
    config_path: Option<&str>,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (run_root, tui_load) = {
        let g = cfg_holder.read().await;
        (
            g.run_command_working_dir.clone(),
            g.tui_load_session_on_start,
        )
    };
    let mut work_dir = cli_effective_work_dir(workspace_cli, &run_root);
    let cli_rt = CliToolRuntime::new_interactive_default();
    let style = CliReplStyle::new();

    {
        let g = cfg_holder.read().await;
        style.print_banner(&g, work_dir.as_path(), tools.len(), no_stream)?;
    }

    // `repl_initial_workspace_messages_enabled` 为 true 时：`initial_workspace_messages` 在独立线程中构建，不阻塞 REPL。
    let (mut messages, initial_pending) = {
        let g = cfg_holder.read().await;
        let fast = crate::runtime::workspace_session::repl_bootstrap_messages_fast(&g);
        if !g.repl_initial_workspace_messages_enabled {
            (fast, None)
        } else {
            let may_scan_workspace = (g.project_profile_inject_enabled
                && g.project_profile_inject_max_chars > 0)
                || (g.project_dependency_brief_inject_enabled
                    && g.project_dependency_brief_inject_max_chars > 0)
                || (g.agent_memory_file_enabled && !g.agent_memory_file.trim().is_empty());
            if may_scan_workspace || tui_load {
                let _ = writeln!(
                    io::stderr(),
                    "（后台正在准备工作区首轮上下文或会话恢复，可立即输入；就绪后将并入对话。）"
                );
                let _ = io::stderr().flush();
            }
            let cfg_bg = g.clone();
            let slot: Arc<StdMutex<Option<Vec<crate::types::Message>>>> =
                Arc::new(StdMutex::new(None));
            let slot_bg = Arc::clone(&slot);
            let wd_bg = work_dir.clone();
            std::thread::spawn(move || {
                let built = crate::runtime::workspace_session::initial_workspace_messages(
                    &cfg_bg,
                    wd_bg.as_path(),
                    tui_load,
                );
                let mut guard = slot_bg.lock().unwrap_or_else(|e| e.into_inner());
                *guard = Some(built);
            });
            (fast, Some(slot))
        }
    };

    let history_dir = PathBuf::from(&run_root).join(".crabmate");
    std::fs::create_dir_all(&history_dir)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let history_file = history_dir.join("repl_history.txt");
    let repl_editor = Arc::new(StdMutex::new(
        ReplLineEditor::new(history_file.as_path())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
    ));

    loop {
        crate::runtime::workspace_session::try_merge_background_initial_workspace(
            &mut messages,
            initial_pending.as_ref(),
        );

        let ed = repl_editor.clone();
        let read_res = tokio::task::spawn_blocking(move || {
            let mut guard = ed.lock().unwrap_or_else(|e| e.into_inner());
            read_repl_line_with_editor(&mut guard)
        })
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        match read_res {
            ReplReadLine::Eof => break,
            ReplReadLine::Empty => continue,
            ReplReadLine::Shell(opt_cmd) => {
                let wd = work_dir.clone();
                let sty = style;
                match tokio::task::spawn_blocking(move || {
                    repl_execute_shell(opt_cmd.as_deref(), wd.as_path(), &sty)
                })
                .await
                {
                    Ok(Ok(())) => continue,
                    Ok(Err(e)) => {
                        let _ = style.eprint_error(&e.to_string());
                        continue;
                    }
                    Err(e) => {
                        let _ = style.eprint_error(&e.to_string());
                        continue;
                    }
                }
            }
            ReplReadLine::Chat(input) => {
                if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
                    break;
                }

                match try_handle_repl_slash_command(
                    input.as_str(),
                    cfg_holder,
                    tools,
                    &mut messages,
                    &mut work_dir,
                    &style,
                    no_stream,
                )
                .await
                {
                    ReplSlashHandled::NotSlash => {}
                    ReplSlashHandled::Handled => continue,
                    ReplSlashHandled::RunProbe => {
                        let g = cfg_holder.read().await;
                        if let Err(e) =
                            crate::runtime::cli_doctor::run_probe_cli(client, &g, api_key.trim())
                                .await
                        {
                            let _ = style.eprint_error(&e.to_string());
                        }
                        continue;
                    }
                    ReplSlashHandled::RunModels => {
                        let g = cfg_holder.read().await;
                        if let Err(e) =
                            crate::runtime::cli_doctor::run_models_cli(client, &g, api_key.trim())
                                .await
                        {
                            let _ = style.eprint_error(&e.to_string());
                        }
                        continue;
                    }
                    ReplSlashHandled::RunMcpList { probe } => {
                        let g = cfg_holder.read().await;
                        crate::runtime::cli_mcp::run_mcp_list(&g, probe, true).await;
                        continue;
                    }
                    ReplSlashHandled::RunConfigReload => {
                        match crate::runtime::config_reload::reload_shared_agent_config(
                            cfg_holder,
                            config_path,
                        )
                        .await
                        {
                            Ok(()) => {
                                let _ = style.print_success(
                                    "配置已热重载（conversation_store_sqlite_path 与 HTTP Client 未重建；详见文档）。",
                                );
                            }
                            Err(e) => {
                                let _ = style.eprint_error(&e);
                            }
                        }
                        continue;
                    }
                }

                crate::runtime::workspace_session::try_merge_background_initial_workspace(
                    &mut messages,
                    initial_pending.as_ref(),
                );
                messages.push(Message::user_only(input.to_string()));
                debug!(
                    target: "crabmate::print",
                    "REPL 用户输入已入队 history_len={} input_preview={}",
                    messages.len(),
                    redact::preview_chars(input.as_str(), redact::MESSAGE_LOG_PREVIEW_CHARS)
                );

                let cfg_snap = {
                    let g = cfg_holder.read().await;
                    Arc::new(g.clone())
                };
                if let Err(e) = run_agent_turn_for_cli(
                    client,
                    api_key,
                    &cfg_snap,
                    tools,
                    &mut messages,
                    work_dir.as_path(),
                    no_stream,
                    Some(&cli_rt),
                )
                .await
                {
                    let _ = style.eprint_error(&format!(
                        "本轮对话失败（可继续输入；异常历史可 /clear 清空）：{}",
                        e
                    ));
                    continue;
                }
            }
        }
    }

    style.print_farewell()?;
    Ok(())
}

#[cfg(test)]
mod repl_slash_tests {
    use super::{ReplBuiltIn, classify_repl_slash_command};

    #[test]
    fn not_slash_is_none() {
        assert!(classify_repl_slash_command("hello").is_none());
    }

    #[test]
    fn bare_slash() {
        assert_eq!(
            classify_repl_slash_command("  /  "),
            Some(ReplBuiltIn::BareSlash)
        );
    }

    #[test]
    fn clear_model_tools_help() {
        assert_eq!(
            classify_repl_slash_command("/CLEAR"),
            Some(ReplBuiltIn::Clear)
        );
        assert_eq!(
            classify_repl_slash_command("/model"),
            Some(ReplBuiltIn::Model)
        );
        assert_eq!(
            classify_repl_slash_command("/tools"),
            Some(ReplBuiltIn::Tools)
        );
        assert_eq!(
            classify_repl_slash_command("/help"),
            Some(ReplBuiltIn::Help)
        );
        assert_eq!(classify_repl_slash_command("/?"), Some(ReplBuiltIn::Help));
        assert_eq!(
            classify_repl_slash_command("/config"),
            Some(ReplBuiltIn::Config(""))
        );
        assert_eq!(
            classify_repl_slash_command("/CONFIG"),
            Some(ReplBuiltIn::Config(""))
        );
        assert_eq!(
            classify_repl_slash_command("/config reload"),
            Some(ReplBuiltIn::Config("reload"))
        );
        assert_eq!(
            classify_repl_slash_command("/config extra"),
            Some(ReplBuiltIn::Config("extra"))
        );
        assert_eq!(
            classify_repl_slash_command("/doctor"),
            Some(ReplBuiltIn::Doctor(""))
        );
        assert_eq!(
            classify_repl_slash_command("/probe"),
            Some(ReplBuiltIn::Probe(""))
        );
        assert_eq!(
            classify_repl_slash_command("/models"),
            Some(ReplBuiltIn::Models(""))
        );
    }

    #[test]
    fn workspace_and_cd() {
        assert_eq!(
            classify_repl_slash_command("/workspace"),
            Some(ReplBuiltIn::WorkspaceShow)
        );
        assert_eq!(
            classify_repl_slash_command("/workspace /tmp"),
            Some(ReplBuiltIn::WorkspaceSet("/tmp"))
        );
        assert_eq!(
            classify_repl_slash_command("  /cd  ./foo  "),
            Some(ReplBuiltIn::WorkspaceSet("./foo"))
        );
    }

    #[test]
    fn unknown() {
        assert_eq!(
            classify_repl_slash_command("/nope"),
            Some(ReplBuiltIn::Unknown("nope"))
        );
    }

    #[test]
    fn mcp_and_version() {
        assert_eq!(
            classify_repl_slash_command("/mcp"),
            Some(ReplBuiltIn::McpList { probe: false })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp list"),
            Some(ReplBuiltIn::McpList { probe: false })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp probe"),
            Some(ReplBuiltIn::McpList { probe: true })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp list probe"),
            Some(ReplBuiltIn::McpList { probe: true })
        );
        assert!(matches!(
            classify_repl_slash_command("/mcp list probe extra"),
            Some(ReplBuiltIn::McpUnknown(_))
        ));
        assert_eq!(
            classify_repl_slash_command("/version"),
            Some(ReplBuiltIn::Version)
        );
    }
}

#[cfg(test)]
mod repl_dollar_tests {
    use super::run_repl_shell_line_sync;
    use crate::runtime::repl_reedline::parse_repl_dollar_shell_line;

    #[test]
    fn parse_not_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("hello"), None);
    }

    #[test]
    fn parse_bare_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("$"), Some(None));
    }

    #[test]
    fn parse_bare_fullwidth_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("\u{ff04}"), Some(None));
    }

    #[test]
    fn parse_fullwidth_dollar_ls() {
        assert_eq!(
            parse_repl_dollar_shell_line("\u{ff04} ls"),
            Some(Some("ls"))
        );
    }

    #[test]
    fn parse_dollar_ls() {
        assert_eq!(parse_repl_dollar_shell_line("$ ls"), Some(Some("ls")));
    }

    #[test]
    fn parse_dollar_leading_space() {
        assert_eq!(
            parse_repl_dollar_shell_line("  $ echo x"),
            Some(Some("echo x"))
        );
    }

    #[test]
    fn shell_true_zero_exit() {
        let dir = std::env::temp_dir();
        let cmd = if cfg!(windows) { "exit /b 0" } else { "true" };
        let code = run_repl_shell_line_sync(cmd, &dir).unwrap();
        assert_eq!(code, 0);
    }
}
