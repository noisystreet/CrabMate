//! 源码分析工具：shellcheck / cppcheck / semgrep / hadolint / bandit / lizard。
//!
//! 均在**工作区根**执行外部 CLI，路径参数须为相对路径且不含 `..`；全部为只读分析，不修改文件。

#[path = "source_analysis_tools_bandit.rs"]
mod source_analysis_tools_bandit;
#[path = "source_analysis_tools_lizard.rs"]
mod source_analysis_tools_lizard;
#[path = "source_analysis_tools_semgrep.rs"]
mod source_analysis_tools_semgrep;
#[path = "source_analysis_tools_shellcheck.rs"]
mod source_analysis_tools_shellcheck;

pub use source_analysis_tools_bandit::bandit_scan;
pub use source_analysis_tools_lizard::lizard_complexity;
pub use source_analysis_tools_semgrep::semgrep_scan;
pub use source_analysis_tools_shellcheck::shellcheck_check;

use std::path::Path;
use std::process::{Command, Stdio};

use super::output_util;
use super::tool_param_types::{
    CppcheckAnalyzeArgs, CppcheckPlatform, HadolintCheckArgs, HadolintOutputFormat,
};

const MAX_OUTPUT_LINES: usize = 800;
const MAX_PATHS: usize = 24;

fn is_safe_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.contains("..")
}

fn parse_rel_paths_from_slice(
    paths: &[String],
    key: &str,
    default: &[&str],
    max: usize,
) -> Result<Vec<String>, String> {
    let arr = if paths.is_empty() {
        default.iter().map(|s| (*s).to_string()).collect()
    } else {
        paths
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    };
    if arr.len() > max {
        return Err(format!("错误：{key} 最多 {max} 项"));
    }
    for p in &arr {
        if !is_safe_rel_path(p) {
            return Err(format!(
                "错误：{key} 中含非法相对路径（须非空、非绝对、不含 ..）：{p}"
            ));
        }
    }
    Ok(arr)
}

fn filter_existing(base: &Path, paths: &[String]) -> Vec<String> {
    let ex: Vec<_> = paths
        .iter()
        .filter(|p| base.join(p).exists())
        .cloned()
        .collect();
    if ex.is_empty() {
        vec![".".to_string()]
    } else {
        ex
    }
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
        output_util::CommandSpawnErrorStyle::CannotStartWithPathHint,
    )
}

// ── cppcheck ────────────────────────────────────────────────

pub fn cppcheck_analyze(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CppcheckAnalyzeArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 cppcheck_analyze 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["src"], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("cppcheck");
    cmd.current_dir(&base);

    let enable = args
        .enable
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("all");
    match enable {
        "all" | "style" | "performance" | "portability" | "information" | "warning"
        | "unusedFunction" | "missingInclude" => {
            cmd.arg(format!("--enable={enable}"));
        }
        _ => {
            return format!(
                "错误：enable 须为 all/style/performance/portability/information/warning/unusedFunction/missingInclude，收到 {enable}"
            );
        }
    }

    if let Some(std_val) = args.std.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if std_val.len() > 20
            || std_val
                .chars()
                .any(|c| !c.is_alphanumeric() && c != '+' && c != '-')
        {
            return format!("错误：std 值非法：{std_val}");
        }
        cmd.arg(format!("--std={std_val}"));
    }

    if let Some(platform) = args.platform {
        let s = match platform {
            CppcheckPlatform::Unix32 => "unix32",
            CppcheckPlatform::Unix64 => "unix64",
            CppcheckPlatform::Win32a => "win32A",
            CppcheckPlatform::Win32w => "win32W",
            CppcheckPlatform::Win64 => "win64",
            CppcheckPlatform::Native => "native",
        };
        cmd.arg(format!("--platform={s}"));
    }

    cmd.arg("--quiet");

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "cppcheck")
}

// ── Hadolint ────────────────────────────────────────────────

pub fn hadolint_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: HadolintCheckArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 hadolint_check 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let path_raw = args
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Dockerfile");
    if !is_safe_rel_path(path_raw) {
        return format!("错误：path 须为相对路径且不含 ..：{path_raw}");
    }
    let full = base.join(path_raw);
    if !full.is_file() {
        return format!("错误：文件不存在：{path_raw}");
    }

    let mut cmd = Command::new("hadolint");
    cmd.current_dir(&base);

    if let Some(fmt) = args.format {
        let s = match fmt {
            HadolintOutputFormat::Tty => "tty",
            HadolintOutputFormat::Json => "json",
            HadolintOutputFormat::Checkstyle => "checkstyle",
            HadolintOutputFormat::Codeclimate => "codeclimate",
            HadolintOutputFormat::GitlabCodeclimate => "gitlab_codeclimate",
            HadolintOutputFormat::Gnu => "gnu",
            HadolintOutputFormat::Codacy => "codacy",
            HadolintOutputFormat::Sonarqube => "sonarqube",
            HadolintOutputFormat::Sarif => "sarif",
        };
        cmd.arg("--format").arg(s);
    }

    for rule in &args.ignore {
        let rule = rule.trim();
        if rule.is_empty() || rule.len() > 20 {
            continue;
        }
        cmd.arg("--ignore").arg(rule);
    }

    for reg in &args.trusted_registries {
        let reg = reg.trim();
        if reg.is_empty() || reg.len() > 200 || reg.contains("..") {
            continue;
        }
        cmd.arg("--trusted-registry").arg(reg);
    }

    cmd.arg(path_raw);
    run_and_format(cmd, max_output_len, "hadolint")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn shellcheck_rejects_bad_paths() {
        let out = shellcheck_check(r#"{"paths":["../etc/passwd"]}"#, Path::new("."), 4096);
        assert!(out.contains("非法相对路径"), "{out}");
    }

    #[test]
    fn cppcheck_rejects_bad_enable() {
        let out = cppcheck_analyze(r#"{"enable":"evil_flag"}"#, Path::new("."), 4096);
        assert!(out.contains("错误"), "{out}");
    }

    #[test]
    fn semgrep_rejects_bad_config() {
        let out = semgrep_scan(r#"{"config":"../../etc/passwd"}"#, Path::new("."), 4096);
        assert!(out.contains("非法字符") || out.contains("错误"), "{out}");
    }

    #[test]
    fn hadolint_rejects_absolute_path() {
        let out = hadolint_check(r#"{"path":"/etc/passwd"}"#, Path::new("."), 4096);
        assert!(out.contains("相对路径"), "{out}");
    }

    #[test]
    fn bandit_rejects_bad_skip() {
        let out = bandit_scan(r#"{"skip":"../../../etc"}"#, Path::new("."), 4096);
        assert!(out.contains("非法字符") || out.contains("错误"), "{out}");
    }

    #[test]
    fn lizard_rejects_bad_language() {
        let out = lizard_complexity(r#"{"language":"c;rm -rf /"}"#, Path::new("."), 4096);
        assert!(out.contains("非法"), "{out}");
    }

    #[test]
    fn is_safe_rel_path_works() {
        assert!(is_safe_rel_path("src"));
        assert!(is_safe_rel_path("src/main.rs"));
        assert!(!is_safe_rel_path(""));
        assert!(!is_safe_rel_path("/etc"));
        assert!(!is_safe_rel_path("../foo"));
        assert!(!is_safe_rel_path("foo/../bar"));
    }

    #[test]
    fn shellcheck_invalid_severity() {
        let out = shellcheck_check(r#"{"severity":"evil"}"#, Path::new("."), 4096);
        assert!(
            out.contains("形状不一致") || out.contains("shellcheck"),
            "{out}"
        );
    }

    #[test]
    fn hadolint_missing_file() {
        let out = hadolint_check(
            r#"{"path":"nonexistent_dockerfile_xyz"}"#,
            Path::new("."),
            4096,
        );
        assert!(out.contains("文件不存在"), "{out}");
    }
}
