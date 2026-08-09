//! REPL 首轮消息后台扫描与行编辑器初始化。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::ProcessHandles;
use crate::config::SharedAgentConfig;
use crate::runtime::repl_reedline::ReplLineEditor;
use crate::types::Message;

fn repl_may_scan_workspace_on_bootstrap(cfg: &crate::config::AgentConfig) -> bool {
    (cfg.context_bootstrap_inject.project_profile_inject_enabled
        && cfg
            .context_bootstrap_inject
            .project_profile_inject_max_chars
            > 0)
        || (cfg
            .context_bootstrap_inject
            .project_dependency_brief_inject_enabled
            && cfg
                .context_bootstrap_inject
                .project_dependency_brief_inject_max_chars
                > 0)
        || (cfg.context_bootstrap_inject.agent_memory_file_enabled
            && !cfg
                .context_bootstrap_inject
                .agent_memory_file
                .trim()
                .is_empty())
}

fn repl_spawn_initial_workspace_messages_bg(
    cfg: crate::config::AgentConfig,
    work_dir: PathBuf,
    tui_load: bool,
    agent_role_owned: Option<String>,
    process_handles: Arc<ProcessHandles>,
) -> Arc<StdMutex<Option<Vec<crate::types::Message>>>> {
    let slot: Arc<StdMutex<Option<Vec<crate::types::Message>>>> = Arc::new(StdMutex::new(None));
    let slot_bg = Arc::clone(&slot);
    std::thread::spawn(move || {
        let built = crate::runtime::workspace_session::initial_workspace_messages(
            &cfg,
            work_dir.as_path(),
            tui_load,
            agent_role_owned.as_deref(),
            &process_handles.tool_outcome_recorder,
        );
        let mut guard = slot_bg.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(built);
    });
    slot
}

/// 构建首轮消息（含可选后台扫描）、并打开 `.crabmate/repl_history.txt` 行编辑器。
pub(crate) async fn repl_prepare_messages_and_editor(
    cfg_holder: &SharedAgentConfig,
    tui_load: bool,
    work_dir: &Path,
    agent_role_owned: &Option<String>,
    run_root: &str,
    process_handles: Arc<ProcessHandles>,
) -> Result<
    (
        Vec<Message>,
        Option<Arc<StdMutex<Option<Vec<Message>>>>>,
        Arc<StdMutex<ReplLineEditor>>,
    ),
    Box<dyn std::error::Error>,
> {
    let (messages, initial_pending) = {
        let g = cfg_holder.read().await;
        let recorder = Arc::clone(&process_handles.tool_outcome_recorder);
        let fast = crate::runtime::workspace_session::repl_bootstrap_messages_fast(
            &g,
            agent_role_owned.as_deref(),
            &recorder,
        );
        if !g.session_ui.repl_initial_workspace_messages_enabled {
            (fast, None)
        } else {
            let may_scan_workspace = repl_may_scan_workspace_on_bootstrap(&g) || tui_load;
            if may_scan_workspace {
                let _ = writeln!(
                    io::stderr(),
                    "（后台正在准备工作区首轮上下文或会话恢复，可立即输入；就绪后将并入对话。）"
                );
                let _ = io::stderr().flush();
            }
            let slot = repl_spawn_initial_workspace_messages_bg(
                g.clone(),
                work_dir.to_path_buf(),
                tui_load,
                agent_role_owned.clone(),
                Arc::clone(&process_handles),
            );
            (fast, Some(slot))
        }
    };

    let history_dir = PathBuf::from(run_root).join(".crabmate");
    std::fs::create_dir_all(&history_dir)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let history_file = history_dir.join("repl_history.txt");
    let repl_editor = Arc::new(StdMutex::new(
        ReplLineEditor::new(history_file.as_path())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
    ));
    Ok((messages, initial_pending, repl_editor))
}
