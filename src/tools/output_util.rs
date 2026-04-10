//! 工具输出截断公共函数。
//!
//! 多数工具在执行外部命令后需要对 stdout/stderr 做行数 + 字节数双重截断，
//! 避免超长输出占满上下文窗口。此模块统一实现，消除各工具文件中的重复 helper。
//!
//! 另含 **`Command::output()`** 后合并流、拼 **`title (exit=…):`** 块的共用逻辑（原分散在
//! `jvm_tools` / `go_tools` / `cargo_tools` 等多处）。
//!
//! 子进程 **`ErrorKind::NotFound`** 时，若可知程序名且命中内置表，在错误文末追加**简短安装提示**
//!（不猜测用户发行版，仅给常见包管理器与文档入口）。

use std::io::ErrorKind;
use std::path::Path;
use std::process::Output;

/// 合并 **stdout / stderr** 的策略（不同 CLI 习惯不同）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessOutputMerge {
    /// 先 stdout，非空则换行再接 stderr（Maven、Go、`cargo` 子进程等多数工具）。
    ConcatStdoutStderr,
    /// 优先 stderr，否则 stdout（`tsc`、`eslint`、部分 Python / ast-grep 等）。
    StderrElseStdout,
}

/// 子进程**无法启动**（`spawn`/`output` 返回 `Err`）时的用户可见前缀风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandSpawnErrorStyle {
    /// `title: 无法启动命令（reason）`
    CannotStartCommand,
    /// `title: 无法启动（reason）。请确认已安装对应 CLI 且在 PATH 中。`
    CannotStartWithPathHint,
    /// `title: 执行失败（reason）`（与历史 `git` / `security_tools` 文案一致）
    ExecuteFailed,
}

/// 将子进程输出按策略合并为一段正文；若均为空则 **`(无输出)`**。
#[must_use]
pub(crate) fn merge_process_output(output: &Output, merge: ProcessOutputMerge) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let body = match merge {
        ProcessOutputMerge::ConcatStdoutStderr => {
            let mut body = String::new();
            if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(stderr.trim_end());
            }
            body
        }
        ProcessOutputMerge::StderrElseStdout => {
            if !stderr.trim().is_empty() {
                stderr.trim_end().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim_end().to_string()
            } else {
                String::new()
            }
        }
    };
    if body.trim().is_empty() {
        "(无输出)".to_string()
    } else {
        body
    }
}

/// `title (exit=code):\n` + 对 `body` 做行/字节截断（`body` 已含 **`(无输出)`** 时同样截断）。
#[must_use]
pub(crate) fn format_exited_command_output(
    title: &str,
    exit_code: i32,
    body: &str,
    max_bytes: usize,
    max_lines: usize,
) -> String {
    format!(
        "{} (exit={}):\n{}",
        title,
        exit_code,
        truncate_output_lines(body, max_bytes, max_lines)
    )
}

/// 在 `err` 为 **`NotFound`** 且 `program_for_hint` 命中内置表时，于 `base` 文末追加安装提示；否则原样返回。
#[must_use]
pub(crate) fn append_notfound_install_hint(
    mut base: String,
    err: &std::io::Error,
    program_for_hint: &str,
) -> String {
    if err.kind() == ErrorKind::NotFound
        && let Some(h) = cli_missing_install_hint(program_for_hint)
    {
        base.push_str("\n\n");
        base.push_str(h);
    }
    base
}

/// 与 [`format_spawn_error`] 相同，但若 `err` 为 **`NotFound`** 且 `program_for_hint` 命中内置表，
/// 在文末追加一行安装/验证建议（供模型与用户直接执行下一步）。
#[must_use]
pub(crate) fn format_spawn_error_with_program(
    title: &str,
    err: &std::io::Error,
    style: CommandSpawnErrorStyle,
    program_for_hint: Option<&str>,
) -> String {
    let mut base = match style {
        CommandSpawnErrorStyle::CannotStartCommand => {
            format!("{title}: 无法启动命令（{err}）")
        }
        CommandSpawnErrorStyle::CannotStartWithPathHint => {
            format!("{title}: 无法启动（{err}）。请确认已安装对应 CLI 且在 PATH 中。")
        }
        CommandSpawnErrorStyle::ExecuteFailed => {
            format!("{title}: 执行失败（{err}）")
        }
    };
    if err.kind() == ErrorKind::NotFound
        && let Some(name) = program_for_hint
        && let Some(hint) = cli_missing_install_hint(name)
    {
        base.push_str("\n\n");
        base.push_str(hint);
    }
    base
}

/// 从 `Command` 取出用于提示的可执行文件名（去路径，仅展示名）。
fn spawn_program_display_name(cmd: &std::process::Command) -> Option<String> {
    let p = cmd.get_program();
    let s = p.to_str()?;
    if s.is_empty() {
        return None;
    }
    Path::new(s)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .or_else(|| Some(s.to_string()))
}

/// 已知外部 CLI 的「未找到」安装提示；未收录则 `None`（避免对 `cargo`/`sh` 等泛名误提示）。
pub(crate) fn cli_missing_install_hint(program: &str) -> Option<&'static str> {
    let key = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    if key.eq_ignore_ascii_case("sh") || key.eq_ignore_ascii_case("bash") {
        return None;
    }
    if key.eq_ignore_ascii_case("cargo") {
        return Some(
            "安装提示：安装 Rust 工具链（含 cargo），见 https://rustup.rs/ 。验证：`cargo --version`。",
        );
    }
    if key.eq_ignore_ascii_case("shellcheck") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install shellcheck`；macOS `brew install shellcheck`；文档 https://github.com/koalaman/shellcheck#installing 。验证：`shellcheck --version`。",
        );
    }
    if key.eq_ignore_ascii_case("cppcheck") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install cppcheck`；macOS `brew install cppcheck`；文档 https://cppcheck.sourceforge.io/ 。验证：`cppcheck --version`。",
        );
    }
    if key.eq_ignore_ascii_case("semgrep") {
        return Some(
            "安装提示：`pip install semgrep` 或 `pipx install semgrep`；官方说明见 https://semgrep.dev/docs/getting-started/ 。验证：`semgrep --version`。",
        );
    }
    if key.eq_ignore_ascii_case("hadolint") {
        return Some(
            "安装提示：macOS `brew install hadolint`；或从 https://github.com/hadolint/hadolint/releases 下载二进制并加入 PATH。验证：`hadolint --version`。",
        );
    }
    if key.eq_ignore_ascii_case("bandit") {
        return Some(
            "安装提示：`pip install bandit` 或 `pipx install bandit`。验证：`bandit --version` 或 `python3 -m bandit --version`。",
        );
    }
    if key.eq_ignore_ascii_case("lizard") {
        return Some(
            "安装提示：`pip install lizard`；确保 `lizard` 或 `python3 -m lizard` 在 PATH 中。验证：`lizard --version`。",
        );
    }
    if key.eq_ignore_ascii_case("typos") {
        return Some(
            "安装提示：`cargo install typos-cli` 或从 https://github.com/crate-ci/typos/releases 获取二进制。验证：`typos --version`。",
        );
    }
    if key.eq_ignore_ascii_case("codespell") {
        return Some(
            "安装提示：`pip install codespell` 或发行版包 `codespell`。验证：`codespell --version`。",
        );
    }
    if key.eq_ignore_ascii_case("ast-grep") || key.eq_ignore_ascii_case("sg") {
        return Some(
            "安装提示：`cargo install ast-grep --locked` 或见 https://ast-grep.github.io/guide/quick-start.html 。验证：`ast-grep --version`。",
        );
    }
    if key.eq_ignore_ascii_case("pre-commit") {
        return Some(
            "安装提示：`pip install pre-commit` 或 `pipx install pre-commit`。验证：`pre-commit --version`。",
        );
    }
    if key.eq_ignore_ascii_case("ruff") {
        return Some(
            "安装提示：`pip install ruff` 或 `cargo install ruff`（任选其一）。验证：`ruff --version`。",
        );
    }
    if key.eq_ignore_ascii_case("mypy") {
        return Some(
            "安装提示：`pip install mypy`。验证：`mypy --version` 或 `python3 -m mypy --version`。",
        );
    }
    if key.eq_ignore_ascii_case("uv") {
        return Some(
            "安装提示：见 https://docs.astral.sh/uv/getting-started/installation/ （官方安装脚本或包管理器）。验证：`uv --version`。",
        );
    }
    if key.eq_ignore_ascii_case("npm") || key.eq_ignore_ascii_case("npx") {
        return Some(
            "安装提示：安装 Node.js（含 npm/npx），见 https://nodejs.org/ 或发行版包 `nodejs`。验证：`node --version` 与 `npm --version`。",
        );
    }
    if key.eq_ignore_ascii_case("mvn") || key.eq_ignore_ascii_case("maven") {
        return Some(
            "安装提示：安装 Apache Maven（`mvn`），见 https://maven.apache.org/install.html 。验证：`mvn --version`。",
        );
    }
    if key.eq_ignore_ascii_case("gradle") {
        return Some("安装提示：安装 Gradle 或使用项目自带 `gradlew`。验证：`gradle --version`。");
    }
    if key.eq_ignore_ascii_case("docker") {
        return Some(
            "安装提示：安装 Docker Engine / Docker CLI，见 https://docs.docker.com/get-docker/ 。验证：`docker version`。",
        );
    }
    if key.eq_ignore_ascii_case("podman") {
        return Some(
            "安装提示：安装 Podman（发行版包或 https://podman.io/docs/installation ）。验证：`podman --version`。",
        );
    }
    if key.eq_ignore_ascii_case("go") {
        return Some("安装提示：安装 Go 工具链，见 https://go.dev/dl/ 。验证：`go version`。");
    }
    if key.eq_ignore_ascii_case("gofmt") {
        return Some("安装提示：`gofmt` 随 Go 发行，见 https://go.dev/dl/ 。验证：`gofmt -h`。");
    }
    if key.eq_ignore_ascii_case("golangci-lint") {
        return Some(
            "安装提示：见 https://golangci-lint.run/welcome/install/ （官方安装脚本或包管理器）。验证：`golangci-lint --version`。",
        );
    }
    if key.eq_ignore_ascii_case("python3") || key.eq_ignore_ascii_case("python") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install python3`；macOS 可用 `brew install python` 或 https://www.python.org/downloads/ 。验证：`python3 --version`。",
        );
    }
    if key.eq_ignore_ascii_case("pytest") {
        return Some(
            "安装提示：`pip install pytest` 或 `python3 -m pip install pytest`。验证：`pytest --version` 或 `python3 -m pytest --version`。",
        );
    }
    if key.eq_ignore_ascii_case("git") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install git`；macOS `brew install git`；文档 https://git-scm.com/downloads 。验证：`git --version`。",
        );
    }
    if key.eq_ignore_ascii_case("gh") {
        return Some(
            "安装提示：macOS `brew install gh`；Debian/Ubuntu 见 https://github.com/cli/cli/blob/trunk/docs/install_linux.md 。验证：`gh --version`；认证：`gh auth login`。",
        );
    }
    if key.eq_ignore_ascii_case("bc") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install bc`；macOS `brew install bc`；验证：`bc --version`。",
        );
    }
    if key.eq_ignore_ascii_case("rustfmt") {
        return Some(
            "安装提示：`rustup component add rustfmt`（推荐）或 `cargo install rustfmt`。验证：`rustfmt --version`。",
        );
    }
    if key.eq_ignore_ascii_case("clang-format") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install clang-format`；macOS `brew install clang-format`。验证：`clang-format --version`。",
        );
    }
    if key.eq_ignore_ascii_case("shfmt") {
        return Some(
            "安装提示：macOS `brew install shfmt`；或 `go install mvdan.cc/sh/v3/cmd/shfmt@latest`（需 Go）。文档 https://github.com/mvdan/sh 。验证：`shfmt --version`。",
        );
    }
    if key.eq_ignore_ascii_case("xmllint") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install libxml2-utils`；macOS `brew install libxml2`（通常含 xmllint）。验证：`xmllint --version`。",
        );
    }
    if key.eq_ignore_ascii_case("sqlfluff") {
        return Some(
            "安装提示：`pip install sqlfluff` 或 `pipx install sqlfluff`。验证：`sqlfluff --version`。",
        );
    }
    if key.eq_ignore_ascii_case("pg_format") {
        return Some(
            "安装提示：安装 [pgFormatter](https://github.com/darold/pgFormatter)（发行版包名因系统而异，可搜索 `pgformatter`）。验证：`pg_format --version`。",
        );
    }
    if key.eq_ignore_ascii_case("tsc") {
        return Some(
            "安装提示：在工作区 `npm install -D typescript` 后使用 `npx tsc`，或全局 `npm install -g typescript`。验证：`npx tsc --version`。",
        );
    }
    if key.eq_ignore_ascii_case("ss") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install iproute2`（提供 `ss`）；多数精简容器需手动安装。验证：`ss -V`。",
        );
    }
    if key.eq_ignore_ascii_case("lsof") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install lsof`；macOS 通常已内置。验证：`lsof -v`。",
        );
    }
    if key.eq_ignore_ascii_case("ps") {
        return Some(
            "安装提示：Debian/Ubuntu `sudo apt install procps`（提供 `ps`）。验证：`ps --version`。",
        );
    }
    None
}

/// 执行 **`cmd.output()`**，合并输出并格式化为工具返回字符串；启动失败时按 **`spawn_style`** 生成说明。
#[must_use]
pub(crate) fn run_command_output_formatted(
    mut cmd: std::process::Command,
    title: &str,
    max_bytes: usize,
    max_lines: usize,
    merge: ProcessOutputMerge,
    spawn_style: CommandSpawnErrorStyle,
) -> String {
    let program_hint = spawn_program_display_name(&cmd);
    match cmd.output() {
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            let body = merge_process_output(&output, merge);
            format_exited_command_output(title, code, &body, max_bytes, max_lines)
        }
        Err(e) => format_spawn_error_with_program(title, &e, spawn_style, program_hint.as_deref()),
    }
}

/// UTF-8 安全的字节截断：在 `max_bytes` 以内找到最近的 char boundary 并截取。
pub(crate) fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// 行数 + 字节数双重截断（UTF-8 安全）。
///
/// 先按 `max_lines` 裁行，再按 `max_bytes` 裁字节。若发生截断则追加摘要后缀。
pub(super) fn truncate_output_lines(s: &str, max_bytes: usize, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines && s.len() <= max_bytes {
        return s.to_string();
    }
    let kept_lines = lines.len().min(max_lines);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        truncate_to_char_boundary(&joined, max_bytes)
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}

/// 纯字节截断（UTF-8 安全），适用于不需要按行裁剪的场景（如 diff、结构化数据）。
pub(super) fn truncate_output_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let truncated = truncate_to_char_boundary(s, max_bytes);
    format!(
        "{}\n\n[输出已截断：共 {} 字节，上限 {} 字节]",
        truncated,
        s.len(),
        max_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_when_within_limits() {
        let s = "line1\nline2\nline3";
        assert_eq!(truncate_output_lines(s, 1000, 10), s);
    }

    #[test]
    fn truncate_by_line_count() {
        let s = "a\nb\nc\nd\ne";
        let out = truncate_output_lines(s, 10000, 3);
        assert!(out.starts_with("a\nb\nc\n"));
        assert!(out.contains("保留前 3 行"));
        assert!(out.contains("共 5 行"));
    }

    #[test]
    fn truncate_by_byte_limit() {
        let s = "x".repeat(200);
        let out = truncate_output_lines(&s, 50, 1000);
        assert!(out.contains("输出已截断"));
        assert!(out.len() < 200);
    }

    #[test]
    fn char_boundary_safety() {
        let s = "你好世界测试";
        let out = truncate_to_char_boundary(s, 7);
        assert!(out.len() <= 7);
        assert!(out == "你好");
    }

    #[test]
    fn truncate_bytes_only() {
        let s = "a".repeat(200);
        let out = truncate_output_bytes(&s, 50);
        assert!(out.contains("输出已截断"));
        assert!(out.contains("共 200 字节"));
    }

    #[test]
    fn truncate_bytes_no_op() {
        let s = "short";
        assert_eq!(truncate_output_bytes(s, 1000), s);
    }

    #[test]
    fn merge_concat_stdout_stderr() {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg("printf 'a\\n'; printf 'b' >&2")
            .output()
            .expect("sh");
        let m = merge_process_output(&out, ProcessOutputMerge::ConcatStdoutStderr);
        assert_eq!(m, "a\nb");
    }

    #[test]
    fn merge_stderr_else_stdout_prefers_stderr() {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg("printf 'out'; printf 'err' >&2")
            .output()
            .expect("sh");
        let m = merge_process_output(&out, ProcessOutputMerge::StderrElseStdout);
        assert_eq!(m, "err");
    }

    #[test]
    fn merge_empty_is_placeholder() {
        let out = std::process::Command::new("true").output().expect("true");
        assert_eq!(
            merge_process_output(&out, ProcessOutputMerge::ConcatStdoutStderr),
            "(无输出)"
        );
    }

    #[test]
    fn spawn_notfound_appends_shellcheck_install_hint() {
        let e = std::io::Error::new(ErrorKind::NotFound, "no such file");
        let s = format_spawn_error_with_program(
            "shellcheck",
            &e,
            CommandSpawnErrorStyle::CannotStartWithPathHint,
            Some("shellcheck"),
        );
        assert!(s.contains("PATH"), "{s}");
        assert!(
            s.contains("apt install shellcheck") || s.contains("brew install shellcheck"),
            "{s}"
        );
        assert!(s.contains("shellcheck --version"), "{s}");
    }

    #[test]
    fn spawn_notfound_appends_cargo_install_hint() {
        let e = std::io::Error::new(ErrorKind::NotFound, "no such file");
        let s = format_spawn_error_with_program(
            "cargo test",
            &e,
            CommandSpawnErrorStyle::CannotStartCommand,
            Some("cargo"),
        );
        assert!(s.contains("安装提示"), "{s}");
        assert!(s.contains("rustup"), "{s}");
    }

    #[test]
    fn spawn_other_io_error_no_install_hint() {
        let e = std::io::Error::new(ErrorKind::PermissionDenied, "permission denied");
        let s = format_spawn_error_with_program(
            "semgrep scan",
            &e,
            CommandSpawnErrorStyle::CannotStartWithPathHint,
            Some("semgrep"),
        );
        assert!(!s.contains("安装提示"), "{s}");
    }
}
