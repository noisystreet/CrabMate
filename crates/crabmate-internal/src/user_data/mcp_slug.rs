//! MCP 服务器 `slug`：由展示名 `name` 自动生成（OpenAI 工具前缀 `mcp__{slug}__…`）。

use std::collections::HashSet;

use super::types::McpServerEntry;

/// 将展示名转为 slug 基串（小写字母数字与下划线；空则 `"mcp"`）。
pub fn base_slug_from_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "mcp".to_string() } else { s }
}

fn unique_slug(base: &str, used: &HashSet<String>) -> String {
    let base = if base.is_empty() {
        "mcp".to_string()
    } else {
        base.to_string()
    };
    if !used.contains(&base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !used.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 按当前 `name` 为列表中每条服务器分配唯一 `slug`（冲突时追加 `_2`、`_3`…）。
pub fn assign_slugs_from_names(servers: &mut [McpServerEntry]) {
    let mut used = HashSet::new();
    for srv in servers.iter_mut() {
        let base = base_slug_from_name(&srv.name);
        let slug = unique_slug(&base, &used);
        used.insert(slug.clone());
        srv.slug = slug;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_display_name_basic() {
        assert_eq!(base_slug_from_name("Filesystem"), "filesystem");
        assert_eq!(base_slug_from_name("My MCP Server"), "my_mcp_server");
        assert_eq!(base_slug_from_name("  "), "mcp");
    }

    #[test]
    fn assign_slugs_deduplicates() {
        let mut servers = vec![
            McpServerEntry {
                id: "a".into(),
                name: "Foo".into(),
                slug: String::new(),
                command: String::new(),
                enabled: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            McpServerEntry {
                id: "b".into(),
                name: "Foo".into(),
                slug: String::new(),
                command: String::new(),
                enabled: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
        ];
        assign_slugs_from_names(&mut servers);
        assert_eq!(servers[0].slug, "foo");
        assert_eq!(servers[1].slug, "foo_2");
    }
}
