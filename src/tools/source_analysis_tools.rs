//! 源码分析工具：shellcheck / cppcheck / semgrep / hadolint / bandit / lizard。
//!
//! 均在**工作区根**执行外部 CLI，路径参数须为相对路径且不含 `..`；全部为只读分析，不修改文件。

use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

use super::output_util;
use super::tool_param_types::{
    BanditConfidenceArg, BanditOutputFormat, BanditScanArgs, BanditSeverityArg,
    CppcheckAnalyzeArgs, CppcheckPlatform, HadolintCheckArgs, HadolintOutputFormat,
    LizardComplexityArgs, LizardSortKind, SemgrepScanArgs, ShellcheckCheckArgs,
    ShellcheckOutputFormat, ShellcheckSeverity, ShellcheckShellDialect,
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

// ── ShellCheck ──────────────────────────────────────────────

pub fn shellcheck_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: ShellcheckCheckArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 shellcheck_check 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("shellcheck");
    cmd.arg("--color=never").current_dir(&base);

    if let Some(sev) = args.severity {
        let s = match sev {
            ShellcheckSeverity::Error => "error",
            ShellcheckSeverity::Warning => "warning",
            ShellcheckSeverity::Info => "info",
            ShellcheckSeverity::Style => "style",
        };
        cmd.arg("--severity").arg(s);
    }
    if let Some(sh) = args.shell {
        let s = match sh {
            ShellcheckShellDialect::Sh => "sh",
            ShellcheckShellDialect::Bash => "bash",
            ShellcheckShellDialect::Dash => "dash",
            ShellcheckShellDialect::Ksh => "ksh",
        };
        cmd.arg("--shell").arg(s);
    }
    if let Some(fmt) = args.format {
        let s = match fmt {
            ShellcheckOutputFormat::Tty => "tty",
            ShellcheckOutputFormat::Gcc => "gcc",
            ShellcheckOutputFormat::Json1 => "json1",
            ShellcheckOutputFormat::Checkstyle => "checkstyle",
            ShellcheckOutputFormat::Diff => "diff",
            ShellcheckOutputFormat::Quiet => "quiet",
        };
        cmd.arg("--format").arg(s);
    }

    let mut found_scripts = Vec::new();
    for p in &paths {
        let full = base.join(p);
        if full.is_file() {
            found_scripts.push(p.clone());
        } else if full.is_dir()
            && let Ok(entries) = walkdir_shell_scripts(&full, &base)
        {
            found_scripts.extend(entries);
        }
    }
    if found_scripts.is_empty() {
        return "shellcheck: 在指定路径下未发现 shell 脚本（.sh/.bash/.zsh/.ksh 或含 shebang 的文件）".to_string();
    }
    const MAX_FILES: usize = 200;
    if found_scripts.len() > MAX_FILES {
        found_scripts.truncate(MAX_FILES);
    }
    for f in &found_scripts {
        cmd.arg(f);
    }
    run_and_format(cmd, max_output_len, "shellcheck")
}

fn walkdir_shell_scripts(dir: &Path, base: &Path) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    const SKIP_DIRS: &[&str] = &[
        "target",
        "node_modules",
        ".git",
        "vendor",
        "dist",
        "build",
        "__pycache__",
    ];
    while let Some(cur) = stack.pop() {
        let entries = match std::fs::read_dir(&cur) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name_str.as_ref()) && !name_str.starts_with('.') {
                    stack.push(path);
                }
            } else if is_shell_script(&path)
                && let Ok(rel) = path.strip_prefix(base)
            {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    Ok(out)
}

fn is_shell_script(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && matches!(ext, "sh" | "bash" | "zsh" | "ksh")
    {
        return true;
    }
    if let Ok(f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 64];
        let mut reader = std::io::BufReader::new(f);
        if let Ok(n) = reader.read(&mut buf) {
            let head = String::from_utf8_lossy(&buf[..n]);
            if head.starts_with("#!")
                && (head.contains("/sh")
                    || head.contains("/bash")
                    || head.contains("/zsh")
                    || head.contains("/ksh")
                    || head.contains("env sh")
                    || head.contains("env bash"))
            {
                return true;
            }
        }
    }
    false
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

// ── Semgrep ─────────────────────────────────────────────────

pub fn semgrep_scan(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: SemgrepScanArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 semgrep_scan 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("semgrep");
    cmd.arg("scan").arg("--no-git-ignore").current_dir(&base);

    let config = args
        .config
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    if config.len() > 256 || config.contains("..") || config.contains('\n') {
        return "错误：config 值过长或含非法字符".to_string();
    }
    cmd.arg("--config").arg(config);

    if let Some(sev) = args
        .severity
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        for s in sev.split(',') {
            let s = s.trim().to_uppercase();
            match s.as_str() {
                "ERROR" | "WARNING" | "INFO" => {
                    cmd.arg("--severity").arg(&s);
                }
                _ => {
                    return format!(
                        "错误：severity 须为 ERROR/WARNING/INFO（逗号分隔），收到 {sev}"
                    );
                }
            }
        }
    }

    if let Some(lang) = args
        .lang
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if lang.len() > 40
            || lang
                .chars()
                .any(|c| !c.is_alphanumeric() && c != ',' && c != '+')
        {
            return format!("错误：lang 值非法：{lang}");
        }
        cmd.arg("--lang").arg(lang);
    }

    if args.json {
        cmd.arg("--json");
    }

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "semgrep scan")
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

// ── Bandit ──────────────────────────────────────────────────

pub fn bandit_scan(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: BanditScanArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 bandit_scan 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("bandit");
    cmd.arg("-r").current_dir(&base);

    if let Some(sev) = args.severity {
        match sev {
            BanditSeverityArg::Low => {
                cmd.arg("-ll");
            }
            BanditSeverityArg::Medium => {
                cmd.arg("-ll");
            }
            BanditSeverityArg::High => {
                cmd.arg("-lll");
            }
        }
    }

    if let Some(conf) = args.confidence {
        match conf {
            BanditConfidenceArg::Low => {
                cmd.arg("-i");
            }
            BanditConfidenceArg::Medium => {
                cmd.arg("-ii");
            }
            BanditConfidenceArg::High => {
                cmd.arg("-iii");
            }
        }
    }

    if let Some(skip) = args
        .skip
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if skip.len() > 512 || skip.contains('\n') || skip.contains("..") {
            return "错误：skip 值过长或含非法字符".to_string();
        }
        cmd.arg("--skip").arg(skip);
    }

    if let Some(fmt) = args.format {
        let s = match fmt {
            BanditOutputFormat::Txt => "txt",
            BanditOutputFormat::Json => "json",
            BanditOutputFormat::Csv => "csv",
            BanditOutputFormat::Xml => "xml",
            BanditOutputFormat::Html => "html",
            BanditOutputFormat::Yaml => "yaml",
            BanditOutputFormat::Screen => "screen",
            BanditOutputFormat::Custom => "custom",
        };
        cmd.arg("-f").arg(s);
    }

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "bandit")
}

// ── Lizard ──────────────────────────────────────────────────

fn push_lizard_cli_args(
    cmd: &mut Command,
    args: &LizardComplexityArgs,
    paths: &[String],
) -> Result<(), String> {
    if let Some(threshold) = args.threshold
        && threshold > 0
        && threshold <= 200
    {
        cmd.arg("-C").arg(threshold.to_string());
    }

    if let Some(lang) = args
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if lang.len() > 40 || lang.chars().any(|c| !c.is_alphanumeric() && c != ',') {
            return Err(format!("错误：language 值非法：{lang}"));
        }
        cmd.arg("-l").arg(lang);
    }

    if let Some(sort) = args.sort {
        let s = match sort {
            LizardSortKind::CyclomaticComplexity => "cyclomatic_complexity",
            LizardSortKind::Length => "length",
            LizardSortKind::TokenCount => "token_count",
            LizardSortKind::ParameterCount => "parameter_count",
            LizardSortKind::Nloc => "nloc",
        };
        cmd.arg("--sort").arg(s);
    }

    if args.warnings_only {
        cmd.arg("-w");
    }

    for ex in &args.exclude {
        let ex = ex.trim();
        if ex.is_empty() || ex.len() > 160 || ex.contains("..") {
            continue;
        }
        cmd.arg("-x").arg(format!("*/{ex}/*"));
    }

    for p in paths {
        cmd.arg(p);
    }
    Ok(())
}

pub fn lizard_complexity(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: LizardComplexityArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 lizard_complexity 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("lizard");
    cmd.current_dir(&base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Err(msg) = push_lizard_cli_args(&mut cmd, &args, &paths) {
        return msg;
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut cmd_py = Command::new("python3");
            cmd_py
                .arg("-m")
                .arg("lizard")
                .current_dir(&base)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Err(msg) = push_lizard_cli_args(&mut cmd_py, &args, &paths) {
                return msg;
            }
            match cmd_py.output() {
                Ok(o) => o,
                Err(e2) => {
                    return format!(
                        "lizard: 未找到命令 `lizard`（{e}），且 `python3 -m lizard` 亦失败（{e2}）。\
请安装：`pip install lizard` 或 `pip install --user lizard`，将 `lizard` 所在目录加入 PATH（`pip install --user` 时常见为 ~/.local/bin）；\
验证：`lizard --version` 或 `python3 -m lizard --version`。"
                    );
                }
            }
        }
        Err(e) => {
            return format!("lizard: 无法启动（{e}）。请确认已安装对应 CLI 且在 PATH 中。");
        }
    };
    let code = output.status.code().unwrap_or(-1);
    let body = output_util::merge_process_output(
        &output,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
    );
    output_util::format_exited_command_output(
        "lizard",
        code,
        &body,
        max_output_len,
        MAX_OUTPUT_LINES,
    )
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
