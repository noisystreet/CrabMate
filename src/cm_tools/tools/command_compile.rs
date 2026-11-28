//! 「是否编译命令」判定与已装配命令的可复刻快照。
//!
//! `#1530`：从 `tools/mod.rs` 拆出。

use super::command::{CommandSpec, parse_command_spec};
use super::command::{
    dash_c_script_body, is_shell_dash_c_invocation, peel_cd_prefix_argv_for_shell_policy,
};
use crate::cm_names as tool_names;

/// 已装配命令的纯数据快照（`std::process::Command` 非 `Send`，无法跨线程复用）。
///
/// 后台任务（async）按同一份 `program + args + cwd` 复刻命令，保证后台与前台
/// 的 argv / 工作目录不漂移。
pub(crate) struct AssembledCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    /// 装配时设定的工作目录（`Command::current_dir`）；`None` = 未显式设定。
    pub(crate) cwd: Option<std::path::PathBuf>,
}

/// 从已装配的 [`std::process::Command`] 提取可复刻快照。
pub(crate) fn command_program_and_args(cmd: &std::process::Command) -> AssembledCommand {
    AssembledCommand {
        program: cmd.get_program().to_string_lossy().into_owned(),
        args: cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        cwd: cmd.get_current_dir().map(std::path::Path::to_path_buf),
    }
}

/// 判断本次 run_command 是否为“成功的编译命令”（常见 C/C++ 构建工具且退出码为 0）
pub fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let Ok(spec) = parse_command_spec(args_json, std::path::Path::new(".")) else {
        return false;
    };
    is_compile_command_success_spec(&spec, result)
}

/// [`is_compile_command_success`] 的已解析版本。
#[must_use]
pub fn is_compile_command_success_spec(spec: &CommandSpec, result: &str) -> bool {
    if !is_compile_command_spec(spec) {
        return false;
    }
    let parsed = crate::cm_tool_result::parse_tool_output(tool_names::RUN_COMMAND, result);
    parsed.ok && parsed.exit_code == Some(0)
}

/// 仅判断命令形态是否为编译命令（供后台任务等没有结果正文的消费方）。
#[must_use]
pub fn is_compile_command_spec(spec: &CommandSpec) -> bool {
    if let Some(script) = spec.script.as_deref() {
        return script_contains_compile_command(script);
    }
    let mut cmd = spec.command.trim().to_string();
    let mut args = spec.args.clone();
    // 兼容 `command` 为空、编译工具落在 `args` 首项的旧形态。
    if cmd.is_empty() {
        if args.is_empty() {
            return false;
        }
        cmd = args.remove(0);
    }
    let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(&cmd, &args);
    if is_shell_dash_c_invocation(&cmd, &args) {
        return dash_c_script_body(&args).is_some_and(script_contains_compile_command);
    }
    command_invocation_is_compile(&cmd, &args)
}

fn script_contains_compile_command(script: &str) -> bool {
    for part in script.split(['&', '|', ';', '\n', '\r']) {
        let words = crate::cmd_mate::split_command_line(part);
        let Some((cmd, rest)) = words.split_first() else {
            continue;
        };
        if cmd.trim().is_empty() {
            continue;
        }
        let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(cmd, rest);
        if is_shell_dash_c_invocation(&cmd, &args) {
            if dash_c_script_body(&args).is_some_and(script_contains_compile_command) {
                return true;
            }
            continue;
        }
        if command_invocation_is_compile(&cmd, &args) {
            return true;
        }
    }
    false
}

fn command_invocation_is_compile(cmd: &str, args: &[String]) -> bool {
    let base = cmd
        .rsplit('/')
        .next()
        .unwrap_or(cmd)
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "gcc" | "g++" | "cc" | "c++" | "clang" | "clang++" | "make" | "cmake" | "ninja"
        | "rustc" => true,
        "cargo" => args
            .first()
            .is_some_and(|a| matches!(a.as_str(), "build" | "check" | "rustc")),
        _ => false,
    }
}
