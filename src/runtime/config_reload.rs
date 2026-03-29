//! 进程内配置热重载（REPL **`/config reload`**、Web **`POST /config/reload`**）。

use crate::config::{AgentConfig, apply_hot_reload_config_subset};

/// 自磁盘+环境变量重新 [`crate::load_config`]，将可热更字段合并进 `holder`，并清空 MCP 进程缓存。
///
/// **不**重连会话 SQLite、**不**重建 `reqwest::Client`；边界见 [`apply_hot_reload_config_subset`] 文档字符串。
pub async fn reload_shared_agent_config(
    holder: &tokio::sync::RwLock<AgentConfig>,
    config_path: Option<&str>,
) -> Result<(), String> {
    let fresh = crate::load_config(config_path)?;
    {
        let mut w = holder.write().await;
        apply_hot_reload_config_subset(&mut w, &fresh);
    }
    crate::mcp::clear_mcp_process_cache().await;
    Ok(())
}
