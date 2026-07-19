//! 测试用 Web 服务器启动器（仅在 `feature = "web"` 时编译）。
//!
//! 集成测试通过 [`start_test_serve`] 快速启动一个随机端口的 axum 实例，避免重复
//! 构造 [`AppState`] 的样板代码。支持注入自定义 LLM 后端（e2e 录制/回放）。

#![cfg(feature = "web")]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use tokio::sync::oneshot;

use crate::chat_job_queue::{ChatJobQueue, WebChatQueueDeps};
use crate::config::SharedAgentConfig;
use crate::http_client::build_shared_api_client;
use crate::llm::ChatCompletionsBackend;
use crate::process_handles::ProcessHandles;
use crate::sse::SseStreamHub;
use crate::web::{self, AppState};
use crate::web_static_dir::resolve_web_static_dir;

/// 测试用服务器句柄。
pub struct TestServeHandle {
    pub base_url: String,
    /// 持有后端引用以防过早 drop（`&'static` 引用安全，但保留引用可验证所有权的逻辑关联）。
    #[allow(dead_code)]
    pub(crate) llm_backend: Option<&'static (dyn ChatCompletionsBackend + 'static)>,
    pub(crate) _shutdown_tx: oneshot::Sender<()>,
}

/// 启动测试用 axum Web 服务器（随机端口），返回 [`TestServeHandle`]。
///
/// `llm_backend` 为 e2e 录制/回放注入的后端；`None` 使用默认 HTTP 后端。
///
/// 构建最小化 `AppState` + `build_app`，跳过定时任务与鉴权中间件。
/// 调用方可在测试结束时 drop `handle`（触发 graceful shutdown）。
///
/// # Panics
///
/// 若加载默认配置 / 绑定端口失败则 panic（测试应快速失败）。
pub async fn start_test_serve(
    llm_backend: Option<&'static (dyn ChatCompletionsBackend + 'static)>,
) -> TestServeHandle {
    let cfg = crate::config::load_config(None).expect("默认配置加载失败");
    let cfg_holder: SharedAgentConfig = Arc::new(tokio::sync::RwLock::new(cfg));
    let client = build_shared_api_client(&cfg_holder.read().await.llm_http_retry)
        .expect("构建 HTTP 客户端失败");
    let tools = crate::build_tools();
    let api_key = std::env::var("API_KEY").unwrap_or_default();
    let uploads_dir = std::env::temp_dir().join("crabmate_e2e_uploads");
    let _ = std::fs::create_dir_all(&uploads_dir);

    let (cq_conc, cq_pending) = {
        let g = cfg_holder.read().await;
        (
            g.chat_queues_cache.chat_queue_max_concurrent,
            g.chat_queues_cache.chat_queue_max_pending,
        )
    };
    let chat_queue = ChatJobQueue::new(cq_conc, cq_pending);

    let sse_stream_hub = Arc::new(SseStreamHub::new());
    let chat_queue_job_deps = Arc::new(WebChatQueueDeps {
        cfg: Arc::clone(&cfg_holder),
        api_key: api_key.clone(),
        client: client.clone(),
        tools: tools.clone(),
        chat_queue: chat_queue.clone(),
        long_term_memory: None,
        sse_stream_hub: Arc::clone(&sse_stream_hub),
        llm_backend,
    });

    let state = Arc::new(AppState {
        http: web::AppStateHttpCore {
            cfg: Arc::clone(&cfg_holder),
            config_path_for_reload: None,
            api_key,
            client,
            tools,
            workspace_override: Arc::new(tokio::sync::RwLock::new(None)),
            uploads_dir: uploads_dir.clone(),
        },
        chat: web::AppStateChatRuntime {
            chat_queue,
            chat_queue_job_deps,
        },
        conversation: web::AppStateConversationRuntime {
            conversation_backing: Arc::new(tokio::sync::RwLock::new(
                web::ConversationBacking::memory_default(),
            )),
            conversation_id_counter: Arc::new(AtomicU64::new(1)),
        },
        aux: web::AppStateWebAux {
            approval_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            long_term_memory: None,
            llm_models_health_cache: Arc::new(std::sync::Mutex::new(None)),
            sse_stream_hub,
            process_handles: ProcessHandles::default_arc_process_handles(),
            async_chat_jobs: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        },
    });

    let app = web::server::build_app(
        state,
        false,
        resolve_web_static_dir(),
        uploads_dir,
        false, /* web_api_bearer_layer_enabled */
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口失败");
    let addr = listener.local_addr().expect("获取监听地址失败");
    let base_url = format!("http://{}", addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
        .ok();
    });

    TestServeHandle {
        base_url,
        llm_backend,
        _shutdown_tx: shutdown_tx,
    }
}
