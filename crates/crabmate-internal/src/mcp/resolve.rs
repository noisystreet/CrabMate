//! 从 user-data（及 legacy TOML 一次性导入）解析本轮 MCP 配置。
//!
//! `crabmate-mcp` crate 中只提供了基于 cfg 的基础构造，本模块补全 user-data 加载层。

use std::collections::BTreeMap;

use crabmate_config::AgentConfig;
use crabmate_mcp::resolve::{ResolvedMcpConfig, ResolvedMcpServer};

use crate::user_data::{load_mcp_servers_with_legacy_import, read_secret_mcp_bearer};

/// 读取 user-data MCP 列表；空列表时尝试从 TOML `mcp_*` 一次性导入。
///
/// 若存在 `secrets/mcp_bearer_{id}`，合并为 `Authorization: Bearer …`（覆盖条目内同名头）。
pub fn resolve_mcp_config(cfg: &AgentConfig) -> ResolvedMcpConfig {
    let file = load_mcp_servers_with_legacy_import(
        cfg.mcp_client.mcp_enabled,
        cfg.mcp_client.mcp_command.trim(),
        cfg.mcp_client.mcp_tool_timeout_secs,
    );
    let tool_timeout_secs = if file.tool_timeout_secs > 0 {
        file.tool_timeout_secs
    } else {
        cfg.mcp_client.mcp_tool_timeout_secs.max(1)
    };
    ResolvedMcpConfig {
        global_enabled: file.global_enabled,
        tool_timeout_secs,
        servers: file
            .servers
            .into_iter()
            .map(|s| {
                let mut headers = s.headers;
                merge_mcp_bearer_header(&s.id, &mut headers);
                ResolvedMcpServer {
                    id: s.id,
                    name: s.name,
                    slug: s.slug,
                    command: s.command,
                    args: s.args,
                    env: s.env,
                    cwd: s.cwd,
                    url: s.url,
                    headers,
                    enabled: s.enabled,
                }
            })
            .collect(),
    }
}

fn merge_mcp_bearer_header(server_id: &str, headers: &mut BTreeMap<String, String>) {
    let Some(token) = read_secret_mcp_bearer(server_id) else {
        return;
    };
    let token = token.trim();
    if token.is_empty() {
        return;
    }
    headers.retain(|k, _| !k.eq_ignore_ascii_case("authorization"));
    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
}

#[cfg(test)]
mod tests {
    use super::merge_mcp_bearer_header;
    use std::collections::BTreeMap;

    #[test]
    fn merge_bearer_skips_when_secret_absent() {
        let mut headers = BTreeMap::new();
        headers.insert("X-Custom".into(), "1".into());
        merge_mcp_bearer_header("mcp_no_such_secret_id_zzzz", &mut headers);
        assert_eq!(headers.len(), 1);
        assert!(!headers.contains_key("Authorization"));
    }
}
