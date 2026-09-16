//! **`help` 子命令** 的 argv 归一化（与 `parse` 共用）。

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

/// `crabmate help` → 根级 `--help`；`crabmate help <已知子命令>` → 对应子命令 `--help`；其余 argv **原样返回**。
///
/// 历史平铺 flag（`--serve` / `--dry-run` / `--benchmark` 等）**已移除**：未写子命令时不再改写，交给 clap 报缺子命令。
/// 同进程 **`chat|repl|tui`** 亦已移除：不把裸 argv / `--query` 映射或默认插入 `chat`/`repl`。
pub fn normalize_help_argv(args: Vec<String>) -> Vec<String> {
    if args.len() <= 1 {
        return args;
    }
    let prog = args[0].clone();
    let rest = &args[1..];
    if rest.first().is_none_or(|s| s != "help") {
        return args;
    }
    match rest.len() {
        1 => vec![prog, "--help".into()],
        _ if is_known_subcommand(rest[1].as_str()) => vec![prog, rest[1].clone(), "--help".into()],
        _ => vec![prog, "--help".into()],
    }
}

#[cfg(test)]
mod help_argv_tests {
    use super::normalize_help_argv;
    use crate::cm_config::cli::definitions::{Commands, ExtraCliCommand, RootCli};
    use crate::cm_config::cli::parse::parse_args_from_argv;
    use clap::Parser;

    fn norm(args: &[&str]) -> Vec<String> {
        normalize_help_argv(args.iter().map(|s| (*s).to_string()).collect())
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
    fn help_doctor_maps_to_subcommand_help() {
        let v = norm(&["crabmate", "help", "doctor"]);
        assert_eq!(v, vec!["crabmate", "doctor", "--help"]);
    }

    #[test]
    fn help_unknown_second_token_falls_back_to_root_help() {
        let v = norm(&["crabmate", "help", "nope"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn help_not_wrapped() {
        let v = norm(&["crabmate", "--help"]);
        assert_eq!(v, vec!["crabmate", "--help"]);
    }

    #[test]
    fn explicit_subcommand_unchanged() {
        let v = norm(&["crabmate", "serve", "3000"]);
        assert_eq!(v, vec!["crabmate", "serve", "3000"]);
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
    fn legacy_flat_flags_are_not_rewritten() {
        let v = norm(&["crabmate", "--serve", "3000"]);
        assert_eq!(v, vec!["crabmate", "--serve", "3000"]);
        let v = norm(&["crabmate", "--dry-run"]);
        assert_eq!(v, vec!["crabmate", "--dry-run"]);
        let v = norm(&["crabmate", "--benchmark", "generic"]);
        assert_eq!(v, vec!["crabmate", "--benchmark", "generic"]);
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
    fn parse_legacy_flat_flags_now_fail() {
        for argv in [
            vec!["crabmate", "--serve"],
            vec!["crabmate", "--serve", "3000"],
            vec!["crabmate", "--dry-run"],
            vec!["crabmate", "--benchmark", "generic", "--batch", "in.jsonl"],
        ] {
            let err = parse_args_from_argv(argv.iter().map(|s| (*s).to_string()).collect())
                .expect_err("legacy flat flag must be rejected");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }
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
    }

    #[test]
    fn parse_serve_no_web_is_rejected() {
        let err = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "serve".to_string(),
            "--no-web".to_string(),
        ])
        .expect_err("--no-web must be unknown");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn parse_config_default_is_dry_run() {
        let p = parse_args_from_argv(vec!["crabmate".to_string(), "config".to_string()]).unwrap();
        assert!(p.dry_run);
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
}
