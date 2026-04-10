//! Python 生态工具：ruff、pytest、mypy、uv sync/run、可编辑安装（uv / pip）、临时脚本执行（`python_snippet_run`）。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::output_util;

#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;

const MAX_OUTPUT_LINES: usize = 800;
const MAX_UV_RUN_ARGS: usize = 48;
const MAX_UV_RUN_ARG_LEN: usize = 512;

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

/// `ruff check`：可选相对路径列表（默认 `["."]`），路径须在工作区内且不含 `..`。
pub fn ruff_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_has_python_project(workspace_root) {
        return "ruff check: 跳过（未找到 pyproject.toml / setup.py / setup.cfg / requirements.txt）".to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let paths = match parse_rel_paths(&v, "paths", &["."]) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = Command::new("ruff");
    cmd.arg("check").current_dir(&base);
    for p in &paths {
        cmd.arg(p);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_and_format(cmd, max_output_len, "ruff check")
}

/// `python3 -m pytest`：可选单一路径、`-k` / `-m`、`-q`、maxfail、是否 nocapture。
pub fn pytest_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_has_python_project(workspace_root) {
        return "pytest: 跳过（未找到 pyproject.toml / setup.py / setup.cfg / requirements.txt）"
            .to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    if let Some(p) = v.get("test_path").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
        && !is_safe_rel_path(p)
    {
        return "错误：test_path 必须是工作区内相对路径且不能包含 ..".to_string();
    }

    let mut cmd = Command::new("python3");
    cmd.arg("-m").arg("pytest").current_dir(&base);

    if let Some(p) = v.get("test_path").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
    {
        cmd.arg(p);
    }

    if let Some(k) = v.get("keyword").and_then(|x| x.as_str()).map(str::trim)
        && !k.is_empty()
    {
        if !is_safe_py_expr(k) {
            return "错误：keyword（-k）含不允许字符（禁止 shell 元字符与换行）".to_string();
        }
        cmd.arg("-k").arg(k);
    }
    if let Some(m) = v.get("markers").and_then(|x| x.as_str()).map(str::trim)
        && !m.is_empty()
    {
        if !is_safe_py_expr(m) {
            return "错误：markers（-m）含不允许字符".to_string();
        }
        cmd.arg("-m").arg(m);
    }
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

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_and_format(cmd, max_output_len, "python3 -m pytest")
}

/// `mypy`：可选相对路径列表（默认 `["."]`）。
pub fn mypy_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_has_python_project(workspace_root) {
        return "mypy: 跳过（未找到 pyproject.toml / setup.py / setup.cfg / requirements.txt）"
            .to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let paths = match parse_rel_paths(&v, "paths", &["."]) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = Command::new("mypy");
    if v.get("strict").and_then(|x| x.as_bool()).unwrap_or(false) {
        cmd.arg("--strict");
    }
    cmd.current_dir(&base);
    for p in &paths {
        cmd.arg(p);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_and_format(cmd, max_output_len, "mypy")
}

/// 在工作区根执行 `uv sync`（须存在 `pyproject.toml`）。
pub fn uv_sync(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_root.join("pyproject.toml").is_file() {
        return "uv sync: 跳过（工作区根未找到 pyproject.toml）".to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = Command::new("uv");
    cmd.arg("sync").current_dir(&base);
    if v.get("frozen").and_then(|x| x.as_bool()).unwrap_or(false) {
        cmd.arg("--frozen");
    }
    if v.get("no_dev").and_then(|x| x.as_bool()).unwrap_or(false) {
        cmd.arg("--no-dev");
    }
    if v.get("all_packages")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        cmd.arg("--all-packages");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_and_format(cmd, max_output_len, "uv sync")
}

/// 在工作区根执行 `uv run <args...>`：`args` 为**非空**字符串数组，逐项传给子进程（不经 shell）。
/// 每项须通过保守字符校验，禁止空白与 shell 元字符。
pub fn uv_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    if !workspace_root.join("pyproject.toml").is_file() {
        return "uv run: 跳过（工作区根未找到 pyproject.toml）".to_string();
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(arr) = v.get("args").and_then(|x| x.as_array()) else {
        return "错误：缺少 args 数组（至少一项，如 [\"pytest\",\"-q\"]）".to_string();
    };
    if arr.is_empty() || arr.len() > MAX_UV_RUN_ARGS {
        return format!("错误：args 须非空且最多 {} 项", MAX_UV_RUN_ARGS);
    }
    let mut argv: Vec<String> = Vec::new();
    for x in arr {
        let s = match x.as_str() {
            Some(t) => t.trim(),
            None => return "错误：args 须全部为字符串".to_string(),
        };
        if !is_safe_uv_run_arg(s) {
            return format!(
                "错误：非法参数项（禁止空白与 shell 元字符，单段最长 {} 字符）：{:?}",
                MAX_UV_RUN_ARG_LEN, s
            );
        }
        argv.push(s.to_string());
    }

    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = Command::new("uv");
    cmd.arg("run").current_dir(&base);
    for a in &argv {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let title = format!(
        "uv run {}",
        argv.iter().take(4).cloned().collect::<Vec<_>>().join(" ")
    );
    let title = if argv.len() > 4 {
        format!("{} …(+{} 项)", title, argv.len() - 4)
    } else {
        title
    };
    run_and_format(cmd, max_output_len, &title)
}

/// 在工作区根执行可编辑安装：`uv pip install -e .` 或 `python3 -m pip install -e .`。
pub fn python_install_editable(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let backend = match v.get("backend").and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if s == "uv" || s == "pip" => s,
        _ => return "错误：backend 须为 \"uv\" 或 \"pip\"".to_string(),
    };

    if !workspace_root.join("pyproject.toml").is_file()
        && !workspace_root.join("setup.py").is_file()
    {
        return "错误：可编辑安装需要工作区根目录存在 pyproject.toml 或 setup.py".to_string();
    }

    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    let mut cmd = if backend == "uv" {
        let mut c = Command::new("uv");
        c.args(["pip", "install", "-e", "."]);
        c
    } else {
        let mut c = Command::new("python3");
        c.args(["-m", "pip", "install", "-e", "."]);
        c
    };
    cmd.current_dir(&base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let title = if backend == "uv" {
        "uv pip install -e ."
    } else {
        "python3 -m pip install -e ."
    };
    run_and_format(cmd, max_output_len, title)
}

/// 在工作区根目录写入**临时** `.py` 并执行（结束后删除）。允许 `import` 任意已安装的第三方包。
///
/// - 默认：`python3 <脚本>`，`PYTHONPATH` 含工作区根（可 `import` 工作区内包）。
/// - `use_uv: true` 且存在 `pyproject.toml`：`uv run python <脚本>`，使用项目/锁文件环境。
/// - 墙上时钟超时：默认 `command_timeout_secs`，可用 `timeout_secs` 覆盖（1～600 秒）。
pub fn python_snippet_run(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    command_timeout_secs: u64,
) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let code = match v.get("code").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return "错误：缺少非空字符串参数 code（Python 源码）".to_string(),
    };
    if code.len() > MAX_PYTHON_SNIPPET_BYTES {
        return format!(
            "错误：code 过长（上限 {} 字节，当前 {} 字节）",
            MAX_PYTHON_SNIPPET_BYTES,
            code.len()
        );
    }

    let use_uv = v.get("use_uv").and_then(|x| x.as_bool()).unwrap_or(false);
    let wall_secs = v
        .get("timeout_secs")
        .and_then(|x| x.as_u64())
        .unwrap_or(command_timeout_secs)
        .clamp(
            MIN_PYTHON_SNIPPET_TIMEOUT_SECS,
            MAX_PYTHON_SNIPPET_TIMEOUT_SECS,
        );

    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    if use_uv && !base.join("pyproject.toml").is_file() {
        return "错误：use_uv 为 true 时需要工作区根存在 pyproject.toml（与 uv_run 一致）"
            .to_string();
    }

    let tmp = match tempfile::Builder::new()
        .prefix(".crabmate_snippet_")
        .suffix(".py")
        .tempfile_in(&base)
    {
        Ok(t) => t,
        Err(e) => return format!("无法在工作区创建临时脚本: {}", e),
    };

    let script_path = match tmp.path().canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("临时脚本路径无法解析: {}", e),
    };
    if !script_path.starts_with(&base) {
        return "错误：临时脚本路径超出工作区根（安全拒绝）".to_string();
    }

    let mut file = match tmp.reopen() {
        Ok(f) => f,
        Err(e) => return format!("无法写入临时脚本: {}", e),
    };
    if let Err(e) = file.write_all(code.as_bytes()) {
        return format!("无法写入临时脚本: {}", e);
    }
    if let Err(e) = file.sync_all() {
        return format!("无法落盘临时脚本: {}", e);
    }
    drop(file);

    let rel_display = script_path
        .strip_prefix(&base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| script_path.to_string_lossy().into_owned());

    let mut cmd = if use_uv {
        let mut c = Command::new("uv");
        c.args(["run", "python"]);
        c.arg(&script_path);
        c
    } else {
        let mut c = Command::new("python3");
        c.arg(&script_path);
        c
    };
    let mut cmd = cmd.current_dir(&base);
    if !use_uv {
        cmd = cmd.env("PYTHONPATH", pythonpath_with_workspace(&base));
    }
    let cmd = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let title = if use_uv {
        format!("uv run python ({})", rel_display)
    } else {
        format!("python3 ({})", rel_display)
    };

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return output_util::format_spawn_error(
                &title,
                &e,
                output_util::CommandSpawnErrorStyle::CannotStartWithPathHint,
            );
        }
    };

    let child_pid = child.id();
    let (tx, rx) = mpsc::channel::<std::io::Result<std::process::Output>>();
    let join = thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let deadline = Instant::now() + Duration::from_secs(wall_secs);
    let output = loop {
        let now = Instant::now();
        if now >= deadline {
            python_snippet_hard_kill(child_pid);
            let _ = join.join();
            let body = format!(
                "已超出墙上时钟上限（{} 秒）；子进程已发送终止信号。请在更小数据集上重试或调大 timeout_secs（上限 {}）。",
                wall_secs, MAX_PYTHON_SNIPPET_TIMEOUT_SECS
            );
            return output_util::format_exited_command_output(
                &title,
                -1,
                &body,
                max_output_len,
                MAX_OUTPUT_LINES,
            );
        }
        let step = deadline
            .saturating_duration_since(now)
            .min(Duration::from_millis(200));
        match rx.recv_timeout(step) {
            Ok(Ok(out)) => {
                let _ = join.join();
                break out;
            }
            Ok(Err(e)) => {
                let _ = join.join();
                return format!("{}: 收集子进程输出失败（{}）", title, e);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = join.join();
                return format!("{}: 等待子进程输出的线程已结束但未返回结果", title);
            }
        }
    };

    let code = output.status.code().unwrap_or(-1);
    let body = output_util::merge_process_output(
        &output,
        output_util::ProcessOutputMerge::StderrElseStdout,
    );
    output_util::format_exited_command_output(&title, code, &body, max_output_len, MAX_OUTPUT_LINES)
}

#[cfg(unix)]
fn python_snippet_hard_kill(pid: u32) {
    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
}

#[cfg(windows)]
fn python_snippet_hard_kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(all(not(unix), not(windows)))]
fn python_snippet_hard_kill(_pid: u32) {}

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

fn is_safe_uv_run_arg(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_UV_RUN_ARG_LEN
        && !s.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    ';' | '|' | '&' | '`' | '$' | '<' | '>' | '(' | ')' | '\'' | '"' | '\\' | '*'
                )
        })
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

fn parse_rel_paths(
    v: &serde_json::Value,
    key: &str,
    default: &[&str],
) -> Result<Vec<String>, String> {
    let arr = match v.get(key) {
        Some(serde_json::Value::Array(a)) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        _ => default.iter().map(|s| (*s).to_string()).collect(),
    };
    for p in &arr {
        if !is_safe_rel_path(p) {
            return Err(format!(
                "错误：{} 中含非法相对路径（须非空、非绝对、不含 ..）：{}",
                key, p
            ));
        }
    }
    Ok(arr)
}

fn run_and_format(cmd: Command, max_output_len: usize, title: &str) -> String {
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::StderrElseStdout,
        output_util::CommandSpawnErrorStyle::CannotStartCommand,
    )
}

/// `ruff format` 单文件（`path` 为相对工作区根的路径）。
pub fn ruff_format_file(
    target: &Path,
    workspace_root: &Path,
    check_only: bool,
) -> Result<String, String> {
    let base = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("目标路径: {}", e))?;
    if !canonical.starts_with(&base) {
        return Err("目标路径不能超出工作区根目录".to_string());
    }
    let relative = canonical
        .strip_prefix(&base)
        .map_err(|_| "路径前缀剥离失败".to_string())?;

    let mut cmd = Command::new("ruff");
    cmd.arg("format");
    if check_only {
        cmd.arg("--check");
    }
    cmd.arg(relative).current_dir(&base);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|e| format!("无法执行 ruff format：{}（请确认已安装 ruff）", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim_end().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim_end().to_string()
        } else {
            String::new()
        };
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!("\n{}", detail)
        };
        let phase = if check_only {
            "ruff format --check"
        } else {
            "ruff format"
        };
        return Err(format!(
            "{} 失败，退出码：{}{}",
            phase,
            output.status.code().unwrap_or(-1),
            suffix
        ));
    }
    Ok(format!(
        "已使用 ruff format {}：{}",
        if check_only {
            "检查通过"
        } else {
            "格式化"
        },
        relative.to_string_lossy().replace('\\', "/")
    ))
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
    fn uv_run_arg_accepts_pytest_node_id() {
        assert!(is_safe_uv_run_arg("tests/test_x.py::test_foo[case1]"));
        assert!(!is_safe_uv_run_arg("pytest;rm"));
    }

    #[test]
    fn python_snippet_run_executes_trivial_print() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = python_snippet_run(r#"{"code":"print(21 * 2)"}"#, dir.path(), 4096, 30);
        assert!(
            out.contains("42"),
            "expected stdout 42 in output, got: {out:?}"
        );
        let _ = std::fs::remove_dir_all(dir.path());
    }
}
