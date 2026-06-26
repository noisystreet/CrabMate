//! 将通用 MCP 配置 JSON（`mcpServers`）转为 `McpServersFile` 条目（与 Web 设置页导入逻辑对齐）。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use super::types::McpServerEntry;

#[derive(Debug, Default)]
pub struct McpJsonImportResult {
    pub entries: Vec<McpServerEntry>,
    pub warnings: Vec<String>,
    pub skipped_remote: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct McpJsonServerDef {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(rename = "envFile")]
    env_file: Option<String>,
}

/// 解析 JSON 值（完整 `mcp.json`、仅 `mcpServers` 或单条 server 对象）。
pub fn import_mcp_json_value(root: &Value) -> Result<McpJsonImportResult, String> {
    let servers_obj = extract_mcp_servers_object(root)?;
    let mut out = McpJsonImportResult::default();
    for (key, value) in servers_obj {
        let server: McpJsonServerDef =
            serde_json::from_value(value).map_err(|e| format!("服务器「{key}」格式无效: {e}"))?;
        import_one_server(&key, server, &mut out);
    }
    if out.entries.is_empty() && out.skipped_remote.is_empty() {
        return Err("未找到可导入的 stdio MCP 服务器（需含 command）".to_string());
    }
    Ok(out)
}

fn extract_mcp_servers_object(root: &Value) -> Result<HashMap<String, Value>, String> {
    if let Some(map) = root.get("mcpServers").and_then(Value::as_object) {
        return Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    }
    if let Some(map) = root.as_object()
        && map
            .values()
            .all(|v| v.get("command").is_some() || v.get("url").is_some())
        && !map.contains_key("schema_version")
        && !map.contains_key("servers")
    {
        return Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    }
    if root.get("command").is_some() || root.get("url").is_some() {
        let mut m = HashMap::new();
        m.insert("imported".to_string(), root.clone());
        return Ok(m);
    }
    Err("缺少 mcpServers 对象（须为常见 MCP 配置 JSON 格式）".to_string())
}

fn import_one_server(key: &str, server: McpJsonServerDef, out: &mut McpJsonImportResult) {
    let has_command = server
        .command
        .as_ref()
        .is_some_and(|c| !c.trim().is_empty());
    let has_url = server.url.as_ref().is_some_and(|u| !u.trim().is_empty());
    if !has_command {
        if has_url {
            out.skipped_remote.push(key.to_string());
        }
        return;
    }
    let command = server.command.unwrap_or_default();
    let args = server.args.unwrap_or_default();
    let env = server.env.unwrap_or_default();
    let cwd = server.cwd.filter(|s| !s.trim().is_empty());

    if let Some(path) = server.env_file.filter(|s| !s.trim().is_empty()) {
        out.warnings.push(format!(
            "「{key}」：envFile（{path}）未自动加载，请改用 env 或在本机 shell 中导出变量"
        ));
    }
    if contains_mcp_json_placeholders(&command)
        || args.iter().any(|a| contains_mcp_json_placeholders(a))
        || env.values().any(|v| contains_mcp_json_placeholders(v))
        || cwd
            .as_ref()
            .is_some_and(|c| contains_mcp_json_placeholders(c))
    {
        out.warnings.push(format!(
            "「{key}」：含 ${{env:…}} / ${{workspaceFolder}} 等占位符，导入后请按需改路径或在本机设置环境变量"
        ));
    }

    let cmdline = build_stdio_command_line(&command, &args, &env, cwd.as_deref());
    let enabled = !server.disabled.unwrap_or(false);
    let now = super::store::now_ms();

    out.entries.push(McpServerEntry {
        id: super::store::new_mcp_server_id(),
        name: name_from_mcp_server_key(key),
        slug: String::new(),
        command: cmdline,
        enabled,
        created_at_ms: now,
        updated_at_ms: now,
    });
}

fn contains_mcp_json_placeholders(s: &str) -> bool {
    s.contains("${")
}

fn name_from_mcp_server_key(key: &str) -> String {
    key.split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn build_stdio_command_line(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
    cwd: Option<&str>,
) -> String {
    let cmd = command.trim();
    if env.is_empty() && cwd.is_none() {
        return join_argv(cmd, args);
    }
    let mut script = String::new();
    if let Some(dir) = cwd.filter(|d| !d.trim().is_empty()) {
        script.push_str("cd ");
        script.push_str(&shell_quote(dir));
        script.push_str(" && ");
    }
    for (k, v) in env {
        if k.trim().is_empty() {
            continue;
        }
        script.push_str("export ");
        script.push_str(k.trim());
        script.push('=');
        script.push_str(&shell_quote(v));
        script.push_str("; ");
    }
    script.push_str(cmd);
    for arg in args {
        script.push(' ');
        script.push_str(&shell_quote(arg));
    }
    format!("sh -c {}", shell_quote(&script))
}

fn join_argv(command: &str, args: &[String]) -> String {
    let mut parts = vec![shell_quote(command)];
    for a in args {
        parts.push(shell_quote(a));
    }
    parts.join(" ")
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | '%' | '+')
    }) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn import_fanalyzer_shape() {
        let root = json!({
            "mcpServers": {
                "fanalyzer": {
                    "command": "/home/gzz/code/analysis_fund/target/debug/fanalyzer",
                    "args": ["mcp", "serve", "--profile", "summary"],
                    "cwd": "/home/gzz/code/analysis_fund",
                    "env": {"RUST_LOG": "warn"}
                }
            }
        });
        let r = import_mcp_json_value(&root).expect("import");
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].name, "Fanalyzer");
        assert!(r.entries[0].command.contains("fanalyzer"));
        assert!(!r.entries[0].id.is_empty());
        let argv = cmd_mate::split_command_line(&r.entries[0].command);
        assert_eq!(argv.first().map(String::as_str), Some("sh"));
        assert_eq!(argv.get(1).map(String::as_str), Some("-c"));
        assert!(
            argv.get(2)
                .is_some_and(|s| s.contains("fanalyzer") && s.contains("analysis_fund"))
        );
    }
}
