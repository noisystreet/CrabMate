//! Python 生态工具：ruff、pytest、mypy、uv sync/run、可编辑安装（uv / pip）。

use std::path::Path;
use std::process::{Command, Stdio};

const MAX_OUTPUT_LINES: usize = 800;
const MAX_UV_RUN_ARGS: usize = 48;
const MAX_UV_RUN_ARG_LEN: usize = 512;

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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
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

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut body = String::new();
            if !stderr.trim().is_empty() {
                body.push_str(stderr.trim_end());
            } else if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            } else {
                body.push_str("(无输出)");
            }
            format!(
                "{} (exit={}):\n{}",
                title,
                status,
                truncate_output(&body, max_output_len)
            )
        }
        Err(e) => format!("{}: 无法启动命令（{}）", title, e),
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if s.len() <= max_bytes && lines.len() <= MAX_OUTPUT_LINES {
        return s.to_string();
    }
    let kept_lines = lines.len().min(MAX_OUTPUT_LINES);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        joined[..max_bytes.min(joined.len())].to_string()
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
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
        target.display()
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
}
