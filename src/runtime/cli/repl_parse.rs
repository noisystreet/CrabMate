//! REPL 行内 `/…` 内建命令解析，以及 `$ …` 本地 shell 一行执行（`sh -c` / `cmd /C`）。

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReplBuiltIn<'a> {
    Clear,
    Model,
    /// `arg` 为命令名后的剩余文本；非空表示用户传了多余参数，应提示用法。
    Config(&'a str),
    /// 与 `crabmate doctor` 一致；`arg` 非空则报错。
    Doctor(&'a str),
    /// 与 `crabmate probe` 一致；`arg` 非空则报错；由 REPL 循环异步执行探测。
    Probe(&'a str),
    /// `/models` · `/models list`：同 `crabmate models`。
    ModelsList,
    /// `/models choose <id>`：从当前 `GET …/models` 列表设内存中的 `model`（支持唯一不区分大小写前缀）。
    ModelsChoose(String),
    /// `/models` 子命令用法错误（多余参数、未知子命令、`choose` 缺 id）。
    ModelsUsage,
    WorkspaceShow,
    WorkspaceSet(&'a str),
    Tools,
    Help,
    Export(&'a str),
    /// 与 `crabmate save-session` 一致：从磁盘会话文件导出（非当前内存）。
    SaveSession(&'a str),
    /// `/mcp` · `/mcp list` · `/mcp list probe` · `/mcp probe`（同 `crabmate mcp list`）
    McpList {
        probe: bool,
    },
    /// `/mcp …` 无法解析的子命令
    McpUnknown(String),
    /// `/version`：二进制与平台信息（不含密钥）
    Version,
    /// `/api-key`：用法说明
    ApiKeyUsage,
    /// `/api-key status`
    ApiKeyStatus,
    /// `/api-key clear`
    ApiKeyClear,
    /// `/api-key set <密钥>`：`set ` 后为完整密钥（仅本进程内存）
    ApiKeySet(String),
    /// `/agent list`：列出内建 `default` 与配置中的命名角色 id
    AgentList,
    /// `/agent set <id>`：校验 id 后更新 REPL 当前角色并**仅刷新首条 system**（保留后续对话）；**`default`** 清除显式命名角色
    AgentSet(String),
    /// `/agent …` 用法错误
    AgentUsage,
    Unknown(&'a str),
    BareSlash,
}

// --- `/models` · `/mcp` 子命令：静态表 + 小处理器，避免 `classify_repl_slash_command` 内重复分叉 ---
// 子分支仅产生不借用输入的变体，故返回 `ReplBuiltIn<'static>`，可安全并入外层 `ReplBuiltIn<'input>`。

type ModelsSubHandler = fn(&mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static>;

pub(crate) const MODELS_SUBCOMMAND_HANDLERS: &[(&str, ModelsSubHandler)] = &[
    ("choose", models_subcommand_choose),
    ("list", models_subcommand_list),
];

fn models_subcommand_list(parts: &mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static> {
    if parts.next().is_some() {
        ReplBuiltIn::ModelsUsage
    } else {
        ReplBuiltIn::ModelsList
    }
}

fn models_subcommand_choose(parts: &mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static> {
    let rest: String = parts.collect::<Vec<_>>().join(" ");
    let rest = rest.trim().to_string();
    if rest.is_empty() {
        ReplBuiltIn::ModelsUsage
    } else {
        ReplBuiltIn::ModelsChoose(rest)
    }
}

/// `/models`、**`/models list`**、**`/models choose …`**：首 token 在 [`MODELS_SUBCOMMAND_HANDLERS`] 中查找。
fn classify_models_slash_command(arg_tail: &str) -> ReplBuiltIn<'static> {
    let t = arg_tail.trim();
    if t.is_empty() {
        return ReplBuiltIn::ModelsList;
    }
    let mut parts = t.split_whitespace();
    let first = parts.next().unwrap_or("");
    let first_l = first.to_ascii_lowercase();
    for (name, handler) in MODELS_SUBCOMMAND_HANDLERS {
        if first_l == *name {
            return handler(&mut parts);
        }
    }
    ReplBuiltIn::ModelsUsage
}

type McpPrimaryHandler = fn(Option<&str>, &str) -> ReplBuiltIn<'static>;

pub(crate) const MCP_PRIMARY_HANDLERS: &[(&str, McpPrimaryHandler)] =
    &[("list", mcp_primary_list), ("probe", mcp_primary_probe)];

fn mcp_primary_list(second: Option<&str>, tail: &str) -> ReplBuiltIn<'static> {
    match second {
        None => ReplBuiltIn::McpList { probe: false },
        Some(x) if x.eq_ignore_ascii_case("probe") => ReplBuiltIn::McpList { probe: true },
        Some(_) => ReplBuiltIn::McpUnknown(tail.to_string()),
    }
}

fn mcp_primary_probe(second: Option<&str>, tail: &str) -> ReplBuiltIn<'static> {
    if second.is_none() {
        ReplBuiltIn::McpList { probe: true }
    } else {
        ReplBuiltIn::McpUnknown(tail.to_string())
    }
}

fn agent_subcommand_list(parts: &mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static> {
    if parts.next().is_some() {
        ReplBuiltIn::AgentUsage
    } else {
        ReplBuiltIn::AgentList
    }
}

fn agent_subcommand_set(parts: &mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static> {
    let rest: String = parts.collect::<Vec<_>>().join(" ");
    let rest = rest.trim().to_string();
    if rest.is_empty() {
        ReplBuiltIn::AgentUsage
    } else {
        ReplBuiltIn::AgentSet(rest)
    }
}

type AgentSubHandler = fn(&mut std::str::SplitWhitespace<'_>) -> ReplBuiltIn<'static>;

pub(crate) const AGENT_SUBCOMMAND_HANDLERS: &[(&str, AgentSubHandler)] = &[
    ("list", agent_subcommand_list),
    ("set", agent_subcommand_set),
];

/// `/agent`、**`/agent list`**、**`/agent set …`**：首 token 在 [`AGENT_SUBCOMMAND_HANDLERS`] 中查找。
fn classify_agent_slash_command(arg_tail: &str) -> ReplBuiltIn<'static> {
    let t = arg_tail.trim();
    if t.is_empty() {
        return ReplBuiltIn::AgentList;
    }
    let mut parts = t.split_whitespace();
    let first = parts.next().unwrap_or("");
    let first_l = first.to_ascii_lowercase();
    for (name, handler) in AGENT_SUBCOMMAND_HANDLERS {
        if first_l == *name {
            return handler(&mut parts);
        }
    }
    ReplBuiltIn::AgentUsage
}

/// `/agent set default`（不区分大小写、忽略首尾空白）：清除 REPL 显式 `agent_role`，与「未设置」及 Web 未选角色时一致（`default_agent_role_id` 或全局 `system_prompt`）。
pub(crate) fn repl_agent_role_set_is_default_pseudo(id: &str) -> bool {
    id.trim().eq_ignore_ascii_case("default")
}

/// `/mcp` 及其子形式：至多两个 token（否则 [`ReplBuiltIn::McpUnknown`]），首 token 在 [`MCP_PRIMARY_HANDLERS`] 中查找。
fn classify_mcp_slash_command(arg_tail: &str) -> ReplBuiltIn<'static> {
    let tail = arg_tail.trim();
    if tail.is_empty() {
        return ReplBuiltIn::McpList { probe: false };
    }
    let mut parts = tail.split_whitespace();
    let a = parts.next().unwrap_or("").to_ascii_lowercase();
    let b = parts.next();
    if parts.next().is_some() {
        return ReplBuiltIn::McpUnknown(tail.to_string());
    }
    for (name, handler) in MCP_PRIMARY_HANDLERS {
        if a == *name {
            return handler(b, tail);
        }
    }
    ReplBuiltIn::McpUnknown(tail.to_string())
}

/// `/api-key set …`：`set` 与大小写无关，其后为完整密钥（单行）。
fn repl_api_key_secret_after_set(arg_trim: &str) -> Option<&str> {
    let t = arg_trim.trim_start();
    const PREF: &str = "set ";
    if t.len() >= PREF.len() && t[..PREF.len()].eq_ignore_ascii_case(PREF) {
        let rest = t[PREF.len()..].trim();
        if rest.is_empty() { None } else { Some(rest) }
    } else {
        None
    }
}

/// 解析 REPL 行首 `/` 内建命令；非内建前缀返回 `None`。
pub(crate) fn classify_repl_slash_command(input: &str) -> Option<ReplBuiltIn<'_>> {
    let s = input.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = s[1..].trim();
    if rest.is_empty() {
        return Some(ReplBuiltIn::BareSlash);
    }
    let head = rest.split_whitespace().next().unwrap_or("");
    let cmd = head.to_ascii_lowercase();
    let arg = rest[head.len()..].trim();
    Some(match cmd.as_str() {
        "clear" => ReplBuiltIn::Clear,
        "model" => ReplBuiltIn::Model,
        "config" => ReplBuiltIn::Config(arg),
        "doctor" => ReplBuiltIn::Doctor(arg),
        "probe" => ReplBuiltIn::Probe(arg),
        "models" => classify_models_slash_command(arg),
        "workspace" | "cd" => {
            if arg.is_empty() {
                ReplBuiltIn::WorkspaceShow
            } else {
                ReplBuiltIn::WorkspaceSet(arg)
            }
        }
        "tools" => ReplBuiltIn::Tools,
        "help" | "?" => ReplBuiltIn::Help,
        "export" => ReplBuiltIn::Export(arg),
        "save-session" => ReplBuiltIn::SaveSession(arg),
        "mcp" => classify_mcp_slash_command(arg),
        "api-key" | "apikey" => {
            let a = arg.trim();
            if a.is_empty() {
                ReplBuiltIn::ApiKeyUsage
            } else if a.eq_ignore_ascii_case("status") {
                ReplBuiltIn::ApiKeyStatus
            } else if a.eq_ignore_ascii_case("clear") {
                ReplBuiltIn::ApiKeyClear
            } else if let Some(secret) = repl_api_key_secret_after_set(a) {
                ReplBuiltIn::ApiKeySet(secret.to_string())
            } else {
                ReplBuiltIn::ApiKeyUsage
            }
        }
        "agent" => classify_agent_slash_command(arg),
        "version" => ReplBuiltIn::Version,
        _ => ReplBuiltIn::Unknown(head),
    })
}

pub(crate) fn print_repl_version_line() {
    println!(
        "crabmate {} ({}/{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// 同步执行 REPL 本地 shell 一行（测试与 `repl` 共用）。
pub(crate) fn run_repl_shell_line_sync(cmd: &str, work_dir: &Path) -> io::Result<i32> {
    let status = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    } else {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .status()?
    };
    Ok(status
        .code()
        .unwrap_or(if status.success() { 0 } else { -1 }))
}

#[cfg(test)]
mod repl_slash_tests {
    use super::{ReplBuiltIn, classify_repl_slash_command, repl_agent_role_set_is_default_pseudo};

    #[test]
    fn not_slash_is_none() {
        assert!(classify_repl_slash_command("hello").is_none());
    }

    #[test]
    fn bare_slash() {
        assert_eq!(
            classify_repl_slash_command("  /  "),
            Some(ReplBuiltIn::BareSlash)
        );
    }

    #[test]
    fn clear_model_tools_help() {
        assert_eq!(
            classify_repl_slash_command("/CLEAR"),
            Some(ReplBuiltIn::Clear)
        );
        assert_eq!(
            classify_repl_slash_command("/model"),
            Some(ReplBuiltIn::Model)
        );
        assert_eq!(
            classify_repl_slash_command("/tools"),
            Some(ReplBuiltIn::Tools)
        );
        assert_eq!(
            classify_repl_slash_command("/help"),
            Some(ReplBuiltIn::Help)
        );
        assert_eq!(classify_repl_slash_command("/?"), Some(ReplBuiltIn::Help));
        assert_eq!(
            classify_repl_slash_command("/config"),
            Some(ReplBuiltIn::Config(""))
        );
        assert_eq!(
            classify_repl_slash_command("/CONFIG"),
            Some(ReplBuiltIn::Config(""))
        );
        assert_eq!(
            classify_repl_slash_command("/config reload"),
            Some(ReplBuiltIn::Config("reload"))
        );
        assert_eq!(
            classify_repl_slash_command("/config extra"),
            Some(ReplBuiltIn::Config("extra"))
        );
        assert_eq!(
            classify_repl_slash_command("/doctor"),
            Some(ReplBuiltIn::Doctor(""))
        );
        assert_eq!(
            classify_repl_slash_command("/probe"),
            Some(ReplBuiltIn::Probe(""))
        );
    }

    #[test]
    fn models_slash_variants() {
        assert_eq!(
            classify_repl_slash_command("/models"),
            Some(ReplBuiltIn::ModelsList)
        );
        assert_eq!(
            classify_repl_slash_command("/models list"),
            Some(ReplBuiltIn::ModelsList)
        );
        assert_eq!(
            classify_repl_slash_command("/models choose gpt-4o"),
            Some(ReplBuiltIn::ModelsChoose("gpt-4o".to_string()))
        );
        assert_eq!(
            classify_repl_slash_command("/models choose  a b c "),
            Some(ReplBuiltIn::ModelsChoose("a b c".to_string()))
        );
        assert_eq!(
            classify_repl_slash_command("/models choose"),
            Some(ReplBuiltIn::ModelsUsage)
        );
        assert_eq!(
            classify_repl_slash_command("/models list extra"),
            Some(ReplBuiltIn::ModelsUsage)
        );
        assert_eq!(
            classify_repl_slash_command("/models bogus"),
            Some(ReplBuiltIn::ModelsUsage)
        );
    }

    #[test]
    fn workspace_and_cd() {
        assert_eq!(
            classify_repl_slash_command("/workspace"),
            Some(ReplBuiltIn::WorkspaceShow)
        );
        assert_eq!(
            classify_repl_slash_command("/workspace /tmp"),
            Some(ReplBuiltIn::WorkspaceSet("/tmp"))
        );
        assert_eq!(
            classify_repl_slash_command("  /cd  ./foo  "),
            Some(ReplBuiltIn::WorkspaceSet("./foo"))
        );
    }

    #[test]
    fn unknown() {
        assert_eq!(
            classify_repl_slash_command("/nope"),
            Some(ReplBuiltIn::Unknown("nope"))
        );
    }

    #[test]
    fn mcp_and_version() {
        assert_eq!(
            classify_repl_slash_command("/mcp"),
            Some(ReplBuiltIn::McpList { probe: false })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp list"),
            Some(ReplBuiltIn::McpList { probe: false })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp probe"),
            Some(ReplBuiltIn::McpList { probe: true })
        );
        assert_eq!(
            classify_repl_slash_command("/mcp list probe"),
            Some(ReplBuiltIn::McpList { probe: true })
        );
        assert!(matches!(
            classify_repl_slash_command("/mcp list probe extra"),
            Some(ReplBuiltIn::McpUnknown(_))
        ));
        assert_eq!(
            classify_repl_slash_command("/version"),
            Some(ReplBuiltIn::Version)
        );
    }

    #[test]
    fn api_key_slash_variants() {
        assert_eq!(
            classify_repl_slash_command("/api-key"),
            Some(ReplBuiltIn::ApiKeyUsage)
        );
        assert_eq!(
            classify_repl_slash_command("/apikey status"),
            Some(ReplBuiltIn::ApiKeyStatus)
        );
        assert_eq!(
            classify_repl_slash_command("/api-key clear"),
            Some(ReplBuiltIn::ApiKeyClear)
        );
        assert_eq!(
            classify_repl_slash_command("/API-KEY SET sk-test"),
            Some(ReplBuiltIn::ApiKeySet("sk-test".to_string()))
        );
    }

    #[test]
    fn agent_slash_variants() {
        assert_eq!(
            classify_repl_slash_command("/agent"),
            Some(ReplBuiltIn::AgentList)
        );
        assert_eq!(
            classify_repl_slash_command("/agent list"),
            Some(ReplBuiltIn::AgentList)
        );
        assert_eq!(
            classify_repl_slash_command("/agent list extra"),
            Some(ReplBuiltIn::AgentUsage)
        );
        assert_eq!(
            classify_repl_slash_command("/agent set  code"),
            Some(ReplBuiltIn::AgentSet("code".to_string()))
        );
        assert_eq!(
            classify_repl_slash_command("/agent set  a b c "),
            Some(ReplBuiltIn::AgentSet("a b c".to_string()))
        );
        assert_eq!(
            classify_repl_slash_command("/agent set"),
            Some(ReplBuiltIn::AgentUsage)
        );
        assert_eq!(
            classify_repl_slash_command("/agent bogus"),
            Some(ReplBuiltIn::AgentUsage)
        );
    }

    #[test]
    fn repl_agent_role_default_pseudo() {
        assert!(repl_agent_role_set_is_default_pseudo("default"));
        assert!(repl_agent_role_set_is_default_pseudo(" Default "));
        assert!(!repl_agent_role_set_is_default_pseudo("companion"));
        assert!(!repl_agent_role_set_is_default_pseudo("defaults"));
        assert_eq!(
            classify_repl_slash_command("/agent set default"),
            Some(ReplBuiltIn::AgentSet("default".to_string()))
        );
    }
}

#[cfg(test)]
mod repl_slash_subcommand_table_tests {
    use super::{AGENT_SUBCOMMAND_HANDLERS, MCP_PRIMARY_HANDLERS, MODELS_SUBCOMMAND_HANDLERS};

    #[test]
    fn models_subcommand_table_sorted_unique() {
        let names: Vec<&str> = MODELS_SUBCOMMAND_HANDLERS.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "MODELS_SUBCOMMAND_HANDLERS 应按名字典序排列");
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            "MODELS_SUBCOMMAND_HANDLERS 名字须唯一"
        );
    }

    #[test]
    fn mcp_primary_table_sorted_unique() {
        let names: Vec<&str> = MCP_PRIMARY_HANDLERS.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "MCP_PRIMARY_HANDLERS 应按名字典序排列");
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            "MCP_PRIMARY_HANDLERS 名字须唯一"
        );
    }

    #[test]
    fn agent_subcommand_table_sorted_unique() {
        let names: Vec<&str> = AGENT_SUBCOMMAND_HANDLERS.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "AGENT_SUBCOMMAND_HANDLERS 应按名字典序排列");
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            "AGENT_SUBCOMMAND_HANDLERS 名字须唯一"
        );
    }
}

#[cfg(test)]
mod repl_dollar_tests {
    use super::run_repl_shell_line_sync;
    use crate::runtime::repl_reedline::parse_repl_dollar_shell_line;

    #[test]
    fn parse_not_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("hello"), None);
    }

    #[test]
    fn parse_bare_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("$"), Some(None));
    }

    #[test]
    fn parse_bare_fullwidth_dollar() {
        assert_eq!(parse_repl_dollar_shell_line("\u{ff04}"), Some(None));
    }

    #[test]
    fn parse_fullwidth_dollar_ls() {
        assert_eq!(
            parse_repl_dollar_shell_line("\u{ff04} ls"),
            Some(Some("ls"))
        );
    }

    #[test]
    fn parse_dollar_ls() {
        assert_eq!(parse_repl_dollar_shell_line("$ ls"), Some(Some("ls")));
    }

    #[test]
    fn parse_dollar_leading_space() {
        assert_eq!(
            parse_repl_dollar_shell_line("  $ echo x"),
            Some(Some("echo x"))
        );
    }

    #[test]
    fn shell_true_zero_exit() {
        let dir = std::env::temp_dir();
        let cmd = if cfg!(windows) { "exit /b 0" } else { "true" };
        let code = run_repl_shell_line_sync(cmd, &dir).unwrap();
        assert_eq!(code, 0);
    }
}
