//! Python 生态工具：ruff、pytest、mypy、临时脚本执行（`python_snippet_run`）。
//!
//! `ruff` / `mypy` / `uv` 统一改用 `run_command`（原 `ruff_check` 聚合器已随 `run_lints` 一并移除）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::output_util;
use crate::cm_tools::subprocess_session::{
    SessionStopKind, SubprocessWaitCtl, prepare_piped_process_group, run_and_capture,
    wait_child_session,
};
use crate::cm_tools::tool_result::ToolError;

const MAX_OUTPUT_LINES: usize = 800;

/// 单次 `python_snippet_run` 源码上限（UTF-8 字节）。
const MAX_PYTHON_SNIPPET_BYTES: usize = 256 * 1024;
const MIN_PYTHON_SNIPPET_TIMEOUT_SECS: u64 = 1;
const MAX_PYTHON_SNIPPET_TIMEOUT_SECS: u64 = 600;

/// 工作区根下是否存在常见 Python 项目标记。
pub fn workspace_has_python_project(root: &Path) -> bool {
    root.join("pyproject.toml").is_file()
        || root.join("setup.py").is_file()
        || root.join("setup.cfg").is_file()
        || root.join("requirements.txt").is_file()
}


/// `python3 -m pytest`：可选单一路径、`-k` / `-m`、`-q`、maxfail、是否 nocapture。
/// `wall_secs = Some(secs)` 时超时对进程组 SIGTERM→SIGKILL 并保留部分输出。
///
/// **退出码口径**：`0`（通过）与 `5`（未收集到用例）算成功；`1`（有测试失败）/ `2`（被中断）/
/// `3`（内部错误）/ `4`（用法错误）为 `pytest_failed`——与 `cargo_test` 的「测试失败即
/// `cargo_test_failed`」同口径，失败与否不再由正文关键词推断。
#[allow(clippy::result_large_err)]
pub fn pytest_run_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    wall_secs: Option<u64>,
) -> Result<String, ToolError> {
    match build_pytest_command(args_json, workspace_root)? {
        PytestCommand::Skipped(msg) => Ok(msg),
        PytestCommand::Run(cmd) => run_and_format_try(
            cmd,
            max_output_len,
            "python3 -m pytest",
            "pytest",
            wall_secs,
            PYTEST_OK_EXITS,
        ),
    }
}

/// `pytest` 视为成功的退出码：`0` 全部通过，`5` 未收集到用例——「这个仓库没有测试」
/// 是合法答案（模型会据此换 `test_path`），不该报成工具故障。`1`（测试失败）**不在**其中。
const PYTEST_OK_EXITS: &[i32] = &[0, 5];

/// [`build_pytest_command`] 的两种结果。
///
/// `Skipped` 不是失败：工作区没有 Python 工程标记时「跳过」是正常语义（审计表按 `Legal` 钉住）。
/// 若把它塞进 `Err`，迁移后这一支就从「成功」翻成「失败」，属于迁移带来的假回归。
enum PytestCommand {
    Run(Command),
    Skipped(String),
}

/// 装配 `python3 -m pytest`（项目标记检查 + 参数校验 + CLI 拼装）；
/// 同步执行与后台任务（`pytest_run_background_argv`）共用，避免两条路径漂移。
#[allow(clippy::result_large_err)]
fn build_pytest_command(
    args_json: &str,
    workspace_root: &Path,
) -> Result<PytestCommand, ToolError> {
    if !workspace_has_python_project(workspace_root) {
        return Ok(PytestCommand::Skipped(
            "pytest: 跳过（未找到 pyproject.toml / setup.py / setup.cfg / requirements.txt）"
                .to_string(),
        ));
    }
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let base = workspace_root.canonicalize().map_err(|e| {
        ToolError::external_code(
            "workspace_resolve_failed",
            format!("工作区根目录无法解析: {}", e),
        )
    })?;

    if let Some(err) = pytest_validate_test_path(&v) {
        return Err(ToolError::invalid_args(err));
    }

    let mut cmd = Command::new("python3");
    cmd.arg("-m").arg("pytest").current_dir(&base);

    pytest_append_test_path_arg(&v, &mut cmd);

    if let Some(err) = pytest_append_keyword_and_marker_args(&v, &mut cmd) {
        return Err(ToolError::invalid_args(err));
    }

    pytest_append_common_pytest_cli_flags(&v, &mut cmd);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(PytestCommand::Run(cmd))
}

/// 后台任务（async）路径：复用同步路径的项目检查 / 参数校验 / CLI 拼装，仅返回可复刻
/// 快照（`program + args + cwd`，其中 `cwd` 与前台同为 canonicalize 后的工作区根）。
pub(crate) fn pytest_run_background_argv(
    args_json: &str,
    workspace_root: &Path,
) -> Result<crate::cm_tools::tools::AssembledCommand, String> {
    // 后台路径只需要「可复刻 argv」：`Skipped`（无 Python 工程标记）没有命令可跑，
    // 按错误返回（与旧实现一致——以前它就返回 `Err(跳过文案)`）。
    match build_pytest_command(args_json, workspace_root).map_err(|e| e.message)? {
        PytestCommand::Run(cmd) => Ok(crate::cm_tools::tools::command_program_and_args(&cmd)),
        PytestCommand::Skipped(msg) => Err(msg),
    }
}

fn pytest_validate_test_path(v: &serde_json::Value) -> Option<String> {
    if let Some(p) = v.get("test_path").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
        && !is_safe_rel_path(p)
    {
        return Some("错误：test_path 必须是工作区内相对路径且不能包含 ..".to_string());
    }
    None
}

fn pytest_append_test_path_arg(v: &serde_json::Value, cmd: &mut Command) {
    if let Some(p) = v.get("test_path").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
    {
        cmd.arg(p);
    }
}

fn pytest_append_keyword_and_marker_args(
    v: &serde_json::Value,
    cmd: &mut Command,
) -> Option<String> {
    if let Some(k) = v.get("keyword").and_then(|x| x.as_str()).map(str::trim)
        && !k.is_empty()
    {
        if !is_safe_py_expr(k) {
            return Some("错误：keyword（-k）含不允许字符（禁止 shell 元字符与换行）".to_string());
        }
        cmd.arg("-k").arg(k);
    }
    if let Some(m) = v.get("markers").and_then(|x| x.as_str()).map(str::trim)
        && !m.is_empty()
    {
        if !is_safe_py_expr(m) {
            return Some("错误：markers（-m）含不允许字符".to_string());
        }
        cmd.arg("-m").arg(m);
    }
    None
}

fn pytest_append_common_pytest_cli_flags(v: &serde_json::Value, cmd: &mut Command) {
    if v.get("quiet").and_then(|x| x.as_bool()).unwrap_or(true) {
        cmd.arg("-q");
    }
    if let Some(n) = v.get("maxfail").and_then(|x| x.as_u64()) {
        cmd.arg("--maxfail").arg(n.to_string());
    }
    if v.get("nocapture")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        cmd.arg("--capture=no");
    }
}

/// `python_snippet_run`：临时脚本路径、命令参数等（`_tmp` 须存活至子进程结束）。
struct PythonSnippetPrep {
    base: PathBuf,
    script_path: PathBuf,
    _tmp: tempfile::NamedTempFile,
    rel_display: String,
    use_uv: bool,
    wall_secs: u64,
}

fn python_snippet_parse_wall_timeout(v: &serde_json::Value, command_timeout_secs: u64) -> u64 {
    v.get("timeout_secs")
        .and_then(|x| x.as_u64())
        .unwrap_or(command_timeout_secs)
        .clamp(
            MIN_PYTHON_SNIPPET_TIMEOUT_SECS,
            MAX_PYTHON_SNIPPET_TIMEOUT_SECS,
        )
}

#[allow(clippy::result_large_err)]
fn python_snippet_prepare_temp_file(
    v: &serde_json::Value,
    code: &str,
    workspace_root: &Path,
    command_timeout_secs: u64,
) -> Result<PythonSnippetPrep, ToolError> {
    let use_uv = v.get("use_uv").and_then(|x| x.as_bool()).unwrap_or(false);
    let wall_secs = python_snippet_parse_wall_timeout(v, command_timeout_secs);

    let base = workspace_root.canonicalize().map_err(|e| {
        ToolError::external_code(
            "workspace_resolve_failed",
            format!("工作区根目录无法解析: {}", e),
        )
    })?;

    if use_uv && !base.join("pyproject.toml").is_file() {
        return Err(ToolError::invalid_args(
            "错误：use_uv 为 true 时需要工作区根存在 pyproject.toml".to_string(),
        ));
    }

    let tmp = tempfile::Builder::new()
        .prefix(".crabmate_snippet_")
        .suffix(".py")
        .tempfile_in(&base)
        .map_err(|e| {
            ToolError::external_code(
                "python_snippet_temp_file_failed",
                format!("无法在工作区创建临时脚本: {}", e),
            )
        })?;

    let script_path = tmp.path().canonicalize().map_err(|e| {
        ToolError::external_code(
            "python_snippet_temp_file_failed",
            format!("临时脚本路径无法解析: {}", e),
        )
    })?;
    if !script_path.starts_with(&base) {
        return Err(ToolError::with_code(
            "python_snippet_temp_path_outside_workspace",
            "错误：临时脚本路径超出工作区根（安全拒绝）".to_string(),
        ));
    }

    let mut file = tmp.reopen().map_err(|e| {
        ToolError::external_code(
            "python_snippet_temp_file_failed",
            format!("无法写入临时脚本: {}", e),
        )
    })?;
    if let Err(e) = file.write_all(code.as_bytes()) {
        return Err(ToolError::external_code(
            "python_snippet_temp_file_failed",
            format!("无法写入临时脚本: {}", e),
        ));
    }
    if let Err(e) = file.sync_all() {
        return Err(ToolError::external_code(
            "python_snippet_temp_file_failed",
            format!("无法落盘临时脚本: {}", e),
        ));
    }
    drop(file);

    let rel_display = script_path
        .strip_prefix(&base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| script_path.to_string_lossy().into_owned());

    Ok(PythonSnippetPrep {
        base,
        script_path,
        _tmp: tmp,
        rel_display,
        use_uv,
        wall_secs,
    })
}

fn python_snippet_build_command(prep: &PythonSnippetPrep) -> (Command, String) {
    let mut cmd = if prep.use_uv {
        let mut c = Command::new("uv");
        c.args(["run", "python"]);
        c.arg(&prep.script_path);
        c
    } else {
        let mut c = Command::new("python3");
        c.arg(&prep.script_path);
        c
    };
    cmd.current_dir(&prep.base);
    if !prep.use_uv {
        cmd.env("PYTHONPATH", pythonpath_with_workspace(&prep.base));
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let title = if prep.use_uv {
        format!("uv run python ({})", prep.rel_display)
    } else {
        format!("python3 ({})", prep.rel_display)
    };
    (cmd, title)
}

/// 在工作区根目录写入**临时** `.py` 并执行（结束后删除）。允许 `import` 任意已安装的第三方包。
///
/// - 默认：`python3 <脚本>`，`PYTHONPATH` 含工作区根（可 `import` 工作区内包）。
/// - `use_uv: true` 且存在 `pyproject.toml`：`uv run python <脚本>`，使用项目/锁文件环境。
/// - 墙上时钟超时：默认 `command_timeout_secs`，可用 `timeout_secs` 覆盖（1～600 秒）。
/// - 执行走共享子进程会话（`subprocess_session`）：并发排空管道、超时对**进程组** SIGTERM→SIGKILL（不再只杀直接 pid）。
///
/// 失败语义：参数 / `use_uv` 缺 `pyproject.toml` → `invalid_args`；工作区根不可解析 →
/// `workspace_resolve_failed`；临时脚本 IO → `python_snippet_temp_file_failed`；
/// 脚本路径越出工作区根 → `python_snippet_temp_path_outside_workspace`（安全拒绝）；
/// 解释器起不来 → `command_not_found` / `spawn_failed`；超时 → `timeout`；取消 → `cancelled`；
/// **脚本非零退出** → `python_snippet_failed`。
#[allow(clippy::result_large_err)]
pub fn python_snippet_run_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    command_timeout_secs: u64,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let code = python_snippet_parse_code(&v)?;
    let prep = python_snippet_prepare_temp_file(&v, code, workspace_root, command_timeout_secs)?;

    let (mut cmd, title) = python_snippet_build_command(&prep);
    prepare_piped_process_group(&mut cmd);
    let child = python_snippet_spawn(&mut cmd, prep.use_uv, &title)?;

    let ctl = SubprocessWaitCtl::with_wall_secs(prep.wall_secs);
    // 路线 A（strangler 过渡）：`python_snippet_run` 仍在同步 `run_tool` 面执行，沿用 `subprocess_session`
    // 同步等待入口；见 `docs/design/subprocess_session_tokio_migration_todo.md`（PR3）。
    let session = wait_child_session(child, &ctl, max_output_len).map_err(|e| {
        ToolError::external_code(
            "python_snippet_wait_failed",
            format!("{}: 收集子进程输出失败（{}）", title, e),
        )
    })?;

    python_snippet_outcome(&session, &title, prep.wall_secs, max_output_len)
}

/// 取 `code` 并做非空 / 长度校验（都是调用方可修的 `invalid_args`）。
#[allow(clippy::result_large_err)]
fn python_snippet_parse_code(v: &serde_json::Value) -> Result<&str, ToolError> {
    let code = v
        .get("code")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ToolError::invalid_args("错误：缺少非空字符串参数 code（Python 源码）".to_string())
        })?;
    if code.len() > MAX_PYTHON_SNIPPET_BYTES {
        return Err(ToolError::invalid_args(format!(
            "错误：code 过长（上限 {} 字节，当前 {} 字节）",
            MAX_PYTHON_SNIPPET_BYTES,
            code.len()
        )));
    }
    Ok(code)
}

/// 拉起解释器；`NotFound` 单独给 `command_not_found`（带安装提示），其余是 `spawn_failed`。
#[allow(clippy::result_large_err)]
fn python_snippet_spawn(
    cmd: &mut Command,
    use_uv: bool,
    title: &str,
) -> Result<std::process::Child, ToolError> {
    cmd.spawn().map_err(|e| {
        let prog = if use_uv { "uv" } else { "python3" };
        let text = output_util::format_spawn_error_with_program(
            title,
            &e,
            output_util::CommandSpawnErrorStyle::CannotStartWithPathHint,
            Some(prog),
        );
        // 与 `output_util::run_command_output_try` 同一口径：`NotFound` 单独给安装提示码。
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            "command_not_found"
        } else {
            "spawn_failed"
        };
        ToolError::external_code(code, text)
    })
}

/// 会话终止 → `Ok(正文)` 或显式失败（非零退出 / 超时 / 取消）。
#[allow(clippy::result_large_err)]
fn python_snippet_outcome(
    session: &crate::cm_tools::subprocess_session::SessionWaitResult,
    title: &str,
    wall_secs: u64,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let exit_code = session.status.and_then(|s| s.code()).unwrap_or(-1);
    let body = match session.kind {
        SessionStopKind::Exited => {
            snippet_merge_stderr_else_stdout(&session.stdout, &session.stderr)
        }
        SessionStopKind::Timeout => format!(
            "已超出墙上时钟上限（{} 秒）；子进程已发送终止信号。请在更小数据集上重试或调大 timeout_secs（上限 {}）。",
            wall_secs, MAX_PYTHON_SNIPPET_TIMEOUT_SECS
        ),
        SessionStopKind::Cancelled => "命令已取消；子进程已发送终止信号。".to_string(),
    };
    let text = output_util::format_exited_command_output(
        title,
        exit_code,
        &body,
        max_output_len,
        MAX_OUTPUT_LINES,
    );
    match session.kind {
        SessionStopKind::Exited if exit_code == 0 => Ok(text),
        SessionStopKind::Exited => Err(ToolError::subprocess_failed(
            "python_snippet",
            exit_code,
            text,
        )),
        SessionStopKind::Timeout => Err(ToolError::session_stop("timeout", text)),
        SessionStopKind::Cancelled => Err(ToolError::session_stop("cancelled", text)),
    }
}

/// `stderr` 优先、否则 `stdout`；两者为空返回 `(无输出)`（与历史 `ProcessOutputMerge::StderrElseStdout` 一致）。
fn snippet_merge_stderr_else_stdout(stdout: &[u8], stderr: &[u8]) -> String {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr);
    if !err.trim().is_empty() {
        err.trim_end().to_string()
    } else if !out.trim().is_empty() {
        out.trim_end().to_string()
    } else {
        "(无输出)".to_string()
    }
}

/// 在已有 `PYTHONPATH` 前追加工作区根，便于 `import` 本地包（仅 `python3` 路径使用）。
fn pythonpath_with_workspace(workspace_root: &Path) -> String {
    let root = workspace_root.to_string_lossy();
    match std::env::var("PYTHONPATH") {
        Ok(prev) if !prev.trim().is_empty() => format!("{}:{}", root, prev),
        _ => root.into_owned(),
    }
}

fn is_safe_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.contains("..")
}

/// 用于 pytest `-k` / `-m` 的保守校验（避免注入）。
fn is_safe_py_expr(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.chars().any(|c| {
            matches!(
                c,
                ';' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '&' | '\n' | '\r'
            )
        })
}


/// 经共享子进程会话运行并格式化输出；`tool_code` 拼成 `{tool_code}_failed`，
/// `allowed_exit` 列出「非零但语义正常」的退出码。
///
/// 与 `cargo_subcommand::run_and_format_try` 同构（那边不需要 `allowed_exit`，cargo 的
/// 子命令没有这种退出码）；同一份逻辑第三次出现时应抽到公共模块，而不是再抄一遍。
#[allow(clippy::result_large_err)]
fn run_and_format_try(
    cmd: Command,
    max_output_len: usize,
    title: &str,
    tool_code: &str,
    wall_secs: Option<u64>,
    allowed_exit: &[i32],
) -> Result<String, ToolError> {
    let program_hint = cmd.get_program().to_str().map(|s| {
        Path::new(s)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| s.to_string())
    });
    // 路线 A（strangler 过渡）：ruff / pytest / mypy 仍在同步 `run_tool` 面执行，沿用 `subprocess_session`
    // 同步等待入口；见 `docs/design/subprocess_session_tokio_migration_todo.md`（PR3）。
    match run_and_capture(cmd, max_output_len, wall_secs) {
        Ok(session) => {
            let stdout = String::from_utf8_lossy(&session.stdout);
            let stderr = String::from_utf8_lossy(&session.stderr);
            // 与历史 `StderrElseStdout` 合并一致；空输出显示 `(无输出)`。
            let mut body = String::new();
            if !stderr.trim().is_empty() {
                body.push_str(stderr.trim_end());
            } else if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            } else {
                body.push_str("(无输出)");
            }
            match session.kind {
                SessionStopKind::Exited => {
                    let code = session.status.and_then(|s| s.code()).unwrap_or(-1);
                    let text = output_util::format_exited_command_output(
                        title,
                        code,
                        &body,
                        max_output_len,
                        MAX_OUTPUT_LINES,
                    );
                    if code == 0 || allowed_exit.contains(&code) {
                        Ok(text)
                    } else {
                        Err(ToolError::subprocess_failed(tool_code, code, text))
                    }
                }
                SessionStopKind::Timeout | SessionStopKind::Cancelled => {
                    let (code, head) = if matches!(session.kind, SessionStopKind::Timeout) {
                        (
                            "timeout",
                            format!(
                                "{} 命令执行超时（{} 秒）；子进程已发送终止信号。",
                                title,
                                wall_secs.unwrap_or_default()
                            ),
                        )
                    } else {
                        (
                            "cancelled",
                            format!("{} 命令已取消；子进程已发送终止信号。", title),
                        )
                    };
                    // `session_stop` 保留 `parsed` 的 stdout/stderr（供信封失败路径免再解析）。
                    Err(ToolError::session_stop(
                        code,
                        format!(
                            "{head}\n{}",
                            output_util::format_marked_streams_block(
                                &stdout,
                                &stderr,
                                max_output_len,
                                MAX_OUTPUT_LINES,
                            )
                        ),
                    ))
                }
            }
        }
        Err(e) => {
            let text = output_util::format_spawn_error_with_program(
                title,
                &e,
                output_util::CommandSpawnErrorStyle::CannotStartCommand,
                program_hint.as_deref(),
            );
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                "command_not_found"
            } else {
                "spawn_failed"
            };
            Err(ToolError::external_code(code, text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_expr_rejects_shellish() {
        assert!(!is_safe_py_expr("foo; rm -rf"));
        assert!(is_safe_py_expr("test_foo and not slow"));
    }

    #[test]
    fn python_snippet_run_executes_trivial_print() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = python_snippet_run_try(r#"{"code":"print(21 * 2)"}"#, dir.path(), 4096, 30)
            .expect("简单脚本应成功");
        assert!(
            out.contains("42"),
            "expected stdout 42 in output, got: {out:?}"
        );
        let _ = std::fs::remove_dir_all(dir.path());
    }

    /// 脚本非零退出是显式失败（以前只有正文，`exit=1` 靠关键词判成败）。
    #[test]
    fn python_snippet_nonzero_exit_reports_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let e = python_snippet_run_try(r#"{"code":"raise SystemExit(3)"}"#, dir.path(), 4096, 30)
            .expect_err("非零退出应失败");
        assert_eq!(e.code, "python_snippet_failed");
        assert!(e.message.contains("exit=3"), "{}", e.message);
    }

    /// 缺 `code` 是参数错误（旧实现返回无「错误：」前缀的裸串时会被启发式判成功）。
    #[test]
    fn python_snippet_missing_code_is_invalid_args() {
        let dir = tempfile::tempdir().expect("tempdir");
        let e = python_snippet_run_try(r#"{"code":"  "}"#, dir.path(), 4096, 30)
            .expect_err("空 code 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("code"), "{}", e.message);
    }

    /// 没有 Python 工程标记时 `pytest_run` **跳过**而不是失败（审计表按 `Legal` 钉住的语义）。
    #[test]
    fn pytest_without_project_marker_is_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = pytest_run_try("{}", dir.path(), 4096, Some(30)).expect("跳过应是成功");
        assert!(out.contains("pytest: 跳过"), "{out}");
    }

    /// `keyword` 里的 shell 元字符是参数错误。
    #[test]
    fn pytest_rejects_shellish_keyword() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("pyproject.toml"), "").expect("write pyproject");
        let e = pytest_run_try(r#"{"keyword":"a; rm -rf /"}"#, dir.path(), 4096, Some(30))
            .expect_err("非法 keyword 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("keyword"), "{}", e.message);
    }

    #[cfg(unix)]
    #[test]
    fn python_snippet_timeout_kills_process_group() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = format!("cm_py_pg_{}", std::process::id());
        // python 孙进程（bash -c 'sleep …'）与 python 同进程组；超时后应一并被终止。
        let code = format!(
            "import subprocess, time\nsubprocess.Popen(['bash','-c','sleep 60 # {marker}'])\ntime.sleep(60)"
        );
        // JSON 字符串内不允许裸换行，转义后再嵌入。
        let code_json = code.replace('\n', "\\n");
        let e = python_snippet_run_try(
            &format!(r#"{{"code":"{}","timeout_secs":1}}"#, code_json),
            dir.path(),
            4096,
            30,
        )
        .expect_err("超时应失败");
        assert_eq!(e.code, "timeout");
        assert!(e.message.contains("墙上时钟"), "{}", e.message);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            !crate::cm_tools::subprocess_session::proc_cmdline_contains(&marker),
            "python 孙进程 sleep 仍在运行（进程组未杀干净）"
        );
        let _ = std::fs::remove_dir_all(dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn run_and_format_timeout_kills_and_keeps_partial_output() {
        // `pytest_run` 等共用 run_and_format：超时对进程组 SIGTERM→SIGKILL，
        // 返回正文含「超时」说明与已捕获部分输出。
        let marker = format!("cm_py_runfmt_{}", std::process::id());
        let mut cmd = Command::new("bash");
        cmd.args([
            "-c",
            &format!("echo partial-py-out; sleep 60 # {marker}"),
        ]);
        let e = run_and_format_try(
            cmd,
            4096,
            "python3 -m pytest",
            "pytest",
            Some(1),
            PYTEST_OK_EXITS,
        )
        .expect_err("超时应失败");
        assert_eq!(e.code, "timeout");
        assert!(e.message.contains("命令执行超时"), "{}", e.message);
        assert!(e.message.contains("partial-py-out"), "{}", e.message);
        assert!(
            e.parsed.stdout.contains("partial-py-out"),
            "legacy 解析应保留部分 stdout: {:?}",
            e.parsed
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            !crate::cm_tools::subprocess_session::proc_cmdline_contains(&marker),
            "run_and_format 孙进程 sleep 仍在运行（进程组未杀干净）"
        );
    }
}
