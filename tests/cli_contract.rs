//! CLI 契约：legacy argv 映射、[`crabmate::parse_args_from_argv`]、[`crabmate::classify_model_error_message`] 与 [`crabmate::CliExitError`] 退出码（与 `main` 一致）。
//!
//! Fixture 位于 `tests/fixtures/cli/`；增删子命令或改映射时请同步更新 JSON。

use crabmate::{
    CliExitError, EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT,
    EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE, ExportSessionFormat, ExtraCliCommand,
    classify_model_error_message, normalize_legacy_argv, parse_args_from_argv,
};
use std::sync::Mutex;

/// `parse_args` 会读 `AGENT_HTTP_HOST`；契约用例假定默认 `127.0.0.1`，故串行并临时清理。
static CLI_CONTRACT_LOCK: Mutex<()> = Mutex::new(());

fn with_isolated_agent_http_host<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _g = CLI_CONTRACT_LOCK
        .lock()
        .expect("cli_contract tests must run serialized");
    let prev = std::env::var("AGENT_HTTP_HOST").ok();
    // SAFETY: `set_var`/`remove_var` are unsafe in Rust 2024; we hold `CLI_CONTRACT_LOCK` so no
    // concurrent tests in this crate read `AGENT_HTTP_HOST` during `f()`.
    unsafe {
        std::env::remove_var("AGENT_HTTP_HOST");
    }
    let out = f();
    unsafe {
        match prev {
            Some(v) => std::env::set_var("AGENT_HTTP_HOST", v),
            None => std::env::remove_var("AGENT_HTTP_HOST"),
        }
    }
    out
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli")
}

fn parse_extra_cli(s: &str) -> ExtraCliCommand {
    match s {
        "None" => ExtraCliCommand::None,
        "Doctor" => ExtraCliCommand::Doctor,
        "Models" => ExtraCliCommand::Models,
        "Probe" => ExtraCliCommand::Probe,
        other => panic!("unknown extra_cli in fixture: {other}"),
    }
}

fn parse_export_format(s: &str) -> ExportSessionFormat {
    match s {
        "Json" => ExportSessionFormat::Json,
        "Markdown" => ExportSessionFormat::Markdown,
        "Both" => ExportSessionFormat::Both,
        other => panic!("unknown export_format: {other}"),
    }
}

#[test]
fn fixture_legacy_normalize_matches_normalize_legacy_argv() {
    let path = fixture_dir().join("legacy_normalize.json");
    let raw = std::fs::read_to_string(&path).unwrap();
    let cases: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let argv: Vec<String> = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        let want: Vec<String> = case["normalized"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        let got = normalize_legacy_argv(argv);
        assert_eq!(got, want, "case {name}");
    }
}

#[test]
fn fixture_parse_args_from_argv_contract() {
    with_isolated_agent_http_host(|| {
        let path = fixture_dir().join("parse_contract.json");
        let raw = std::fs::read_to_string(&path).unwrap();
        let cases: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for case in cases.as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let argv: Vec<String> = case["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect();
            let stdin_fixture = case["stdin_fixture"].as_str().map(|s| s.to_string());
            let p = parse_args_from_argv(argv, stdin_fixture).unwrap();

            let want_extra = parse_extra_cli(case["extra_cli"].as_str().unwrap());
            assert_eq!(p.extra_cli, want_extra, "{name} extra_cli");
            assert_eq!(
                p.dry_run,
                case["dry_run"].as_bool().unwrap(),
                "{name} dry_run"
            );
            let want_port = case["serve_port"].as_u64().map(|u| u as u16);
            assert_eq!(p.serve_port, want_port, "{name} serve_port");
            assert_eq!(
                p.no_stream,
                case["no_stream"].as_bool().unwrap(),
                "{name} no_stream"
            );
            assert_eq!(
                p.chat_cli.wants_chat(),
                case["chat_wants"].as_bool().unwrap(),
                "{name} chat_wants"
            );

            if let Some(ws) = case.get("workspace").and_then(|v| v.as_str()) {
                assert_eq!(p.workspace_cli.as_deref(), Some(ws), "{name} workspace");
            }

            if let Some(inline) = case.get("chat_inline").and_then(|v| v.as_str()) {
                assert_eq!(
                    p.chat_cli.inline_user_text.as_deref(),
                    Some(inline),
                    "{name} chat_inline"
                );
            }

            if let Some(om) = case.get("chat_output").and_then(|v| v.as_str()) {
                assert_eq!(p.chat_cli.output.as_deref(), Some(om), "{name} chat_output");
            }

            if let Some(ef) = case.get("export_format").and_then(|v| v.as_str()) {
                let es = p
                    .export_session
                    .as_ref()
                    .unwrap_or_else(|| panic!("{name}: expected export_session"));
                assert_eq!(es.format, parse_export_format(ef), "{name} export_format");
            } else {
                assert!(
                    p.export_session.is_none(),
                    "{name}: export_session should be absent"
                );
            }

            assert_eq!(
                p.http_bind_host, "127.0.0.1",
                "{name} http_bind_host default"
            );
        }
    });
}

#[test]
fn fixture_classify_model_error_message_contract() {
    let path = fixture_dir().join("exit_classify.json");
    let raw = std::fs::read_to_string(&path).unwrap();
    let cases: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let msg = case["message"].as_str().unwrap();
        let code = case["code"].as_i64().unwrap() as i32;
        assert_eq!(classify_model_error_message(msg), code, "case {name}");
    }
}

#[test]
fn cli_exit_code_numeric_contract_for_main_mapping() {
    assert_eq!(EXIT_GENERAL, 1);
    assert_eq!(EXIT_USAGE, 2);
    assert_eq!(EXIT_MODEL_ERROR, 3);
    assert_eq!(EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, 4);
    assert_eq!(EXIT_QUOTA_OR_RATE_LIMIT, 5);
    let mut v = vec![
        EXIT_GENERAL,
        EXIT_USAGE,
        EXIT_MODEL_ERROR,
        EXIT_TOOLS_ALL_RUN_COMMAND_DENIED,
        EXIT_QUOTA_OR_RATE_LIMIT,
    ];
    v.sort();
    v.dedup();
    assert_eq!(v.len(), 5);
}

#[test]
fn cli_exit_error_downcast_matches_main() {
    let e: Box<dyn std::error::Error> = Box::new(CliExitError::new(EXIT_USAGE, "bad args"));
    let cli = e
        .downcast_ref::<CliExitError>()
        .expect("main-style downcast");
    assert_eq!(cli.code, EXIT_USAGE);
    assert!(cli.message.contains("bad"));
}
