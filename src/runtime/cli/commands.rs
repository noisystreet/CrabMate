//! `save-session` / `tool-replay` 子命令（不要求 `API_KEY`）。

use crate::config::AgentConfig;
use crate::config::cli::{
    PluginInitCli, PluginListCli, PluginValidateCli, SaveSessionCli, SaveSessionFormat,
    ToolReplayCli,
};
use crate::runtime::cli::{ReplExportKind, cli_effective_work_dir};
use crate::runtime::cli_exit::{CliExitError, EXIT_TOOL_REPLAY_MISMATCH, EXIT_USAGE};
use std::io::ErrorKind;
use std::path::PathBuf;

/// `crabmate tool-replay export|run`（不要求 API_KEY；重放路径与对话相同执行真实工具，须在可信工作区）。
pub fn run_tool_replay_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cmd: ToolReplayCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
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

/// `crabmate save-session`：从磁盘会话文件读取并写入导出目录（兼容别名 `export-session`）。
pub fn run_save_session_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    args: SaveSessionCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
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

fn plugin_default_output_path(workspace: &std::path::Path, name: &str) -> PathBuf {
    let stem = name.trim_start_matches("dyn__");
    let file_stem = if stem.trim().is_empty() {
        "tool"
    } else {
        stem.trim()
    };
    workspace.join("plugins").join(format!("{file_stem}.json"))
}

#[derive(Debug)]
struct PluginCheckResult {
    path: PathBuf,
    name: Option<String>,
    command: Option<String>,
    errors: Vec<String>,
}

#[derive(serde::Serialize)]
struct PluginCheckJsonRow {
    path: String,
    name: Option<String>,
    command: Option<String>,
    ok: bool,
    errors: Vec<String>,
}

fn collect_plugin_paths(
    workspace: &std::path::Path,
    file: Option<&str>,
) -> std::io::Result<Vec<PathBuf>> {
    if let Some(f) = file.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(vec![PathBuf::from(f)]);
    }
    let dir = workspace.join("plugins");
    let mut v = Vec::new();
    if dir.is_dir() {
        for ent in std::fs::read_dir(&dir)? {
            let p = ent?.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                v.push(p);
            }
        }
    }
    v.sort();
    Ok(v)
}

fn validate_plugin_file(path: &PathBuf, cfg: &AgentConfig) -> PluginCheckResult {
    let mut out = PluginCheckResult {
        path: path.clone(),
        name: None,
        command: None,
        errors: Vec::new(),
    };
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            out.errors.push(format!("读取失败: {e}"));
            return out;
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            out.errors.push(format!("JSON 解析失败: {e}"));
            return out;
        }
    };
    let Some(obj) = v.as_object() else {
        out.errors.push("顶层必须为 JSON 对象".to_string());
        return out;
    };
    let name = obj
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let desc = obj
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let cmd = obj
        .get("command")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let params_is_obj = obj.get("parameters").is_some_and(|x| x.is_object());
    if !name.is_empty() {
        out.name = Some(name.to_string());
    }
    if !cmd.is_empty() {
        out.command = Some(cmd.to_string());
    }
    if !name.starts_with("dyn__") {
        out.errors.push("name 必须以 dyn__ 开头".to_string());
    }
    if desc.is_empty() {
        out.errors.push("description 不能为空".to_string());
    }
    if !params_is_obj {
        out.errors.push("parameters 必须是 JSON 对象".to_string());
    }
    if cmd.is_empty() {
        out.errors.push("command 不能为空".to_string());
    } else if !cfg
        .command_exec
        .allowed_commands
        .iter()
        .any(|c| c.eq_ignore_ascii_case(cmd))
    {
        out.errors
            .push(format!("command `{cmd}` 不在 allowed_commands 白名单"));
    }
    out
}

/// `crabmate plugin init`：在工作区生成动态工具模板 JSON（不要求 API_KEY）。
pub fn run_plugin_init_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cli: PluginInitCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
    let name = cli.name.trim();
    if !name.starts_with("dyn__") {
        return Err(
            CliExitError::new(EXIT_USAGE, "plugin init：--name 必须以 `dyn__` 开头").into(),
        );
    }
    if name.chars().count() > 120 {
        return Err(CliExitError::new(EXIT_USAGE, "plugin init：--name 过长").into());
    }
    let desc = cli
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("动态工具（请补充描述）");
    let command = cli
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("python3");
    let output = cli
        .output
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| plugin_default_output_path(&workspace, name));
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload = serde_json::json!({
        "name": name,
        "description": desc,
        "parameters": {
            "type": "object",
            "properties": {},
            "required": []
        },
        "command": command,
        "args": cli.args,
        "pass_args_json": cli.pass_args_json
    });
    let content = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&output, format!("{content}\n"))?;
    println!("{}", output.display());
    Ok(())
}

/// `crabmate plugin validate`：校验 `plugins/*.json` 动态工具定义与白名单命令。
pub fn run_plugin_validate_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cli: PluginValidateCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
    let paths = collect_plugin_paths(&workspace, cli.file.as_deref())?;
    if paths.is_empty() {
        println!("未发现可校验的动态工具文件");
        return Ok(());
    }

    let mut ok_count = 0usize;
    let mut fail_count = 0usize;
    let mut rows: Vec<PluginCheckJsonRow> = Vec::new();
    for p in paths {
        let checked = validate_plugin_file(&p, cfg);
        let ok = checked.errors.is_empty();
        if ok {
            ok_count += 1;
            if !cli.json && !cli.jsonl {
                println!("OK  {}", p.display());
            }
        } else {
            fail_count += 1;
            if !cli.json && !cli.jsonl {
                eprintln!("FAIL {}: {}", p.display(), checked.errors.join("; "));
            }
        }
        rows.push(PluginCheckJsonRow {
            path: checked.path.display().to_string(),
            name: checked.name,
            command: checked.command,
            ok,
            errors: checked.errors,
        });
    }
    if cli.jsonl {
        for row in &rows {
            println!("{}", serde_json::to_string(row)?);
        }
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "crabmate_plugin_validate_summary",
                "ok_count": ok_count,
                "failed_count": fail_count,
            }))?
        );
    } else if cli.json {
        let payload = serde_json::json!({
            "type": "crabmate_plugin_validate_result",
            "ok_count": ok_count,
            "failed_count": fail_count,
            "rows": rows,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("校验完成：ok={ok_count}, failed={fail_count}");
    }
    if fail_count > 0 {
        return Err(CliExitError::new(EXIT_USAGE, "存在动态工具校验失败").into());
    }
    Ok(())
}

/// `crabmate plugin list`：列出动态工具及校验状态。
pub fn run_plugin_list_command(
    cfg: &AgentConfig,
    workspace_cli: &Option<String>,
    cli: PluginListCli,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace =
        cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
    let paths = collect_plugin_paths(&workspace, cli.file.as_deref())?;
    if paths.is_empty() {
        println!("未发现动态工具文件");
        return Ok(());
    }
    let mut ok_count = 0usize;
    let mut fail_count = 0usize;
    let mut rows: Vec<PluginCheckJsonRow> = Vec::new();
    for p in paths {
        let checked = validate_plugin_file(&p, cfg);
        let status = if checked.errors.is_empty() {
            ok_count += 1;
            "OK"
        } else {
            fail_count += 1;
            "FAIL"
        };
        if !cli.json && !cli.jsonl {
            let name = checked.name.as_deref().unwrap_or("-");
            let cmd = checked.command.as_deref().unwrap_or("-");
            println!(
                "{status}\t{}\tname={name}\tcommand={cmd}",
                checked.path.display()
            );
            if !checked.errors.is_empty() {
                println!("  errors: {}", checked.errors.join("; "));
            }
        }
        rows.push(PluginCheckJsonRow {
            path: checked.path.display().to_string(),
            name: checked.name,
            command: checked.command,
            ok: checked.errors.is_empty(),
            errors: checked.errors,
        });
    }
    if cli.jsonl {
        for row in &rows {
            println!("{}", serde_json::to_string(row)?);
        }
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "crabmate_plugin_list_summary",
                "ok_count": ok_count,
                "failed_count": fail_count,
            }))?
        );
    } else if cli.json {
        let payload = serde_json::json!({
            "type": "crabmate_plugin_list_result",
            "ok_count": ok_count,
            "failed_count": fail_count,
            "rows": rows,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("汇总：ok={ok_count}, failed={fail_count}");
    }
    Ok(())
}
