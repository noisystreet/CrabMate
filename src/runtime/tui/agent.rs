//! TUI 侧 agent 回合（委托 `agent_turn::run_agent_turn_common`）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::{Mutex, mpsc};

use crate::config::AgentConfig;
use crate::types::Message;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_agent_turn_tui(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &AgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&mpsc::Sender<String>>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    no_stream: bool,
    persistent_allowlist: HashSet<String>,
    approval_rx: mpsc::Receiver<crate::types::CommandApprovalDecision>,
    cancel: Option<&AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let out_tx = out.cloned();
    let approval_rx_shared: Arc<Mutex<mpsc::Receiver<crate::types::CommandApprovalDecision>>> =
        Arc::new(Mutex::new(approval_rx));
    let approval_request_guard = Arc::new(Mutex::new(()));
    let persistent_allowlist_shared = Arc::new(Mutex::new(persistent_allowlist));
    let tui_tool_ctx = crate::tool_registry::TuiToolRuntime {
        out_tx,
        approval_rx_shared,
        approval_request_guard,
        persistent_allowlist_shared,
    };
    crate::agent_turn::run_agent_turn_common(
        client,
        api_key,
        cfg,
        tools,
        messages,
        out,
        effective_working_dir,
        workspace_is_set,
        no_stream,
        cancel,
        crate::agent_turn::AgentRunMode::Tui {
            tui_tool_ctx: &tui_tool_ctx,
        },
    )
    .await
}
