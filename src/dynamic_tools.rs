//! 运行时动态工具（工作区 `plugins/*.json`）：回合开始按目录扫描并注册，执行时按名称解析。
//!
//! 目标：为“无需重编译即可新增工具”提供最小实现，同时保持现有安全边界（命令仍受
//! `allowed_commands` 白名单约束，执行目录固定在当前工作区）。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use crate::tools::split_command_prefix_if_embedded;
use crate::types::{FunctionDef, Tool};

const DYNAMIC_TOOL_PREFIX: &str = "dyn__";
const DYNAMIC_TOOLS_DIR: &str = "plugins";

#[derive(Debug, Clone, Deserialize)]
struct DynamicToolFile {
    name: String,
    description: String,
    #[serde(default)]
    parameters: Value,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    pass_args_json: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicToolRuntimeDef {
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) pass_args_json: bool,
}

fn plugins_dir(working_dir: &Path) -> PathBuf {
    working_dir.join(DYNAMIC_TOOLS_DIR)
}

pub(crate) fn is_dynamic_tool_name(name: &str) -> bool {
    name.starts_with(DYNAMIC_TOOL_PREFIX)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars().take(max_chars) {
        out.push(c);
    }
    out.push_str("…（已截断）");
    out
}

fn validate_file(spec: &DynamicToolFile, path: &Path) -> Result<(), String> {
    if !is_dynamic_tool_name(spec.name.trim()) {
        return Err(format!(
            "动态工具名必须以 `{DYNAMIC_TOOL_PREFIX}` 开头（文件：{}）",
            path.display()
        ));
    }
    if spec.description.trim().is_empty() {
        return Err(format!(
            "动态工具 description 不能为空（文件：{}）",
            path.display()
        ));
    }
    if !spec.parameters.is_object() {
        return Err(format!(
            "动态工具 parameters 必须是 JSON 对象（文件：{}）",
            path.display()
        ));
    }
    if spec.command.trim().is_empty() {
        return Err(format!(
            "动态工具 command 不能为空（文件：{}）",
            path.display()
        ));
    }
    Ok(())
}

fn load_files(working_dir: &Path) -> Vec<(DynamicToolFile, PathBuf)> {
    let dir = plugins_dir(working_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(target: "crabmate", "读取动态工具文件失败 {}: {}", path.display(), e);
                continue;
            }
        };
        let spec: DynamicToolFile = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "解析动态工具 JSON 失败 {}: {}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        if let Err(e) = validate_file(&spec, &path) {
            log::warn!(target: "crabmate", "{}", e);
            continue;
        }
        out.push((spec, path));
    }
    out
}

pub(crate) fn load_dynamic_tools(working_dir: &Path) -> Vec<Tool> {
    let mut defs = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for (spec, path) in load_files(working_dir) {
        if !seen.insert(spec.name.clone()) {
            log::warn!(
                target: "crabmate",
                "动态工具名重复，已跳过 {}（文件：{}）",
                spec.name,
                path.display()
            );
            continue;
        }
        defs.push(Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: spec.name,
                description: spec.description,
                parameters: spec.parameters,
            },
        });
    }
    defs
}

pub(crate) fn resolve_runtime_def(
    working_dir: &Path,
    name: &str,
) -> Result<Option<DynamicToolRuntimeDef>, String> {
    for (spec, _path) in load_files(working_dir) {
        if spec.name == name {
            return Ok(Some(DynamicToolRuntimeDef {
                command: spec.command,
                args: spec.args,
                pass_args_json: spec.pass_args_json,
            }));
        }
    }
    Ok(None)
}

pub(crate) fn run_dynamic_tool(
    def: &DynamicToolRuntimeDef,
    args_json: &str,
    working_dir: &Path,
    command_max_output_len: usize,
    allowed_commands: &[String],
) -> String {
    let mut prog = def.command.trim().to_string();
    let mut merged_args = def.args.clone();
    split_command_prefix_if_embedded(&mut prog, &mut merged_args);

    let cmd_key = prog.to_ascii_lowercase();
    let allowed = allowed_commands
        .iter()
        .any(|c| c.eq_ignore_ascii_case(cmd_key.as_str()));
    if !allowed {
        return format!(
            "错误：动态工具可执行名 `{}`（来自 command 字段 `{}`）不在 allowed_commands 白名单中",
            prog, def.command
        );
    }

    let mut command = Command::new(&prog);
    command.current_dir(working_dir);
    for a in &merged_args {
        command.arg(a);
    }
    if def.pass_args_json {
        command.arg(args_json);
    }

    let output = match command.output() {
        Ok(o) => o,
        Err(e) => {
            return format!("错误：动态工具执行失败：{}", e);
        }
    };
    let stdout = truncate_chars(
        &String::from_utf8_lossy(&output.stdout),
        command_max_output_len,
    );
    let stderr = truncate_chars(
        &String::from_utf8_lossy(&output.stderr),
        command_max_output_len,
    );
    let exit = output.status.code().unwrap_or(-1);
    format!(
        "{} (exit={exit})\n标准输出：\n{}\n标准错误：\n{}",
        prog, stdout, stderr
    )
}
