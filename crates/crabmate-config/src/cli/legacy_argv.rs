//! 历史平铺 flag → 子命令形式的 argv 归一化（与 `parse` 共用）。

#[inline]
fn is_known_subcommand(s: &str) -> bool {
    matches!(
        s,
        "serve"
            | "bench"
            | "config"
            | "doctor"
            | "web-bearer"
            | "models"
            | "probe"
            | "mcp"
            | "save-session"
            | "export-session"
            | "tool-replay"
            | "sse-replay"
            | "plugin"
            | "workflow"
            | "e2e"
    )
}

fn normalize_help_argv(prog: String, rest: &[String]) -> Option<Vec<String>> {
    if rest.first().is_none_or(|s| s != "help") {
        return None;
    }
    Some(match rest.len() {
        1 => vec![prog, "--help".into()],
        _ if is_known_subcommand(rest[1].as_str()) => {
            vec![prog, rest[1].clone(), "--help".into()]
        }
        _ => vec![prog, "--help".into()],
    })
}

fn rewrite_dry_run_to_config(prog: &str, rest: &[String]) -> Option<Vec<String>> {
    if !rest.iter().any(|a| a == "--dry-run") {
        return None;
    }
    let mut out = vec![prog.to_string(), "config".into()];
    for a in rest {
        if a != "--dry-run" {
            out.push(a.clone());
        }
    }
    out.push("--dry-run".into());
    Some(out)
}

/// `--serve` 及可选端口 → `serve …`；其余 flag 保留在尾部。
fn rewrite_serve_subcommand(prog: String, rest: &[String]) -> Option<Vec<String>> {
    if !rest.iter().any(|a| a == "--serve") {
        return None;
    }
    let mut new_rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--serve" {
            i += 1;
            if i < rest.len() && !rest[i].starts_with('-') {
                i += 1;
            }
            continue;
        }
        new_rest.push(rest[i].clone());
        i += 1;
    }
    let mut out = vec![prog, "serve".into()];
    i = 0;
    while i < rest.len() {
        if rest[i] == "--serve" {
            i += 1;
            if i < rest.len() && !rest[i].starts_with('-') {
                out.push(rest[i].clone());
            }
            break;
        }
        i += 1;
    }
    out.extend(new_rest);
    Some(out)
}

/// `--name` 或 `--name=value`（`value` 非空），不含 `--namefoo` 这类前缀误匹配。
#[inline]
fn flag_or_eq_value(arg: &str, base: &str) -> bool {
    arg == base
        || (arg.len() > base.len()
            && arg.starts_with(base)
            && arg.as_bytes().get(base.len()) == Some(&b'='))
}

/// 同时支持 `flag` 与 `flag=<value>` 的历史平铺项。
const LEGACY_BENCH_FLAG_BASES: &[&str] = &[
    "--benchmark",
    "--batch",
    "--batch-output",
    "--task-timeout",
    "--max-tool-rounds",
    "--bench-system-prompt",
];

fn rest_has_legacy_bench_flag(rest: &[String]) -> bool {
    rest.iter().any(|a| {
        a.as_str() == "--resume"
            || LEGACY_BENCH_FLAG_BASES
                .iter()
                .any(|base| flag_or_eq_value(a, base))
    })
}

/// 若 argv 在 **未写子命令名** 时使用历史平铺 flag（`--serve`、`--benchmark` 等），改写为 `serve` / `bench` / … 形式再交给 clap。
///
/// 已写子命令或 `-h` / `--help` / `-V` / `--version` 时不改写。
///
/// **`help` 子命令**：`crabmate help` → 根级 `--help`；`crabmate help serve` 等 → 对应子命令 `--help`。
///
/// 同进程 **`chat|repl|tui` 已移除**：不再把 `--query` / 裸 flag 映射或默认插入 `chat`/`repl`（交给 clap 报缺子命令）。
pub fn normalize_legacy_argv(args: Vec<String>) -> Vec<String> {
    if args.len() <= 1 {
        return args;
    }
    let prog = args[0].clone();
    let rest = &args[1..];
    if let Some(v) = normalize_help_argv(prog.clone(), rest) {
        return v;
    }
    if rest.iter().any(|a| is_known_subcommand(a.as_str())) {
        return args;
    }
    if rest
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "-V" | "--version"))
    {
        return args;
    }

    if let Some(out) = rewrite_dry_run_to_config(&prog, rest) {
        return out;
    }

    if let Some(out) = rewrite_serve_subcommand(prog.clone(), rest) {
        return out;
    }

    if rest_has_legacy_bench_flag(rest) {
        let mut out = vec![prog, "bench".into()];
        out.extend(rest.iter().cloned());
        return out;
    }

    // 旧 chat/repl 平铺 flag / 裸全局 flag：保持原 argv，由 clap 因缺合法子命令失败。
    args
}

#[cfg(test)]
mod legacy_argv_tests {
    use super::normalize_legacy_argv;
    use crate::cli::definitions::{Commands, ExtraCliCommand, RootCli};
    use crate::cli::parse::parse_args_from_argv;
    use clap::Parser;

    fn norm(args: &[&str]) -> Vec<String> {
        normalize_legacy_argv(args.iter().map(|s| (*s).to_string()).collect())
    }

    #[test]
    fn explicit_subcommand_unchanged() {
        let v = norm(&["crabmate", "serve", "3000"]);
        assert_eq!(v, vec!["crabmate", "serve", "3000"]);
    }

    #[test]
    fn help_save_session_routes_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "save-session"]);
        assert_eq!(v, vec!["crabmate", "save-session", "--help"]);
    }

    #[test]
    fn help_export_session_alias_routes_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "export-session"]);
        assert_eq!(v, vec!["crabmate", "export-session", "--help"]);
    }

    #[test]
    fn explicit_doctor_subcommand_not_prefixed_with_repl() {
        let v = norm(&["crabmate", "doctor"]);
        assert_eq!(v, vec!["crabmate", "doctor"]);
    }

    #[test]
    fn explicit_web_bearer_subcommand_not_prefixed_with_repl() {
        let v = norm(&["crabmate", "web-bearer", "status"]);
        assert_eq!(v, vec!["crabmate", "web-bearer", "status"]);
    }

    #[test]
    fn explicit_workflow_validate_not_prefixed_with_repl() {
        let v = norm(&[
            "crabmate",
            "workflow",
            "validate",
            "fixtures/workflows/01_serial_after.yaml",
        ]);
        assert_eq!(
            v,
            vec![
                "crabmate",
                "workflow",
                "validate",
                "fixtures/workflows/01_serial_after.yaml",
            ]
        );
    }

    #[test]
    fn try_parse_root_doctor_subcommand() {
        let r = RootCli::try_parse_from(vec!["crabmate".to_string(), "doctor".to_string()]);
        assert!(r.is_ok(), "{:?}", r.as_ref().err());
        assert!(matches!(r.unwrap().command, Commands::Doctor));
    }

    #[test]
    fn parse_args_from_argv_doctor_matches_extra_cli() {
        let p = parse_args_from_argv(vec!["crabmate".to_string(), "doctor".to_string()]).unwrap();
        assert_eq!(p.extra_cli, ExtraCliCommand::Doctor);
    }

    #[test]
    fn parse_serve_default_is_api_only() {
        let p = parse_args_from_argv(vec!["crabmate".to_string(), "serve".to_string()]).unwrap();
        assert_eq!(p.serve_port, Some(8080));
        assert!(!p.with_web);
    }

    #[test]
    fn parse_serve_with_web_enables_ui_mount() {
        let p = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "serve".to_string(),
            "--with-web".to_string(),
        ])
        .unwrap();
        assert!(p.with_web);
    }

    #[test]
    fn parse_serve_no_web_is_compat_noop() {
        let p = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "serve".to_string(),
            "--no-web".to_string(),
        ])
        .unwrap();
        assert!(!p.with_web);
    }

    #[test]
    fn parse_serve_with_web_and_no_web_conflicts() {
        let err = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "serve".to_string(),
            "--with-web".to_string(),
            "--no-web".to_string(),
        ])
        .expect_err("with-web and no-web must conflict");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn parse_config_default_skips_web_static_check() {
        let p = parse_args_from_argv(vec!["crabmate".to_string(), "config".to_string()]).unwrap();
        assert!(p.dry_run);
        assert!(!p.with_web);
    }

    #[test]
    fn legacy_serve_with_port() {
        let v = norm(&["crabmate", "--serve", "3000", "--no-web"]);
        assert_eq!(v, vec!["crabmate", "serve", "3000", "--no-web"]);
    }

    #[test]
    fn legacy_serve_default_port() {
        let v = norm(&["crabmate", "--serve"]);
        assert_eq!(v, vec!["crabmate", "serve"]);
    }

    #[test]
    fn legacy_serve_then_host() {
        let v = norm(&["crabmate", "--serve", "--host", "0.0.0.0"]);
        assert_eq!(v, vec!["crabmate", "serve", "--host", "0.0.0.0"]);
    }

    #[test]
    fn legacy_no_longer_inserts_repl_or_chat() {
        let v = norm(&["crabmate", "--no-stream"]);
        assert_eq!(v, vec!["crabmate", "--no-stream"]);
        let v = norm(&["crabmate", "--query", "hi"]);
        assert_eq!(v, vec!["crabmate", "--query", "hi"]);
        let v = norm(&["crabmate", "--message-file", "cases.jsonl"]);
        assert_eq!(v, vec!["crabmate", "--message-file", "cases.jsonl"]);
    }

    #[test]
    fn parse_removed_chat_subcommand_fails() {
        let err = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "chat".to_string(),
            "--query".to_string(),
            "hi".to_string(),
        ])
        .expect_err("chat entry removed");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn legacy_config_dry_run() {
        let v = norm(&["crabmate", "--dry-run"]);
        assert_eq!(v, vec!["crabmate", "config", "--dry-run"]);
    }

    #[test]
    fn help_not_wrapped() {
        let v = norm(&["crabmate", "--help"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn help_subcommand_maps_to_root_help() {
        let v = norm(&["crabmate", "help"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn help_known_subcommand_maps_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "serve"]);
        assert_eq!(v, vec!["crabmate", "serve", "--help"]);
    }

    #[test]
    fn help_doctor_maps_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "doctor"]);
        assert_eq!(v, vec!["crabmate", "doctor", "--help"]);
    }

    #[test]
    fn help_unknown_second_token_falls_back_to_root_help() {
        let v = norm(&["crabmate", "help", "nope"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }
}
