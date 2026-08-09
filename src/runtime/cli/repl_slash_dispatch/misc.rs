//! `/config`、`/workspace`、`/export` 等杂项斜杠命令实现。

use std::path::Path;

use crate::config::SharedAgentConfig;
use crate::config::cli::{SaveSessionCli, SaveSessionFormat};
use crate::runtime::cli::{ReplExportKind, run_save_session_command};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::types::Message;

use super::super::repl_extras::{
    ReplSlashHandled, repl_export_current_messages_with_projection,
    repl_export_kind_and_projection_from_arg,
};

pub(super) async fn slash_config(
    extra: &str,
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    tools: &[crate::types::Tool],
    style: &CliReplStyle,
    no_stream: bool,
) -> ReplSlashHandled {
    let e = extra.trim();
    if e.eq_ignore_ascii_case("reload") {
        return ReplSlashHandled::RunConfigReload;
    }
    if !e.is_empty() {
        let _ = style.eprint_error("用法: /config · /config reload（热重载，见文档）");
        return ReplSlashHandled::Handled;
    }
    let cfg = cfg_holder.read().await;
    if let Err(err) = style.print_repl_config_summary(&cfg, work_dir, tools.len(), no_stream) {
        let _ = style.eprint_error(&err.to_string());
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_doctor(
    extra: &str,
    _cfg_holder: &SharedAgentConfig,
    _work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    if !extra.is_empty() {
        let _ = style.eprint_error("用法: /doctor（无额外参数；同 crabmate doctor）");
        return ReplSlashHandled::Handled;
    }
    ReplSlashHandled::RunDoctor
}

pub(super) fn slash_probe(extra: &str, style: &CliReplStyle) -> ReplSlashHandled {
    if !extra.is_empty() {
        let _ = style.eprint_error("用法: /probe（无额外参数；同 crabmate probe）");
        ReplSlashHandled::Handled
    } else {
        ReplSlashHandled::RunProbe
    }
}

pub(super) fn slash_workspace_show(work_dir: &Path, style: &CliReplStyle) -> ReplSlashHandled {
    match work_dir.canonicalize() {
        Ok(p) => {
            let _ = style.print_line(&format!("当前工作区: {}", p.display()));
        }
        Err(_) => {
            let _ = style.print_line(&format!("当前工作区: {}", work_dir.display()));
        }
    }
    ReplSlashHandled::Handled
}

#[allow(clippy::ptr_arg)]
pub(super) async fn slash_workspace_set(
    cfg_holder: &SharedAgentConfig,
    work_dir: &mut std::path::PathBuf,
    arg: &str,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    match crate::tools::resolve_repl_workspace_switch_path(&cfg, work_dir.as_path(), arg) {
        Ok(resolved) => {
            *work_dir = resolved;
            let _ = style.print_success(&format!("工作区已切换为: {}", work_dir.display()));
        }
        Err(e) => {
            let _ = style.eprint_error(&e.to_string());
        }
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_skills_list(
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    if !cfg.skills.skills_enabled {
        let _ = style.print_line("skills 已关闭（skills_enabled=false）。");
        return ReplSlashHandled::Handled;
    }
    match crate::config::skills::list_skills(cfg.skills.list_opts(work_dir)) {
        Ok(files) if files.is_empty() => {
            let _ = style.print_line("当前未发现 skills。");
        }
        Ok(files) => {
            let _ = style.print_line(&format!(
                "当前 skills（{}）；对话中可用 `/<id> [任务]` 强制调用：",
                files.len()
            ));
            for f in files {
                let id = crate::config::skills_slash::skill_callable_id(&f);
                let name = f.name.as_deref().unwrap_or("（无 frontmatter name）");
                let desc = crate::config::skills::skill_ui_description(&f);
                if desc.is_empty() {
                    let _ = style
                        .print_line(&format!("  - /{id}  →  {}  (name: {name})", f.display_path));
                } else {
                    let _ = style.print_line(&format!(
                        "  - /{id}  →  {}  (name: {name}) — {desc}",
                        f.display_path
                    ));
                }
            }
        }
        Err(e) => {
            let _ = style.eprint_error(&format!("读取 skills 失败：{e}"));
        }
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_tools_list(
    tools: &[crate::types::Tool],
    style: &CliReplStyle,
) -> ReplSlashHandled {
    if tools.is_empty() {
        let _ = style.print_line("当前未加载工具（可能使用了 --no-tools）。");
    } else {
        let _ = style.print_line(&format!("当前 {} 个工具:", tools.len()));
        for t in tools {
            let _ = style.print_line(&format!("  · {}", t.function.name));
        }
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_export(
    arg: &str,
    work_dir: &Path,
    messages: &[Message],
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let (kind, projection) = match repl_export_kind_and_projection_from_arg(arg) {
        Ok(v) => v,
        Err(()) => {
            let _ = style.eprint_error(
                "用法: /export [json|markdown|both] [raw|display]（JSON 默认 raw；display 不可 tool-replay）",
            );
            return ReplSlashHandled::Handled;
        }
    };
    if let Err(e) =
        repl_export_current_messages_with_projection(work_dir, messages, kind, projection, style)
    {
        let _ = style.eprint_error(&e.to_string());
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_save_session(
    arg: &str,
    cfg_holder: &SharedAgentConfig,
    work_dir: &Path,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let (kind, projection) = match repl_export_kind_and_projection_from_arg(arg) {
        Ok(v) => v,
        Err(()) => {
            let _ = style.eprint_error(
                "用法: /save-session [json|markdown|both] [raw|display]（JSON 默认 raw）",
            );
            return ReplSlashHandled::Handled;
        }
    };
    let format = match kind {
        ReplExportKind::Json => SaveSessionFormat::Json,
        ReplExportKind::Markdown => SaveSessionFormat::Markdown,
        ReplExportKind::Both => SaveSessionFormat::Both,
    };
    let projection = match projection {
        crate::runtime::chat_export::JsonExportProjection::Raw => {
            crate::config::cli::SaveSessionProjection::Raw
        }
        crate::runtime::chat_export::JsonExportProjection::Display => {
            crate::config::cli::SaveSessionProjection::Display
        }
    };
    let cli = SaveSessionCli {
        format,
        projection,
        session_file: None,
    };
    let ws = Some(work_dir.to_string_lossy().into_owned());
    let cfg = cfg_holder.read().await;
    if let Err(e) = run_save_session_command(&cfg, &ws, cli) {
        let _ = style.eprint_error(&e.to_string());
    }
    ReplSlashHandled::Handled
}
