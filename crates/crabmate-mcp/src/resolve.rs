//! MCP 配置类型及基础构造。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crabmate_config::AgentConfig;

/// stdio 子进程启动规格（结构化 `command`/`args`/`env`/`cwd`，或 legacy 整行拆分）。
#[derive(Debug, Clone)]
pub struct McpStdioLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

/// 单条已启用的 MCP 服务器（运行时视图：stdio 或远程 Streamable HTTP）。
#[derive(Debug, Clone)]
pub struct ResolvedMcpServer {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// 可执行文件路径，或 legacy 整行命令（`args`/`env`/`cwd` 皆空时按 shell 词法拆分）。
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    /// Streamable HTTP 端点（与 `command` 互斥）。
    pub url: Option<String>,
    /// 远程请求附加头（含可选 `Authorization`）。
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
}

impl ResolvedMcpServer {
    pub fn has_stdio(&self) -> bool {
        !self.command.trim().is_empty()
    }

    pub fn has_remote_url(&self) -> bool {
        self.url.as_ref().is_some_and(|u| !u.trim().is_empty())
    }

    /// 是否含结构化启动字段（非空 `args` / `env` / `cwd`）。
    pub fn has_structured_launch(&self) -> bool {
        !self.args.is_empty()
            || !self.env.is_empty()
            || self.cwd.as_ref().is_some_and(|c| !c.trim().is_empty())
    }

    /// 解析为可直接 `Command::new` 的启动规格。
    pub fn stdio_launch(&self) -> Result<McpStdioLaunch, String> {
        let cmd = self.command.trim();
        if cmd.is_empty() {
            return Err("command 为空".to_string());
        }
        if self.has_structured_launch() {
            return Ok(McpStdioLaunch {
                program: cmd.to_string(),
                args: self.args.clone(),
                env: self.env.clone(),
                cwd: self
                    .cwd
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
            });
        }
        let parts = cmd_mate::split_command_line(cmd);
        if parts.is_empty() {
            return Err("MCP command 为空或仅空白".to_string());
        }
        Ok(McpStdioLaunch {
            program: parts[0].clone(),
            args: parts[1..].to_vec(),
            env: BTreeMap::new(),
            cwd: None,
        })
    }
}

/// 校验远程 MCP URL（默认偏保守，降低 SSRF 面）。
///
/// - 允许任意 `https://`
/// - 允许 loopback `http://`（`127.0.0.1` / `localhost` / `[::1]`）
/// - 拒绝其它 `http://` 与非 http(s) 方案
pub fn validate_mcp_remote_url(url: &str) -> Result<(), String> {
    let raw = url.trim();
    if raw.is_empty() {
        return Err("url 为空".to_string());
    }
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err("url 须含 scheme（https:// 或 http://）".to_string());
    };
    let scheme = scheme.to_ascii_lowercase();
    let after_auth = rest.split_once('@').map(|(_, host)| host).unwrap_or(rest);
    let host_port = after_auth
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    let host = host_port
        .rsplit_once(':')
        .filter(|(h, p)| {
            // IPv6 in brackets: keep whole `[::1]`；普通 host:port 才拆端口
            !h.starts_with('[') && p.chars().all(|c| c.is_ascii_digit())
        })
        .map(|(h, _)| h)
        .unwrap_or(host_port)
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();

    match scheme.as_str() {
        "https" => Ok(()),
        "http" => {
            let ok = host == "127.0.0.1" || host == "localhost" || host == "::1";
            if ok {
                Ok(())
            } else {
                Err(
                    "非 HTTPS 的远程 MCP 仅允许 http://127.0.0.1、http://localhost 或 http://[::1]"
                        .to_string(),
                )
            }
        }
        other => Err(format!("不支持的 URL scheme「{other}」（仅 http/https）")),
    }
}

/// 本轮 agent 使用的 MCP 配置。
#[derive(Debug, Clone)]
pub struct ResolvedMcpConfig {
    pub global_enabled: bool,
    pub tool_timeout_secs: u64,
    pub servers: Vec<ResolvedMcpServer>,
}

impl ResolvedMcpConfig {
    pub fn enabled_servers(&self) -> impl Iterator<Item = &ResolvedMcpServer> {
        self.servers
            .iter()
            .filter(|s| s.enabled && (s.has_stdio() || s.has_remote_url()))
    }
}

/// 从 `cfg` 构造基础 MCP 配置（无 user-data 覆盖；`servers` 恒空）。
///
/// Agent 回合须使用 `crabmate_internal::mcp::resolve_mcp_config`（加载 `mcp_servers.json`）。
pub fn resolve_mcp_config(cfg: &AgentConfig) -> ResolvedMcpConfig {
    ResolvedMcpConfig {
        global_enabled: cfg.mcp_client.mcp_enabled,
        tool_timeout_secs: cfg.mcp_client.mcp_tool_timeout_secs.max(1),
        servers: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_launch_keeps_program_and_env() {
        let mut env = BTreeMap::new();
        env.insert("RUST_LOG".into(), "warn".into());
        let srv = ResolvedMcpServer {
            id: "1".into(),
            name: "F".into(),
            slug: "f".into(),
            command: "fanalyzer".into(),
            args: vec!["mcp".into(), "serve".into()],
            env,
            cwd: Some("/tmp/ws".into()),
            url: None,
            headers: BTreeMap::new(),
            enabled: true,
        };
        let launch = srv.stdio_launch().expect("launch");
        assert_eq!(launch.program, "fanalyzer");
        assert_eq!(launch.args, vec!["mcp", "serve"]);
        assert_eq!(launch.env.get("RUST_LOG").map(String::as_str), Some("warn"));
        assert_eq!(launch.cwd.as_deref(), Some(std::path::Path::new("/tmp/ws")));
    }

    #[test]
    fn legacy_cmdline_splits_sh_c() {
        let srv = ResolvedMcpServer {
            id: "1".into(),
            name: "F".into(),
            slug: "f".into(),
            command: "sh -c 'export RUST_LOG=warn; fanalyzer mcp serve'".into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
            enabled: true,
        };
        let launch = srv.stdio_launch().expect("launch");
        assert_eq!(launch.program, "sh");
        assert_eq!(launch.args.first().map(String::as_str), Some("-c"));
        assert!(launch.args.get(1).is_some_and(|s| s.contains("fanalyzer")));
    }

    #[test]
    fn validate_remote_url_https_and_loopback_http() {
        assert!(validate_mcp_remote_url("https://mcp.example/mcp").is_ok());
        assert!(validate_mcp_remote_url("http://127.0.0.1:8080/mcp").is_ok());
        assert!(validate_mcp_remote_url("http://localhost/mcp").is_ok());
        assert!(validate_mcp_remote_url("http://evil.example/mcp").is_err());
        assert!(validate_mcp_remote_url("ftp://x").is_err());
    }
}
