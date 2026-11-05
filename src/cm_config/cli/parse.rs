//! `parse_args` / `parse_args_from_argv` 与 `RootCli` → [`ParsedCliArgs`] 映射。
//!
//! 这里**只做组装**：把 clap 的全局选项清洗成 [`CliGlobals`]，子命令**原样**留在
//! [`ParsedCliArgs::command`]。子命令载荷不再拆成平铺字段（S5，见 #1295）——派生量
//! （`serve` 的端口默认值、`CM_HTTP_HOST` 回退）由各子命令分支自己算。

use super::definitions::{CliGlobals, Commands, GlobalOpts, ParsedCliArgs, RootCli};
use super::help_argv::normalize_help_argv;
use clap::Parser;
use std::io;

/// 全局选项清洗：`--log` 去空白、`--llm-context-tokens` 的 `0` 折叠为 `None`。
///
/// `0` 表示「不覆盖」（见 [`GlobalOpts::llm_context_tokens`] 的说明），
/// 若原样透传会被下游当成「上下文窗口 0 token」。
fn globals_from(opts: &GlobalOpts) -> CliGlobals {
    let log_file = opts
        .log
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    CliGlobals {
        config_path: opts.config.clone(),
        workspace: opts.workspace.clone(),
        no_tools: opts.no_tools,
        log_file,
        llm_context_tokens: opts.llm_context_tokens.filter(|&n| n > 0),
    }
}

/// 解析命令行：须显式子命令（**`serve` / `bench` / `config` / `doctor` / …**）；同进程 **`chat|repl|tui` 入口已移除**。
///
/// 非法 CLI：打印 clap 说明后以 **非零** 码退出进程（与历史 `parse_from` 行为一致）；**不会**向调用方返回 `Err`。
pub fn parse_args() -> io::Result<ParsedCliArgs> {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_help_argv(raw);
    let root = RootCli::try_parse_from(normalized).unwrap_or_else(|e| e.exit());
    let parsed = build_parsed_cli_args(root);
    validate_parsed(&parsed)?;
    Ok(parsed)
}

/// 使用给定 **`argv`**（首元素为程序名）解析 CLI，供契约/集成测试；生产请用 [`parse_args`]。
///
/// 非法参数：返回 [`io::Error`]（**不**退出进程），便于断言。
pub fn parse_args_from_argv(raw: Vec<String>) -> io::Result<ParsedCliArgs> {
    let normalized = normalize_help_argv(raw);
    let root = RootCli::try_parse_from(normalized)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let parsed = build_parsed_cli_args(root);
    validate_parsed(&parsed)?;
    Ok(parsed)
}

/// 解析后的跨字段校验。
///
/// clap 的 `conflicts_with` 对 **global** 参数（`--workspace`）在子命令上下文里不生效
/// （实测 `--workspace /tmp serve --workspace-unset` 仍被接受），故在此兜底：
/// 待绑定模式与显式工作区同时出现属于用户误解，必须拒绝而不是「后者赢」。
fn validate_parsed(p: &ParsedCliArgs) -> io::Result<()> {
    if let Commands::Serve(serve) = &p.command
        && serve.workspace_unset
        && p.global.workspace.is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--workspace` 与 `--workspace-unset` 互斥：待绑定模式下工作区由 client 通过 `POST /workspace` 设置",
        ));
    }
    Ok(())
}

fn build_parsed_cli_args(root: RootCli) -> ParsedCliArgs {
    ParsedCliArgs {
        global: globals_from(&root.global),
        command: root.command,
    }
}

#[cfg(test)]
mod parse_tests {
    use super::parse_args_from_argv;
    use crate::cm_config::cli::definitions::{Commands, McpSubCmd, WorkflowSubCmd};

    fn parse(argv: &[&str]) -> crate::cm_config::cli::definitions::ParsedCliArgs {
        parse_args_from_argv(argv.iter().map(|s| (*s).to_string()).collect())
            .expect("argv should parse")
    }

    /// 子命令载荷必须**原样**到达：这里钉住「不再摊平」这件事本身。
    #[test]
    fn command_payload_is_preserved() {
        let p = parse(&["crabmate", "workflow", "run", "a.yaml"]);
        let Commands::Workflow(w) = p.command else {
            panic!("expected workflow, got {:?}", p.command);
        };
        let WorkflowSubCmd::Run(r) = w.sub else {
            panic!("expected workflow run");
        };
        assert_eq!(r.file, "a.yaml");
    }

    #[test]
    fn mcp_serve_keeps_port() {
        let p = parse(&["crabmate", "mcp", "serve", "--port", "9999"]);
        let Commands::Mcp(m) = p.command else {
            panic!("expected mcp");
        };
        let McpSubCmd::Serve(s) = m.sub else {
            panic!("expected mcp serve");
        };
        assert_eq!(s.port, 9999);
    }

    #[test]
    fn globals_are_cleaned() {
        let p = parse(&["crabmate", "--log", "  ", "--workspace", "/tmp", "doctor"]);
        assert_eq!(p.global.log_file, None, "空白 --log 应折成 None");
        assert_eq!(p.global.workspace.as_deref(), Some("/tmp"));
    }

    #[test]
    fn zero_context_tokens_folds_to_none() {
        let p = parse(&["crabmate", "--llm-context-tokens", "0", "doctor"]);
        assert_eq!(p.global.llm_context_tokens, None);
        let p = parse(&["crabmate", "--llm-context-tokens", "1234", "doctor"]);
        assert_eq!(p.global.llm_context_tokens, Some(1234));
    }

    /// `--workspace` 与 `serve --workspace-unset` 互斥（clap 的 global 冲突失效，见 `validate_parsed`）。
    #[test]
    fn workspace_unset_conflicts_with_workspace() {
        let err = parse_args_from_argv(
            ["crabmate", "--workspace", "/tmp", "serve", "--workspace-unset"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        )
        .expect_err("must reject --workspace + --workspace-unset");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
