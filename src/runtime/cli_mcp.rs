//! `crabmate mcp` 子命令：只读查看本进程内 MCP stdio 缓存（与 `serve` / `repl` / `chat` 共用）。

use crate::config::AgentConfig;

/// 执行 `mcp list`（`probe` 为 true 时按配置尝试建立/刷新进程内 MCP 缓存）。
pub async fn run_mcp_list(cfg: &AgentConfig, probe: bool) {
    if probe {
        let _ = crate::mcp::try_open_session_and_tools(cfg).await;
    }
    let st = crate::mcp::cached_mcp_status(cfg).await;
    if !cfg.mcp_enabled {
        println!("MCP：配置中未启用 (mcp_enabled=false)。本进程无 MCP 会话缓存。");
        return;
    }
    if cfg.mcp_command.trim().is_empty() {
        println!("MCP：已启用但 mcp_command 为空，无法建立 stdio 会话。");
        return;
    }
    if !st.fingerprint_matches_config {
        if probe {
            println!(
                "MCP：已尝试按配置连接，但未在进程内留下可用会话（见日志 target=crabmate）。\
                 常见原因：子进程启动失败、握手失败或 tools/list 为空。"
            );
        } else {
            println!(
                "MCP：本进程内尚无与当前配置匹配的已缓存 stdio 会话。\
                 请先在本进程中执行至少一轮对话（`repl` / `chat` / Web `/chat`），\
                 或使用 `crabmate mcp list --probe` 尝试立即连接一次。"
            );
        }
        return;
    }
    let slug = st.slug.as_deref().unwrap_or("?");
    println!("MCP：本进程内已缓存 stdio 会话（slug={slug}）");
    println!(
        "合并后的 OpenAI 工具名（{} 个）：",
        st.openai_tool_names.len()
    );
    for name in &st.openai_tool_names {
        println!("  {name}");
    }
}
