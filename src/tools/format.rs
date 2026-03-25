//! 工作区内的代码格式化工具。
//!
//! 根据文件扩展名自动选择本地格式化器：
//! - `.rs`   -> `rustfmt`
//! - `.py`   -> `ruff format`
//! - `.c` / `.h` / `.cpp` / `.cc` / `.cxx` / `.hpp` / `.hh` -> `clang-format`
//! - `.ts` / `.tsx` / `.js` / `.jsx` / `.json` -> `npx prettier --write`
//! - `.go`   -> `gofmt`
//! - `.sh` / `.bash` / `.zsh` -> `shfmt`
//! - `.md` / `.yaml` / `.yml` / `.css` / `.scss` / `.less` / `.html` / `.vue` / `.svelte` / `.graphql` -> `npx prettier`
//! - `.xml`  -> `xmllint --format`
//! - `.sql`  -> `sqlfluff fix` / `pg_format`
//!
//! 参数：{ "path": "相对工作区根目录的文件路径" }
//! 会直接对目标文件就地格式化，并返回简要的结果说明。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::python_tools;

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "错误：缺少 path 参数".to_string(),
    };

    let target = match resolve_target(workspace_root, path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if !target.exists() {
        return "错误：指定的文件不存在".to_string();
    }
    if !target.is_file() {
        return "错误：只能格式化普通文件，不能对目录执行格式化".to_string();
    }

    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let formatter = select_formatter(&ext);
    match formatter {
        Some(f) => match run_formatter(f, &target, workspace_root, false) {
            Ok(msg) => msg,
            Err(e) => e,
        },
        None => format!("错误：暂不支持扩展名为 .{} 的文件格式化", ext),
    }
}

/// 对单个文件做格式「检查」（不写入）。
pub fn run_check(args_json: &str, workspace_root: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "错误：缺少 path 参数".to_string(),
    };

    let target = match resolve_target(workspace_root, path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if !target.exists() {
        return "错误：指定的文件不存在".to_string();
    }
    if !target.is_file() {
        return "错误：只能检查普通文件".to_string();
    }

    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let formatter = select_formatter(&ext);
    match formatter {
        Some(f) => match run_formatter(f, &target, workspace_root, true) {
            Ok(msg) => msg,
            Err(e) => e,
        },
        None => format!("错误：暂不支持扩展名为 .{} 的格式检查", ext),
    }
}

#[derive(Copy, Clone)]
enum Formatter {
    Rustfmt,
    Prettier,
    Ruff,
    ClangFormat,
    Gofmt,
    Shfmt,
    XmlLint,
    SqlFormat,
}

fn is_c_cpp_extension(ext: &str) -> bool {
    matches!(ext, "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh")
}

fn select_formatter(ext: &str) -> Option<Formatter> {
    if ext == "rs" {
        Some(Formatter::Rustfmt)
    } else if ext == "py" {
        Some(Formatter::Ruff)
    } else if is_c_cpp_extension(ext) {
        Some(Formatter::ClangFormat)
    } else if ext == "go" {
        Some(Formatter::Gofmt)
    } else if matches!(ext, "sh" | "bash" | "zsh") {
        Some(Formatter::Shfmt)
    } else if ext == "xml" {
        Some(Formatter::XmlLint)
    } else if ext == "sql" {
        Some(Formatter::SqlFormat)
    } else if matches!(
        ext,
        "ts" | "tsx"
            | "js"
            | "jsx"
            | "json"
            | "md"
            | "markdown"
            | "yaml"
            | "yml"
            | "css"
            | "scss"
            | "less"
            | "html"
            | "htm"
            | "vue"
            | "svelte"
            | "graphql"
    ) {
        Some(Formatter::Prettier)
    } else {
        None
    }
}

/// 工具返回说明中的路径：相对工作区根（POSIX），不输出绝对路径。
fn display_in_workspace(workspace_root: &Path, target: &Path) -> String {
    let Ok(base) = workspace_root.canonicalize() else {
        return target.display().to_string();
    };
    match target.strip_prefix(&base) {
        Ok(rel) => {
            let s = rel.to_string_lossy().replace('\\', "/");
            if s.is_empty() { ".".to_string() } else { s }
        }
        Err(_) => target.display().to_string(),
    }
}

fn resolve_target(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub_path = Path::new(sub);
    if sub_path.is_absolute() {
        return Err("路径必须是相对于工作区根目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
    let joined = base_canonical.join(sub_path);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("目标文件路径无法解析: {}", e))?;
    if !canonical.starts_with(&base_canonical) {
        return Err("目标文件路径不能超出工作区根目录".to_string());
    }
    Ok(canonical)
}

fn run_formatter(
    formatter: Formatter,
    target: &Path,
    workspace_root: &Path,
    check_only: bool,
) -> Result<String, String> {
    match formatter {
        Formatter::Rustfmt => run_rustfmt(target, workspace_root, check_only),
        Formatter::Prettier => run_prettier(target, workspace_root, check_only),
        Formatter::Ruff => python_tools::ruff_format_file(target, workspace_root, check_only),
        Formatter::ClangFormat => run_clang_format(target, workspace_root, check_only),
        Formatter::Gofmt => run_gofmt(target, workspace_root, check_only),
        Formatter::Shfmt => run_shfmt(target, workspace_root, check_only),
        Formatter::XmlLint => run_xmllint(target, workspace_root, check_only),
        Formatter::SqlFormat => run_sql_format(target, workspace_root, check_only),
    }
}

fn run_rustfmt(target: &Path, workspace_root: &Path, check_only: bool) -> Result<String, String> {
    let mut cmd = Command::new("rustfmt");
    if check_only {
        cmd.arg("--check");
    } else {
        cmd.arg("--emit").arg("files");
    }
    cmd.arg(target);
    // TUI 全屏下若继承 stdout/stderr，子进程输出会直接画到终端（常落在输入框区域），必须捕获。
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|e| format!("无法执行 rustfmt：{}（请确认已安装 rustfmt）", e))?;
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
        return Err(format!(
            "rustfmt {}失败，退出码：{}{}",
            if check_only { "检查" } else { "格式化" },
            output.status.code().unwrap_or(-1),
            suffix
        ));
    }
    Ok(format!(
        "已使用 rustfmt {}：{}",
        if check_only {
            "检查通过"
        } else {
            "格式化"
        },
        display_in_workspace(workspace_root, target)
    ))
}

fn run_prettier(target: &Path, workspace_root: &Path, check_only: bool) -> Result<String, String> {
    // 使用项目内的 prettier（若存在），否则依赖全局 npx
    let relative = target
        .strip_prefix(
            workspace_root
                .canonicalize()
                .map_err(|e| format!("工作区根目录无法解析: {}", e))?,
        )
        .unwrap_or(target);

    let mut cmd = Command::new("npx");
    cmd.arg("prettier");
    if check_only {
        cmd.arg("--check");
    } else {
        cmd.arg("--write");
    }
    cmd.arg(relative).current_dir(workspace_root);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| {
        format!(
            "无法执行 prettier：{}（请确认已在工作区内安装 prettier 或可通过 npx 调用）",
            e
        )
    })?;
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
        return Err(format!(
            "prettier {}失败，退出码：{}{}",
            if check_only { "检查" } else { "格式化" },
            output.status.code().unwrap_or(-1),
            suffix
        ));
    }
    Ok(format!(
        "已使用 prettier {}：{}",
        if check_only {
            "检查通过"
        } else {
            "格式化"
        },
        display_in_workspace(workspace_root, target)
    ))
}

fn run_clang_format(
    target: &Path,
    workspace_root: &Path,
    check_only: bool,
) -> Result<String, String> {
    let mut cmd = Command::new("clang-format");
    if check_only {
        cmd.args(["--dry-run", "--Werror"]);
    } else {
        cmd.arg("-i");
    }
    cmd.arg(target);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| {
        format!(
            "无法执行 clang-format：{}（请确认已安装 LLVM/Clang 的 clang-format，且检查模式需支持 --dry-run --Werror）",
            e
        )
    })?;
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
        return Err(format!(
            "clang-format {}失败，退出码：{}{}",
            if check_only { "检查" } else { "格式化" },
            output.status.code().unwrap_or(-1),
            suffix
        ));
    }
    Ok(format!(
        "已使用 clang-format {}：{}",
        if check_only {
            "检查通过"
        } else {
            "格式化"
        },
        display_in_workspace(workspace_root, target)
    ))
}

fn run_gofmt(target: &Path, workspace_root: &Path, check_only: bool) -> Result<String, String> {
    if check_only {
        let output = Command::new("gofmt")
            .arg("-l")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("无法执行 gofmt：{}（请确认已安装 Go）", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            Ok(format!(
                "gofmt 检查通过：{}",
                display_in_workspace(workspace_root, target)
            ))
        } else {
            Err(format!(
                "gofmt 检查失败（文件需格式化）：{}",
                display_in_workspace(workspace_root, target)
            ))
        }
    } else {
        let output = Command::new("gofmt")
            .arg("-w")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("无法执行 gofmt：{}（请确认已安装 Go）", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gofmt 格式化失败：{}", stderr.trim_end()));
        }
        Ok(format!(
            "已使用 gofmt 格式化：{}",
            display_in_workspace(workspace_root, target)
        ))
    }
}

fn run_shfmt(target: &Path, workspace_root: &Path, check_only: bool) -> Result<String, String> {
    let output = Command::new("shfmt")
        .arg(if check_only { "-d" } else { "-w" })
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            format!(
                "无法执行 shfmt：{}（请安装 shfmt: https://github.com/mvdan/sh）",
                e
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stdout.trim().is_empty() {
            stdout.trim_end()
        } else {
            stderr.trim_end()
        };
        return Err(format!(
            "shfmt {}失败，退出码：{}\n{}",
            if check_only { "检查" } else { "格式化" },
            output.status.code().unwrap_or(-1),
            detail
        ));
    }
    Ok(format!(
        "已使用 shfmt {}：{}",
        if check_only {
            "检查通过"
        } else {
            "格式化"
        },
        display_in_workspace(workspace_root, target)
    ))
}

fn run_xmllint(target: &Path, workspace_root: &Path, check_only: bool) -> Result<String, String> {
    if check_only {
        let output = Command::new("xmllint")
            .arg("--noout")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("无法执行 xmllint：{}（请安装 libxml2-utils）", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("xmllint 检查失败：{}", stderr.trim_end()));
        }
        Ok(format!(
            "xmllint 检查通过：{}",
            display_in_workspace(workspace_root, target)
        ))
    } else {
        let output = Command::new("xmllint")
            .arg("--format")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("无法执行 xmllint：{}（请安装 libxml2-utils）", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("xmllint 格式化失败：{}", stderr.trim_end()));
        }
        let formatted = String::from_utf8_lossy(&output.stdout);
        std::fs::write(target, formatted.as_bytes()).map_err(|e| format!("写回文件失败：{}", e))?;
        Ok(format!(
            "已使用 xmllint 格式化：{}",
            display_in_workspace(workspace_root, target)
        ))
    }
}

fn run_sql_format(
    target: &Path,
    workspace_root: &Path,
    check_only: bool,
) -> Result<String, String> {
    if let Ok(output) = Command::new("sqlfluff")
        .arg(if check_only { "lint" } else { "fix" })
        .arg("--dialect")
        .arg("ansi")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if !stdout.trim().is_empty() {
            stdout.trim_end().to_string()
        } else {
            stderr.trim_end().to_string()
        };
        return Ok(format!(
            "sqlfluff {}：{}\n{}",
            if check_only {
                "检查完成"
            } else {
                "格式化完成"
            },
            display_in_workspace(workspace_root, target),
            detail
        ));
    }

    if let Ok(output) = Command::new("pg_format")
        .arg(if check_only {
            "--no-space"
        } else {
            "--inplace"
        })
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        && output.status.success()
    {
        return Ok(format!(
            "已使用 pg_format {}：{}",
            if check_only { "检查" } else { "格式化" },
            display_in_workspace(workspace_root, target)
        ));
    }

    Err("SQL 格式化需要安装 sqlfluff 或 pg_format".to_string())
}
