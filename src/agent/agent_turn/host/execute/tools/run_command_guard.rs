use std::collections::VecDeque;
use std::path::Path;

pub(super) fn parse_run_command_payload(args_json: &str) -> Option<(String, Vec<String>)> {
    crate::cm_tools::tools::parse_run_command_payload_lenient(args_json)
}

pub(super) fn classify_run_command_failure_family_from_invocation(
    command: &str,
    args: &[String],
) -> Option<&'static str> {
    // `run_command` 不经 shell：`cd` 仅支持 `[相对目录, "&&", 命令, …]`（可串联）。
    // 其余形态在剥离阶段即报 `CdPrefixInvalid`（`error_code = "cd_prefix_invalid"`），
    // 此处必须使用同一家族键，否则执行前后标记无法互相命中。
    if command == "cd" && !(args.len() >= 3 && args.get(1).is_some_and(|s| s == "&&")) {
        return Some("cd_prefix_invalid");
    }
    if args.iter().any(|a| a.contains("..") || a.starts_with('/')) {
        return Some("path_parent_or_absolute_forbidden");
    }
    None
}

/// 依据调用参数与结构化结果（退出码 / stdout / stderr / error_code）判定失败家族。
///
/// 进程已启动（`exit_code` 非空）：只按输出内容判定；启动前失败：优先信任结构化 `error_code`，
/// 仅在没有 `error_code` 的旧式输出上才回退到参数形态判定。
pub(super) fn classify_run_command_failure_family_from_result(
    args_json: &str,
    parsed: &crate::cm_tools::tool_result::ParsedLegacyOutput,
) -> Option<&'static str> {
    if parsed.exit_code.is_some() {
        let output_has_no_tests = parsed.stdout.contains("No tests were found")
            || parsed.stderr.contains("No tests were found");
        return output_has_no_tests.then_some("ctest_no_tests_found");
    }
    let (command, args) = parse_run_command_payload(args_json)?;
    if let Some(code) = parsed.error_code.as_deref() {
        return match code {
            "cd_prefix_invalid" => Some("cd_prefix_invalid"),
            "unsafe_arg" => Some("path_parent_or_absolute_forbidden"),
            "command_not_found" if command == "cd" => Some("shell_builtin_cd_unavailable"),
            _ => None,
        };
    }
    classify_run_command_failure_family_from_invocation(&command, &args)
}

fn cargo_subcommand_needs_manifest(args: &[String]) -> bool {
    let Some(sub) = args.iter().find(|s| !s.starts_with('-')) else {
        return false;
    };
    matches!(
        sub.as_str(),
        "build" | "run" | "test" | "check" | "clippy" | "fmt"
    )
}

fn find_cargo_toml_candidates(base: &Path, max_depth: usize, max_hits: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut q: VecDeque<(std::path::PathBuf, usize)> = VecDeque::new();
    q.push_back((base.to_path_buf(), 0));
    while let Some((dir, depth)) = q.pop_front() {
        if out.len() >= max_hits {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if path.is_file() && path.file_name().is_some_and(|n| n == "Cargo.toml") {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= max_hits {
                    break;
                }
            } else if path.is_dir() && depth < max_depth {
                q.push_back((path, depth + 1));
            }
        }
    }
    out
}

pub(super) fn run_command_cargo_workdir_preflight_error(
    tool_name: &str,
    tool_args_json: &str,
    effective_working_dir: &Path,
) -> Option<String> {
    if tool_name != "run_command" {
        return None;
    }
    let (command, args) = parse_run_command_payload(tool_args_json)?;
    if command != "cargo" {
        return None;
    }
    if args.iter().any(|a| a == "--manifest-path") {
        return None;
    }
    if !cargo_subcommand_needs_manifest(&args) {
        return None;
    }
    if effective_working_dir.join("Cargo.toml").is_file() {
        return None;
    }

    let candidates = find_cargo_toml_candidates(effective_working_dir, 3, 3);
    let command_preview = format!("cargo {}", args.join(" "));
    if candidates.len() == 1 {
        return Some(format!(
            "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。请改为：`{command_preview} --manifest-path {}`",
            candidates[0]
        ));
    }
    if candidates.len() > 1 {
        return Some(format!(
            "错误：当前目录缺少 Cargo.toml，且发现多个候选（{}）。请显式使用 `--manifest-path <path>` 后重试。",
            candidates.join(", ")
        ));
    }
    Some(
        "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。请先定位项目根目录，或改用 `--manifest-path <path>`。"
            .to_string(),
    )
}

pub(super) fn run_command_ctest_preflight_error(
    tool_name: &str,
    tool_args_json: &str,
) -> Option<String> {
    if tool_name != "run_command" {
        return None;
    }
    let (command, args) = parse_run_command_payload(tool_args_json)?;
    if command != "ctest" {
        return None;
    }
    if args.iter().any(|a| a == "--test-dir") {
        return None;
    }
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "-C"
            && let Some(cfg) = iter.peek()
            && (cfg.as_str() == "build" || cfg.contains('/'))
        {
            return Some(
                "错误：检测到 `ctest -C build` 用法疑似错误；`-C` 是配置名（如 Debug/Release），不是构建目录。请改为 `ctest --test-dir build --output-on-failure`（或先 `cd build` 再运行 `ctest --output-on-failure`）。".to_string(),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        classify_run_command_failure_family_from_invocation,
        classify_run_command_failure_family_from_result,
    };
    use crate::cm_tools::tool_result::ParsedLegacyOutput;

    fn launch_failure(exit_code: Option<i32>) -> ParsedLegacyOutput {
        ParsedLegacyOutput {
            ok: false,
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
            error_code: None,
        }
    }

    fn launch_failure_with_code(code: &str) -> ParsedLegacyOutput {
        let mut parsed = launch_failure(None);
        parsed.error_code = Some(code.to_string());
        parsed
    }

    #[test]
    fn classify_family_from_invocation_forbidden_path_and_cd() {
        let cd_args = vec!["tmp".to_string()];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("cd", cd_args.as_slice()),
            Some("cd_prefix_invalid")
        );

        let cd_incomplete = vec!["tmp".to_string(), "&&".to_string()];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("cd", cd_incomplete.as_slice()),
            Some("cd_prefix_invalid")
        );

        let cd_peel = vec![
            "frontend".to_string(),
            "&&".to_string(),
            "cargo".to_string(),
            "check".to_string(),
        ];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("cd", cd_peel.as_slice()),
            None
        );

        let bad_args = vec![
            "-c".to_string(),
            "cd build && ../configure Linux_Serial".to_string(),
        ];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("sh", bad_args.as_slice()),
            Some("path_parent_or_absolute_forbidden")
        );
    }

    #[test]
    fn classify_family_from_result_launch_failures() {
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"sh","args":["-c","cd build && ../configure"]}"#,
                &launch_failure_with_code("unsafe_arg")
            ),
            Some("path_parent_or_absolute_forbidden")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"cd","args":["tmp"]}"#,
                &launch_failure_with_code("cd_prefix_invalid")
            ),
            Some("cd_prefix_invalid")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"cd","args":["tmp","&&"]}"#,
                &launch_failure_with_code("cd_prefix_invalid")
            ),
            Some("cd_prefix_invalid")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"cd","args":["tmp"]}"#,
                &launch_failure_with_code("command_not_found")
            ),
            Some("shell_builtin_cd_unavailable")
        );
        // 非 `cd` 的启动失败不被参数形态误归类（如 skip_arg_safety 路径）。
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"ls","args":["../x"]}"#,
                &launch_failure_with_code("command_not_found")
            ),
            None
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"cargo","args":["test"]}"#,
                &launch_failure(None)
            ),
            None
        );
    }

    #[test]
    fn classify_family_consistent_between_invocation_and_result() {
        for (args_json, code) in [
            (r#"{"command":"cd","args":["tmp"]}"#, "cd_prefix_invalid"),
            (r#"{"command":"cd","args":["tmp","&&"]}"#, "cd_prefix_invalid"),
            (
                r#"{"command":"sh","args":["-c","cd build && ../configure"]}"#,
                "unsafe_arg",
            ),
        ] {
            let (command, args) = super::parse_run_command_payload(args_json).expect("payload");
            assert_eq!(
                classify_run_command_failure_family_from_invocation(&command, &args),
                classify_run_command_failure_family_from_result(
                    args_json,
                    &launch_failure_with_code(code)
                ),
                "args_json={args_json}"
            );
        }
    }

    #[test]
    fn classify_family_from_result_exited_process() {
        let mut parsed = launch_failure(Some(0));
        parsed.stdout = "No tests were found!!!".to_string();
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"ctest","args":["--test-dir","build"]}"#,
                &parsed
            ),
            Some("ctest_no_tests_found")
        );

        let parsed = launch_failure(Some(1));
        assert_eq!(
            classify_run_command_failure_family_from_result(
                r#"{"command":"sh","args":["-c","cd build && ../configure"]}"#,
                &parsed
            ),
            None
        );
    }

    #[test]
    fn ctest_preflight_rejects_dash_c_build_misuse() {
        let args_json = r#"{"command":"ctest","args":["-C","build","-N"]}"#;
        let got = super::run_command_ctest_preflight_error("run_command", args_json);
        assert!(got.is_some());
    }
}
