use std::collections::VecDeque;
use std::path::Path;

pub(super) fn parse_run_command_payload(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_string();
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some((command, args))
}

pub(super) fn classify_run_command_failure_family_from_invocation(
    command: &str,
    args: &[String],
) -> Option<&'static str> {
    // `run_command` 不经 shell：`cd` 不可 exec。支持 `cd <相对> && <cmd…>` 由实现换目录，勿归为「cd 不可用」。
    if command == "cd" && !(args.len() >= 3 && args.get(1).is_some_and(|s| s == "&&")) {
        return Some("shell_builtin_cd_unavailable");
    }
    if args.iter().any(|a| a.contains("..") || a.starts_with('/')) {
        return Some("path_parent_or_absolute_forbidden");
    }
    None
}

pub(super) fn classify_run_command_failure_family_from_result(
    result: &str,
) -> Option<&'static str> {
    if result.contains("参数不允许包含 \"..\" 或绝对路径（以 / 开头）") {
        return Some("path_parent_or_absolute_forbidden");
    }
    if result.contains("`cd` 前缀无效") {
        return Some("cd_prefix_invalid");
    }
    if result.contains("命令 \"cd\" 不存在或在当前环境中不可用") {
        return Some("shell_builtin_cd_unavailable");
    }
    if result.contains("当前目录缺少 Cargo.toml") {
        return Some("cargo_manifest_missing");
    }
    if result.contains("No tests were found") {
        return Some("ctest_no_tests_found");
    }
    None
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

    #[test]
    fn classify_family_from_invocation_forbidden_path_and_cd() {
        let cd_args = vec!["tmp".to_string()];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("cd", cd_args.as_slice()),
            Some("shell_builtin_cd_unavailable")
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
    fn classify_family_from_result_known_failures() {
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）"
            ),
            Some("path_parent_or_absolute_forbidden")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：命令 \"cd\" 不存在或在当前环境中不可用（工作目录：/tmp）"
            ),
            Some("shell_builtin_cd_unavailable")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：`cd` 前缀无效：…（当前工作目录：/tmp）"
            ),
            Some("cd_prefix_invalid")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。"
            ),
            Some("cargo_manifest_missing")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result("No tests were found!!!"),
            Some("ctest_no_tests_found")
        );
    }

    #[test]
    fn ctest_preflight_rejects_dash_c_build_misuse() {
        let args_json = r#"{"command":"ctest","args":["-C","build","-N"]}"#;
        let got = super::run_command_ctest_preflight_error("run_command", args_json);
        assert!(got.is_some());
    }
}
