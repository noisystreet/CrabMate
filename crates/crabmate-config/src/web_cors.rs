//! Web CORS Origin 白名单解析：官方壳默认 Origin + 显式扩展 / 关闭。

/// 官方 Client 壳 WebView 的 Origin（Phase 2 包内 UI → 远程 API）。
///
/// - `tauri://localhost`：Linux WebKitGTK 对包内页 `fetch` 的实际 `Origin`
/// - `http://tauri.localhost`：Android http 资产 / 部分 WebView
pub const DEFAULT_SHELL_CORS_ORIGINS: &[&str] = &["tauri://localhost", "http://tauri.localhost"];

/// 解析最终挂到 `serve` 的 CORS 白名单。
///
/// | 输入 | 结果 |
/// |------|------|
/// | `None`（未配置） | [`DEFAULT_SHELL_CORS_ORIGINS`] |
/// | `Some([])` 或仅空白 | 空（显式关闭 CORS） |
/// | `Some(自定义…)` | 自定义 ∪ 默认壳 Origin（去重，保留自定义顺序，缺的默认项追加） |
#[must_use]
pub fn resolve_web_cors_allowed_origins(configured: Option<Vec<String>>) -> Vec<String> {
    match configured {
        None => DEFAULT_SHELL_CORS_ORIGINS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        Some(list) => {
            let trimmed: Vec<String> = list
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                merge_default_shell_cors_origins(trimmed)
            }
        }
    }
}

fn merge_default_shell_cors_origins(mut list: Vec<String>) -> Vec<String> {
    for origin in DEFAULT_SHELL_CORS_ORIGINS {
        if !list.iter().any(|x| x == *origin) {
            list.push((*origin).to_string());
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_uses_shell_defaults() {
        assert_eq!(
            resolve_web_cors_allowed_origins(None),
            vec![
                "tauri://localhost".to_string(),
                "http://tauri.localhost".to_string()
            ]
        );
    }

    #[test]
    fn explicit_empty_disables_cors() {
        assert!(resolve_web_cors_allowed_origins(Some(vec![])).is_empty());
        assert!(resolve_web_cors_allowed_origins(Some(vec!["  ".into(), "\t".into()])).is_empty());
    }

    #[test]
    fn custom_list_merges_shell_defaults() {
        let got = resolve_web_cors_allowed_origins(Some(vec![
            "http://127.0.0.1:8081".into(),
            "https://ui.example.com".into(),
        ]));
        assert_eq!(
            got,
            vec![
                "http://127.0.0.1:8081".to_string(),
                "https://ui.example.com".to_string(),
                "tauri://localhost".to_string(),
                "http://tauri.localhost".to_string(),
            ]
        );
    }

    #[test]
    fn custom_list_does_not_duplicate_shell_origins() {
        let got = resolve_web_cors_allowed_origins(Some(vec![
            "tauri://localhost".into(),
            "http://127.0.0.1:9".into(),
        ]));
        assert_eq!(
            got,
            vec![
                "tauri://localhost".to_string(),
                "http://127.0.0.1:9".to_string(),
                "http://tauri.localhost".to_string(),
            ]
        );
    }
}
