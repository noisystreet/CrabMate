#![cfg(feature = "server")]
//! CLI 契约：`help` argv 归一化、[`crabmate::parse_args_from_argv`]、[`crabmate::classify_model_error_message`] 与 [`crabmate::CliExitError`] 退出码（与 `main` 一致）。
//!
//! Fixture 位于 `tests/fixtures/cli/`；增删子命令或改映射时请同步更新 JSON。

use crabmate::config::cli::definitions::{
    Commands, McpSubCmd, ParsedCliArgs, ServeCmd, ToolReplaySubCmd, WebBearerSubCmd,
};
use crabmate::{
    CliExitError, EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT,
    EXIT_TOOL_REPLAY_MISMATCH, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE, SaveSessionFormat,
    classify_model_error_message, normalize_help_argv, parse_args_from_argv,
};
use std::sync::Mutex;

/// 解析层**不再读** `CM_HTTP_HOST`（该 env 只在 `cm_cli::serve` 真正起服务时读取），
/// 所以这里不需要清理环境变量；保留串行锁，让断言失败时的输出不互相交错。
static CLI_CONTRACT_LOCK: Mutex<()> = Mutex::new(());

fn with_serialized_cli_contract<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _g = CLI_CONTRACT_LOCK
        .lock()
        .expect("cli_contract tests must run serialized");
    f()
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli")
}

fn expect_serve(p: &ParsedCliArgs) -> &ServeCmd {
    match &p.command {
        Commands::Serve(serve) => serve,
        other => panic!("expected serve subcommand, got {other:?}"),
    }
}

/// fixture 沿用旧的 `extra_cli` 字段名（不改 fixture 数据），这里把它翻译成子命令断言。
///
/// S5（#1295）后 `ParsedCliArgs` 不再有 `extra_cli` 槽，断言改为直接看 `p.command`。
fn assert_extra_cli(name: &str, case: &serde_json::Value, p: &ParsedCliArgs) {
    let want = case["extra_cli"].as_str().expect("extra_cli");
    let ok = match want {
        "None" => !matches!(
            &p.command,
            Commands::Doctor | Commands::Models | Commands::Probe | Commands::Mcp(_)
        ),
        "Doctor" => matches!(&p.command, Commands::Doctor),
        "Models" => matches!(&p.command, Commands::Models),
        "Probe" => matches!(&p.command, Commands::Probe),
        "McpList" => {
            let probe = case["mcp_probe"].as_bool().unwrap_or(false);
            matches!(&p.command, Commands::Mcp(m) if matches!(&m.sub, McpSubCmd::List(l) if l.probe == probe))
        }
        other => panic!("unknown extra_cli in fixture: {other}"),
    };
    assert!(ok, "{name}: extra_cli want {want}, got {:?}", p.command);
}

fn parse_save_session_format(s: &str) -> SaveSessionFormat {
    match s {
        "Json" => SaveSessionFormat::Json,
        "Markdown" => SaveSessionFormat::Markdown,
        "Both" => SaveSessionFormat::Both,
        other => panic!("unknown save_session_format: {other}"),
    }
}

#[test]
fn fixture_help_normalize_matches_normalize_help_argv() {
    let path = fixture_dir().join("help_normalize.json");
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
        let got = normalize_help_argv(argv);
        assert_eq!(got, want, "case {name}");
    }
}

#[test]
fn removed_flat_flags_are_rejected() {
    with_serialized_cli_contract(|| {
        for argv in [
            vec!["crabmate", "--serve"],
            vec!["crabmate", "--dry-run"],
            vec!["crabmate", "--benchmark", "generic"],
        ] {
            let err = parse_args_from_argv(argv.iter().map(|s| (*s).to_string()).collect())
                .expect_err("legacy flat flag must no longer map to a subcommand");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }
    });
}

#[test]
fn config_subcommand_rejects_options() {
    with_serialized_cli_contract(|| {
        // `config` 已无 `--dry-run` 选项（改前为 no-op 旗标）：须由 clap 报未知参数。
        let err = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "config".to_string(),
            "--dry-run".to_string(),
        ])
        .expect_err("config takes no options; --dry-run must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    });
}

#[test]
fn serve_workspace_unset_flag_is_parsed() {
    with_serialized_cli_contract(|| {
        let p = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "serve".to_string(),
            "--workspace-unset".to_string(),
        ])
        .expect("--workspace-unset 应被接受");
        assert!(expect_serve(&p).workspace_unset, "标志须落到 ServeCmd");
        assert_eq!(
            p.global.workspace, None,
            "待绑定模式下不应同时给 --workspace"
        );
    });
}

#[test]
fn serve_workspace_unset_defaults_off() {
    with_serialized_cli_contract(|| {
        let p = parse_args_from_argv(vec!["crabmate".to_string(), "serve".to_string()])
            .expect("serve 无其它参数应可解析");
        assert!(
            !expect_serve(&p).workspace_unset,
            "默认不是待绑定模式（CLI 直启仍用配置的默认目录）"
        );
    });
}

#[test]
fn serve_workspace_unset_conflicts_with_workspace() {
    with_serialized_cli_contract(|| {
        let err = parse_args_from_argv(vec![
            "crabmate".to_string(),
            "--workspace".to_string(),
            "/tmp".to_string(),
            "serve".to_string(),
            "--workspace-unset".to_string(),
        ])
        .expect_err("--workspace 与 --workspace-unset 互斥，须由 clap 拒绝");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    });
}

#[test]
fn fixture_parse_args_from_argv_contract() {
    with_serialized_cli_contract(|| {
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
            let p = parse_args_from_argv(argv).unwrap();

            assert_extra_cli(name, case, &p);
            assert_eq!(
                matches!(&p.command, Commands::Config),
                case["dry_run"].as_bool().unwrap(),
                "{name} dry_run"
            );

            if let Some(want_port) = case["serve_port"].as_u64().map(|u| u as u16) {
                assert_eq!(expect_serve(&p).bind_port(), want_port, "{name} serve_port");
            }

            if let Some(ws) = case.get("workspace").and_then(|v| v.as_str()) {
                assert_eq!(p.global.workspace.as_deref(), Some(ws), "{name} workspace");
            }

            if let Some(ef) = case.get("save_session_format").and_then(|v| v.as_str()) {
                let Commands::SaveSession(cmd) = &p.command else {
                    panic!("{name}: expected save_session");
                };
                assert_eq!(
                    cmd.format,
                    parse_save_session_format(ef),
                    "{name} save_session_format"
                );
            }

            if let Some(wb) = case.get("web_bearer").and_then(|v| v.as_str()) {
                let Commands::WebBearer(cmd) = &p.command else {
                    panic!("{name}: expected web_bearer");
                };
                match wb {
                    "Status" => assert!(
                        matches!(&cmd.sub, WebBearerSubCmd::Status),
                        "{name} web_bearer Status"
                    ),
                    "Clear" => assert!(
                        matches!(&cmd.sub, WebBearerSubCmd::Clear),
                        "{name} web_bearer Clear"
                    ),
                    other => panic!("{name}: unknown web_bearer fixture {other}"),
                }
            }

            if let Some(tr) = case.get("tool_replay") {
                let tr = tr.as_object().expect("tool_replay object");
                let kind = tr["kind"].as_str().expect("tool_replay.kind");
                let Commands::ToolReplay(cmd) = &p.command else {
                    panic!("{name}: expected tool_replay");
                };
                match kind {
                    "Export" => {
                        let ToolReplaySubCmd::Export(e) = &cmd.sub else {
                            panic!("{name}: expected ToolReplaySubCmd::Export");
                        };
                        assert_eq!(
                            e.session_file.as_deref(),
                            tr.get("session_file").and_then(|v| v.as_str()),
                            "{name} tool_replay.session_file"
                        );
                        assert_eq!(
                            e.output.as_deref(),
                            tr.get("output").and_then(|v| v.as_str()),
                            "{name} tool_replay.output"
                        );
                        assert_eq!(
                            e.note.as_deref(),
                            tr.get("note").and_then(|v| v.as_str()),
                            "{name} tool_replay.note"
                        );
                    }
                    "Run" => {
                        let ToolReplaySubCmd::Run(r) = &cmd.sub else {
                            panic!("{name}: expected ToolReplaySubCmd::Run");
                        };
                        assert_eq!(
                            r.fixture.as_str(),
                            tr["fixture"].as_str().expect("tool_replay.fixture"),
                            "{name} tool_replay.fixture"
                        );
                        assert_eq!(
                            r.compare_recorded,
                            tr["compare_recorded"].as_bool().unwrap_or(false),
                            "{name} tool_replay.compare_recorded"
                        );
                    }
                    other => panic!("unknown tool_replay.kind: {other}"),
                }
            }

            // `http_bind_host` 的默认 127.0.0.1 已下沉到 serve 分支（`CM_HTTP_HOST` 亦不再由
            // 解析层读取）；解析层只需保证 `--host` 未被凭空填值。
            if let Commands::Serve(serve) = &p.command {
                assert_eq!(serve.host, None, "{name} serve --host 未给");
            }
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
    assert_eq!(EXIT_TOOL_REPLAY_MISMATCH, 6);
    let mut v = vec![
        EXIT_GENERAL,
        EXIT_USAGE,
        EXIT_MODEL_ERROR,
        EXIT_TOOLS_ALL_RUN_COMMAND_DENIED,
        EXIT_QUOTA_OR_RATE_LIMIT,
        EXIT_TOOL_REPLAY_MISMATCH,
    ];
    v.sort();
    v.dedup();
    assert_eq!(v.len(), 6);
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
