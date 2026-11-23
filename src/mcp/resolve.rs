//! MCP 配置类型及基础构造。

use std::collections::BTreeMap;
use std::path::PathBuf;

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

    /// CLI / status 用传输标签：`stdio` | `remote` | `none`。
    pub fn transport_label(&self) -> &'static str {
        if self.has_remote_url() {
            "remote"
        } else if self.has_stdio() {
            "stdio"
        } else {
            "none"
        }
    }

    /// 是否含结构化启动字段（非空 `args` / `env` / `cwd`）。
    pub fn has_structured_launch(&self) -> bool {
        !self.args.is_empty()
            || !self.env.is_empty()
            || self.cwd.as_ref().is_some_and(|c| !c.trim().is_empty())
    }

    /// 传输配置自相矛盾或缺失时给出**结构化**结论（`None` = 配置合法）。
    ///
    /// 判定只读 `command` / `url` 两个字段是否为空，产出 [`McpConfigError`]；
    /// 下游按变体取 `kind`，不再对中文校验文案做子串匹配。
    pub fn config_error(&self) -> Option<McpConfigError> {
        match (self.has_stdio(), self.has_remote_url()) {
            (true, true) => Some(McpConfigError::BothStdioAndUrl),
            (false, false) => Some(McpConfigError::NeitherStdioNorUrl),
            _ => None,
        }
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
        let parts = crate::cmd_mate::split_command_line(cmd);
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

/// 本仓**自产**的 MCP 配置校验失败。
///
/// 与外部握手 / DNS / TLS 错误分开：这些结论在产出点就是确定的，展示文案只是它的渲染结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigError {
    /// `command` 与 `url` 同时非空。
    BothStdioAndUrl,
    /// 两者都为空（启用但没有传输）。
    NeitherStdioNorUrl,
}

impl McpConfigError {
    /// status / CLI 用的稳定英文 kind（与旧 `classify_mcp_connect_error` 的 `config` 同名）。
    pub const KIND: &'static str = "config";

    /// 用户可见文案；`BothStdioAndUrl` 与历史字符串逐字一致。
    pub fn message(self) -> &'static str {
        match self {
            Self::BothStdioAndUrl => "不能同时配置 command 与 url",
            Self::NeitherStdioNorUrl => "须配置 command 或 url 之一",
        }
    }
}

impl std::fmt::Display for McpConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
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

/// 将**外部**连接/握手类错误文案归类，供 status / CLI 展示（稳定英文 kind）。
///
/// 只处理外部错误（DNS / TLS / 鉴权 / 握手…）。本仓自产的配置错误不在这里：
/// [`ResolvedMcpServer::config_error`] 在产出点就给出 [`McpConfigError`]，
/// 调用方直接取 [`McpConfigError::KIND`]，不参与下面的子串匹配。
pub fn classify_mcp_connect_error(msg: &str) -> &'static str {
    let lower = msg.to_ascii_lowercase();
    for (kind, needles) in MCP_CONNECT_ERROR_RULES {
        if needles.iter().any(|n| lower.contains(n)) {
            return kind;
        }
    }
    "unknown"
}

const MCP_CONNECT_ERROR_RULES: &[(&str, &[&str])] = &[
    (
        "url_invalid",
        &[
            "url 为空",
            "url 须含",
            "url 无效",
            "不支持的 url scheme",
            "非 https 的远程",
        ],
    ),
    ("header_invalid", &["非法 header"]),
    (
        "unauthorized",
        &["401", "unauthorized", "authentication", "鉴权"],
    ),
    (
        "dns",
        &[
            "name or service not known",
            "nodename nor servname",
            "dns",
            "failed to lookup",
            "no such host",
        ],
    ),
    ("tls", &["certificate", "tls", "ssl", "handshake failure"]),
    ("spawn", &["子进程", "启动 mcp", "no such file"]),
    ("tools_list", &["tools/list"]),
    ("handshake", &["握手", "handshake", "远程 mcp 握手"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_remote_errors() {
        assert_eq!(
            classify_mcp_connect_error("非 HTTPS 的远程 MCP 仅允许 http://127.0.0.1"),
            "url_invalid"
        );
        assert_eq!(
            classify_mcp_connect_error("远程 MCP 握手失败: error sending request for url"),
            "handshake"
        );
        assert_eq!(
            classify_mcp_connect_error("tools/list 失败: timeout"),
            "tools_list"
        );
        assert_eq!(
            classify_mcp_connect_error("error: 401 Unauthorized"),
            "unauthorized"
        );
    }

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

    fn server_with(command: &str, url: Option<&str>) -> ResolvedMcpServer {
        ResolvedMcpServer {
            id: "1".into(),
            name: "F".into(),
            slug: "f".into(),
            command: command.into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: url.map(str::to_string),
            headers: BTreeMap::new(),
            enabled: true,
        }
    }

    #[test]
    fn config_error_comes_from_fields_not_from_message() {
        assert_eq!(
            server_with("fanalyzer", Some("https://x/mcp")).config_error(),
            Some(McpConfigError::BothStdioAndUrl)
        );
        assert_eq!(
            server_with("", None).config_error(),
            Some(McpConfigError::NeitherStdioNorUrl)
        );
        assert_eq!(server_with("fanalyzer", None).config_error(), None);
        assert_eq!(
            server_with("", Some("https://x/mcp")).config_error(),
            None
        );
        // 展示文案保持历史口径（用户已在日志/CLI 里见过这句）。
        assert_eq!(
            McpConfigError::BothStdioAndUrl.to_string(),
            "不能同时配置 command 与 url"
        );
        assert_eq!(McpConfigError::KIND, "config");
        // 文案匹配只服务外部错误：自产配置文案不再被规则表命中。
        assert_eq!(
            classify_mcp_connect_error(&McpConfigError::BothStdioAndUrl.to_string()),
            "unknown"
        );
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
