//! 写盘前校验：按目标文件扩展名对**将写入的内容**做语法/缩进冒烟，减少"写盘后才暴露坏文件"的往返。
//!
//! 语义（供写盘工具调用 [`precheck_before_write`]）：
//! - 校验失败默认**拒写**，返回编译器风格的 `path:行: 信息` 错误（首行含 `校验失败（CODE）`，便于
//!   `tool_result` 分类为 `write_precheck_failed` 与模型扫读）；
//! - `skip_precheck=true` 可显式绕过（渐进式编辑 / 中间态）；
//! - 校验器缺失（如无 `python3`）、启动失败或超时 → **跳过**，不阻塞写盘。

use std::path::Path;
use std::time::Duration;

/// 写盘前校验结论。
#[derive(Debug)]
pub enum PrecheckVerdict {
    /// 通过（或该类型无需校验）。
    Ok,
    /// 跳过（无校验器 / 外部工具缺失 / 超时）；不阻塞写盘。
    Skipped(&'static str),
    /// 失败（含机器可读码与行号），默认拒写。
    Failed(PrecheckFailure),
}

/// 校验失败详情。
#[derive(Debug)]
pub struct PrecheckFailure {
    /// 稳定机器可读码（如 `PY_SYNTAX_ERROR`）。
    pub code: &'static str,
    /// 出错行号（1-based；无法定位时为 `None`）。
    pub line: Option<usize>,
    /// 面向模型的友好信息（编译器风格）。
    pub message: String,
}

impl PrecheckFailure {
    /// 首行形如 `校验失败（CODE）：rel_path:行: 信息`。
    pub fn tool_message(&self, rel_path: &str) -> String {
        let loc = match self.line {
            Some(l) => format!("{rel_path}:{l}"),
            None => rel_path.to_string(),
        };
        format!("校验失败（{}）：{}: {}", self.code, loc, self.message)
    }
}

/// 写盘前校验：按目标相对路径的扩展名选择校验器。
pub fn precheck_write(rel_path: &str, content: &str) -> PrecheckVerdict {
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "py" | "pyw" => check_python(content),
        "json" => check_json(content),
        "toml" => check_toml(content),
        _ => PrecheckVerdict::Ok,
    }
}

/// 供写盘工具调用：`skip_precheck` 为 `true` 或无需校验/校验器缺失时返回 `Ok(())`；
/// 校验失败返回 `Err`（默认拒写）。
pub fn precheck_before_write(
    rel_path: &str,
    content: &str,
    skip_precheck: bool,
) -> Result<(), String> {
    if skip_precheck {
        return Ok(());
    }
    match precheck_write(rel_path, content) {
        PrecheckVerdict::Ok | PrecheckVerdict::Skipped(_) => Ok(()),
        PrecheckVerdict::Failed(f) => Err(f.tool_message(rel_path)),
    }
}

/// 从工具参数 JSON 读取 `skip_precheck`（供手写 `serde_json::Value` 解析的工具使用）。
pub fn arg_skip_precheck(args_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| v.get("skip_precheck").and_then(|x| x.as_bool()))
        .unwrap_or(false)
}

// ── Python：`compile`（stdin 传入；捕获 `IndentationError` 与 `return outside function` 等）──

const PY_COMPILE_CHECK: &str = r#"
import sys
try:
    compile(sys.stdin.read(), "<stdin>", "exec")
except SyntaxError as e:
    print(f"{e.lineno or 0}:{e.offset or 0}: {e.msg}", file=sys.stderr)
    sys.exit(1)
"#;

fn check_python(content: &str) -> PrecheckVerdict {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("python3")
        .args(["-c", PY_COMPILE_CHECK])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return PrecheckVerdict::Skipped("python3 未安装，跳过 Python 语法校验");
        }
        Err(_) => return PrecheckVerdict::Skipped("无法启动 python3，跳过 Python 语法校验"),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(content.as_bytes());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(output)) if output.status.success() => PrecheckVerdict::Ok,
        Ok(Ok(output)) => {
            let err = String::from_utf8_lossy(&output.stderr);
            PrecheckVerdict::Failed(parse_python_syntax_error(err.trim()))
        }
        Ok(Err(_)) => PrecheckVerdict::Skipped("python3 校验执行失败，跳过"),
        Err(_) => PrecheckVerdict::Skipped("python3 校验超时，跳过"),
    }
}

fn parse_python_syntax_error(stderr: &str) -> PrecheckFailure {
    let first = stderr.lines().next().unwrap_or("");
    let mut parts = first.splitn(3, ':');
    let line = parts.next().and_then(|s| s.trim().parse::<usize>().ok());
    let message = parts
        .next()
        .and_then(|_| parts.next())
        .unwrap_or(first)
        .trim()
        .to_string();
    PrecheckFailure {
        code: "PY_SYNTAX_ERROR",
        line,
        message,
    }
}

// ── JSON / TOML：纯内存解析，无外部依赖 ───────────────────────────────────────

fn check_json(content: &str) -> PrecheckVerdict {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_) => PrecheckVerdict::Ok,
        Err(e) => PrecheckVerdict::Failed(PrecheckFailure {
            code: "JSON_SYNTAX_ERROR",
            line: Some(e.line()),
            message: e.to_string(),
        }),
    }
}

fn check_toml(content: &str) -> PrecheckVerdict {
    match toml::from_str::<toml::Value>(content) {
        Ok(_) => PrecheckVerdict::Ok,
        Err(e) => PrecheckVerdict::Failed(PrecheckFailure {
            code: "TOML_SYNTAX_ERROR",
            line: None,
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_file_skips() {
        assert!(matches!(precheck_write("a.rs", "fn main(){}"), PrecheckVerdict::Ok));
    }

    #[test]
    fn json_valid_and_invalid() {
        assert!(matches!(precheck_write("x.json", r#"{"a":1}"#), PrecheckVerdict::Ok));
        let f = match precheck_write("x.json", "{not json") {
            PrecheckVerdict::Failed(f) => f,
            other => panic!("expected failed, got {other:?}"),
        };
        assert_eq!(f.code, "JSON_SYNTAX_ERROR");
        assert_eq!(f.line, Some(1));
    }

    #[test]
    fn toml_valid_and_invalid() {
        let v = precheck_write("Cargo.toml", "[package]\nname = \"a\"\n");
        assert!(matches!(v, PrecheckVerdict::Ok), "valid toml rejected: {v:?}");
        assert!(matches!(
            precheck_write("c.toml", "name = = = bad"),
            PrecheckVerdict::Failed(_)
        ));
    }

    #[test]
    fn python_bad_indent_rejected() {
        // 无 python3 时跳过（Skipped）；有则必须报错。
        match precheck_write("app.py", "def f():\nx = 1\n") {
            PrecheckVerdict::Failed(f) => {
                assert_eq!(f.code, "PY_SYNTAX_ERROR");
                assert_eq!(f.line, Some(2));
            }
            PrecheckVerdict::Skipped(_) => {}
            other => panic!("expected failed or skipped, got {other:?}"),
        }
    }

    #[test]
    fn python_valid_ok_or_skipped() {
        match precheck_write("app.py", "def f():\n    return 1\n") {
            PrecheckVerdict::Ok => {}
            PrecheckVerdict::Skipped(_) => {}
            other => panic!("expected ok or skipped, got {other:?}"),
        }
    }

    #[test]
    fn skip_precheck_bypasses() {
        assert!(precheck_before_write("x.json", "{bad", true).is_ok());
        assert!(precheck_before_write("x.json", "{bad", false).is_err());
        assert!(precheck_before_write("x.py", "def f():\nx=1", true).is_ok());
    }

    #[test]
    fn error_message_has_code_and_line() {
        let msg = precheck_before_write("app.py", "def f():\nx = 1\n", false).unwrap_err();
        assert!(msg.starts_with("校验失败（PY_SYNTAX_ERROR）：app.py:2"), "{msg}");
    }
}
