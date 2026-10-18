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
//! 参数：{ "paths": ["相对工作区根目录的文件路径", ...] }
//! 会直接对目标文件就地格式化，并返回每个文件的简要结果说明。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::output_util;
use super::python_tools;
use super::tool_param_types::FormatPathsArgs;

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let FormatPathsArgs { paths } = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 format_files 形状不一致: {e}"),
    };
    if paths.is_empty() {
        return "错误：缺少 paths 参数（至少一个文件路径）".to_string();
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    run_paths(&refs, workspace_root)
}

fn run_paths(paths: &[&str], workspace_root: &Path) -> String {
    // 保序去重，避免同一文件重复调用格式化器。
    let mut seen = HashSet::new();
    let paths: Vec<&str> = paths
        .iter()
        .copied()
        .map(str::trim)
        .filter(|p| seen.insert(*p))
        .collect();

    let mut outputs: Vec<Option<String>> = vec![None; paths.len()];
    let mut prettier_targets: Vec<(usize, PathBuf)> = Vec::new();
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for (idx, path) in paths.iter().enumerate() {
        if path.is_empty() {
            failed += 1;
            outputs[idx] = Some("错误：paths 中含有空路径".to_string());
            continue;
        }
        match prepare_one(path, workspace_root) {
            // prettier 逐文件启动 npx 开销大，集中到最后一次批处理。
            Ok((Formatter::Prettier, target)) => prettier_targets.push((idx, target)),
            Ok((formatter, target)) => {
                match run_formatter(formatter, &target, workspace_root, false) {
                    Ok(msg) => {
                        succeeded += 1;
                        outputs[idx] = Some(msg);
                    }
                    Err(e) => {
                        failed += 1;
                        outputs[idx] = Some(format!("{}: {}", path, e));
                    }
                }
            }
            Err(e) => {
                failed += 1;
                outputs[idx] = Some(format!("{}: {}", path, e));
            }
        }
    }

    if !prettier_targets.is_empty() {
        let targets: Vec<&Path> = prettier_targets.iter().map(|(_, t)| t.as_path()).collect();
        match run_prettier_files(&targets, workspace_root, false) {
            Ok(msgs) => {
                succeeded += prettier_targets.len();
                for ((idx, _), msg) in prettier_targets.iter().zip(msgs) {
                    outputs[*idx] = Some(msg);
                }
            }
            Err(e) => {
                failed += prettier_targets.len();
                for (idx, target) in &prettier_targets {
                    outputs[*idx] = Some(format!(
                        "{}: {}",
                        display_in_workspace(workspace_root, target),
                        e
                    ));
                }
            }
        }
    }

    let mut results: Vec<String> = outputs.into_iter().flatten().collect();
    if results.len() == 1 {
        return results.pop().unwrap_or_default();
    }
    results.push(format!(
        "完成：共 {} 个文件，成功 {} 个，失败 {} 个",
        results.len(),
        succeeded,
        failed
    ));
    results.join("\n")
}

/// 校验并解析单个路径、选出格式化器；不执行格式化。
fn prepare_one(path: &str, workspace_root: &Path) -> Result<(Formatter, PathBuf), String> {
    let target = resolve_target(workspace_root, path)?;

    if !target.exists() {
        return Err("错误：指定的文件不存在".to_string());
    }
    if !target.is_file() {
        return Err("错误：只能格式化普通文件，不能对目录执行格式化".to_string());
    }

    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match select_formatter(&ext) {
        Some(f) => Ok((f, target)),
        None => Err(format!("错误：暂不支持扩展名为 .{} 的文件格式化", ext)),
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
    let output = cmd.output().map_err(|e| {
        let b = format!("无法执行 rustfmt：{}（请确认已安装 rustfmt）", e);
        output_util::append_notfound_install_hint(b, &e, "rustfmt")
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
    run_prettier_files(&[target], workspace_root, check_only)?
        .into_iter()
        .next()
        .ok_or_else(|| "prettier 未返回结果".to_string())
}

/// 一次 `npx prettier` 调用处理多个文件，避免逐文件启动 npx。
fn run_prettier_files(
    targets: &[&Path],
    workspace_root: &Path,
    check_only: bool,
) -> Result<Vec<String>, String> {
    // 使用项目内的 prettier（若存在），否则依赖全局 npx
    let base = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {}", e))?;

    let mut cmd = Command::new("npx");
    cmd.arg("prettier");
    if check_only {
        cmd.arg("--check");
    } else {
        cmd.arg("--write");
    }
    for target in targets {
        cmd.arg(target.strip_prefix(&base).unwrap_or(*target));
    }
    cmd.current_dir(workspace_root);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| {
        let b = format!(
            "无法执行 prettier：{}（请确认已在工作区内安装 prettier 或可通过 npx 调用）",
            e
        );
        output_util::append_notfound_install_hint(b, &e, "npx")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = [stderr.trim_end(), stdout.trim_end()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("");
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
    Ok(targets
        .iter()
        .map(|target| {
            format!(
                "已使用 prettier {}：{}",
                if check_only {
                    "检查通过"
                } else {
                    "格式化"
                },
                display_in_workspace(workspace_root, target)
            )
        })
        .collect())
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
        let b = format!(
            "无法执行 clang-format：{}（请确认已安装 LLVM/Clang 的 clang-format，且检查模式需支持 --dry-run --Werror）",
            e
        );
        output_util::append_notfound_install_hint(b, &e, "clang-format")
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
            .map_err(|e| {
                let b = format!("无法执行 gofmt：{}（请确认已安装 Go）", e);
                output_util::append_notfound_install_hint(b, &e, "gofmt")
            })?;
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
            .map_err(|e| {
                let b = format!("无法执行 gofmt：{}（请确认已安装 Go）", e);
                output_util::append_notfound_install_hint(b, &e, "gofmt")
            })?;
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
            let b = format!(
                "无法执行 shfmt：{}（请安装 shfmt: https://github.com/mvdan/sh）",
                e
            );
            output_util::append_notfound_install_hint(b, &e, "shfmt")
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
            .map_err(|e| {
                let b = format!("无法执行 xmllint：{}（请安装 libxml2-utils）", e);
                output_util::append_notfound_install_hint(b, &e, "xmllint")
            })?;
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
            .map_err(|e| {
                let b = format!("无法执行 xmllint：{}（请安装 libxml2-utils）", e);
                output_util::append_notfound_install_hint(b, &e, "xmllint")
            })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_paths_rejected() {
        let out = run(r#"{"paths":[]}"#, Path::new("."));
        assert!(out.starts_with("错误：缺少 paths 参数"), "{out}");
    }

    #[test]
    fn duplicate_paths_are_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let out = run(
            r#"{"paths":["missing.txt","missing.txt","other.txt"]}"#,
            dir.path(),
        );
        assert!(
            out.contains("完成：共 2 个文件，成功 0 个，失败 2 个"),
            "{out}"
        );
        assert_eq!(out.matches("missing.txt:").count(), 1, "{out}");
    }

    #[test]
    fn batch_reports_partial_failure_summary() {
        let dir = tempfile::tempdir().unwrap();
        let out = run(r#"{"paths":["a.rs","b.txt"]}"#, dir.path());
        assert!(out.contains("a.rs: 目标文件路径无法解析"), "{out}");
        assert!(out.contains("b.txt: 目标文件路径无法解析"), "{out}");
        assert!(
            out.contains("完成：共 2 个文件，成功 0 个，失败 2 个"),
            "{out}"
        );
    }

    #[test]
    fn single_path_keeps_single_message() {
        let dir = tempfile::tempdir().unwrap();
        let out = run(r#"{"paths":["missing.rs"]}"#, dir.path());
        assert!(out.starts_with("missing.rs: 目标文件路径无法解析"), "{out}");
        assert!(!out.contains("完成：共"), "{out}");
    }

    #[test]
    fn blank_path_entry_is_reported() {
        let out = run(r#"{"paths":["   "]}"#, Path::new("."));
        assert_eq!(out, "错误：paths 中含有空路径");
    }
}
