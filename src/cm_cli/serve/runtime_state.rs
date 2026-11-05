//! `serve` 运行时状态（`Arc<AppState>`）装配，以及两个后台定时器。

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use crate::config;
use crate::web;

use super::paths::ServeResolvedPaths;

pub(super) struct ServeRuntimeBuildInput<'a> {
    pub(super) cfg_holder: &'a config::SharedAgentConfig,
    pub(super) config_path: &'a Option<String>,
    pub(super) client: reqwest::Client,
    pub(super) tools: Vec<crate::types::Tool>,
    pub(super) api_key: String,
    pub(super) initial_workspace: Option<String>,
    pub(super) process_handles: Arc<crate::process_handles::ProcessHandles>,
    pub(super) paths: ServeResolvedPaths,
}

pub(super) async fn build(
    input: ServeRuntimeBuildInput<'_>,
) -> Result<Arc<crate::AppState>, Box<dyn std::error::Error>> {
    let ServeRuntimeBuildInput {
        cfg_holder,
        config_path,
        client,
        tools,
        api_key,
        initial_workspace,
        process_handles,
        paths,
    } = input;
    let (cq_conc, cq_pending) = {
        let g = cfg_holder.read().await;
        (
            g.chat_queues_cache.chat_queue_max_concurrent,
            g.chat_queues_cache.chat_queue_max_pending,
        )
    };
    std::fs::create_dir_all(&paths.uploads_dir).ok();
    {
        let mut g = cfg_holder.write().await;
        g.chat_uploads_dir = Some(paths.uploads_dir.clone());
        g.chat_workspace_root = Some(paths.workspace_root.clone());
    }
    let chat_queue = crate::chat_job_queue::ChatJobQueue::new(cq_conc, cq_pending);
    let conversation_backing =
        super::store::conversation_backing_from_sqlite_path(paths.conversation_sqlite.trim())?;
    let long_term_memory = super::store::serve_long_term_memory_runtime(
        paths.long_term_memory_enabled,
        &conversation_backing,
        paths.long_term_memory_sqlite.trim(),
    );
    let sse_stream_hub = std::sync::Arc::new(crate::sse::SseStreamHub::new());
    // 注册表本身 + TTL 清理定时器（终态过期 → 轮询 410）；门面在根包，见模块文档。
    let tool_job_registry = {
        let g = cfg_holder.read().await;
        crate::build_serve_tool_job_registry(&g)
    };
    let api_key: Arc<str> = Arc::from(api_key);
    let chat_queue_job_deps = std::sync::Arc::new(crate::chat_job_queue::WebChatQueueDeps {
        cfg: Arc::clone(cfg_holder),
        api_key: api_key.to_string(),
        client: client.clone(),
        tools: tools.clone(),
        chat_queue: chat_queue.clone(),
        long_term_memory: long_term_memory.clone(),
        sse_stream_hub: Arc::clone(&sse_stream_hub),
        llm_backend: None,
        turn_runner: crate::default_turn_engine(),
    });
    Ok(Arc::new(crate::AppState {
        http: web::AppStateHttpCore {
            cfg: Arc::clone(cfg_holder),
            config_path_for_reload: config_path.clone(),
            api_key,
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: paths.uploads_dir,
        },
        chat: web::AppStateChatRuntime {
            chat_queue,
            chat_queue_job_deps,
        },
        conversation: web::AppStateConversationRuntime {
            conversation_backing: std::sync::Arc::new(tokio::sync::RwLock::new(
                conversation_backing,
            )),
            conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
            cfg: std::sync::Arc::clone(cfg_holder),
        },
        aux: web::AppStateWebAux {
            approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            long_term_memory,
            llm_models_health_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tiktoken_baseline_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            sse_stream_hub,
            process_handles: Arc::clone(&process_handles),
            async_chat_jobs: std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            tool_job_registry,
        },
    }))
}

pub(super) fn spawn_uploads_cleanup(state: Arc<crate::AppState>) {
    // uploads 自动清理：每 10 分钟执行一次；保留 24h；总容量上限 500MB；会话仍引用的文件跳过
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(600));
        loop {
            interval.tick().await;
            let dir = {
                let g = state.http.cfg.read().await;
                g.chat_uploads_dir.clone()
            };
            let Some(dir) = dir else {
                continue;
            };
            let referenced = state.conversation.referenced_upload_filenames().await;
            web::cleanup_uploads_dir(
                dir,
                Duration::from_secs(24 * 3600),
                500 * 1024 * 1024,
                &referenced,
            )
            .await;
        }
    });
}

/// 会话保留策略的兜底清理：启动时立即执行一次，此后每小时一次；TTL / 条数上限取自配置（可热更）。
pub(super) fn spawn_conversation_prune(state: Arc<crate::AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state.conversation.prune_conversations_now().await;
        }
    });
}
