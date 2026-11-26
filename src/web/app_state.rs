//! Web 服务进程内状态：HTTP 客户端、工作区覆盖、上传目录、队列句柄等（自 `lib.rs` 下沉）。
//!
//! 与队列 / 会话落盘**共用**的中立片段（`AppStateConversationRuntime`、`WebChatJobAppFacet`、
//! `ApprovalSessionSlot`）已下沉到 [`crate::context::web_state_facet`]（#1510 P0）：它们留在
//! 本模块会让 `chat_job_queue` 反向 `use crate::web::…` 去拿类型，形成 `chat_job_queue ↔ web`
//! 真回边。下面用 `pub(crate) use` 转发原名，Web 侧既有调用点零涟漪。

use std::collections::HashMap;
use std::sync::Arc;

use crate::chat_job_queue::{ChatJobQueue, WebChatQueueDeps};
use crate::config::SharedAgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::sse::SseStreamHub;

pub(crate) use crate::cm_session_store::{ConversationBacking, open_conversation_sqlite};

/// 与 `chat_handlers::normalize_client_conversation_id` 及存储上限对齐。
pub(crate) use crate::conversation_store::CONVERSATION_ID_MAX_LEN;

pub(crate) use crate::cm_session_store::ConversationTurnSeed;

// P0（#1510）：以下名字的定义点在 `context::web_state_facet`，此处只转发，
// 保持 `web::app_state::…` 与 `web::…` 的既有导入路径不变。
// **新代码请直接引 `crate::context::web_state_facet::…`**：这里的转发名只为兼容既有调用点，
// 不再新增使用者 —— 否则又长回 #1510 P1 要治的「同模块双名」。
pub(crate) use crate::context::web_state_facet::{
    APPROVAL_SESSION_TTL, AppStateConversationRuntime, ApprovalSessionSlot, WebChatJobAppFacet,
    purge_expired_approval_sessions,
};

/// HTTP 客户端、共享配置快照与工作区覆盖（与队列 / 会话后端解耦）。
///
/// 根包保留本类型以满足 axum `FromRef` 孤儿规则（**不**在 `cm_web_host` 镜像同名类型）。
#[derive(Clone)]
pub(crate) struct AppStateHttpCore {
    pub(crate) cfg: SharedAgentConfig,
    /// 与启动时 `--config` / 默认探测一致，供 **`POST /config/reload`** 调用 [`load_config`]。
    pub(crate) config_path_for_reload: Option<String>,
    /// 进程级 Bearer 密钥快照（`Arc` 便于 handler facet 廉价共享，勿整串 `Clone`）。
    pub(crate) api_key: Arc<str>,
    pub(crate) client: reqwest::Client,
    pub(crate) tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.command_exec.run_command_working_dir
    pub(crate) workspace_override: Arc<tokio::sync::RwLock<Option<String>>>,
    pub(crate) uploads_dir: std::path::PathBuf,
    /// `GET /workspace/git` 的只读投影缓存（TTL + single-flight，见 `web::workspace::git_projection`）。
    pub(crate) workspace_git_cache: Arc<WorkspaceGitCache>,
    /// `#1455` C 步：自动续跑账本（见 [`WebChatJobAppFacet::run_epoch`] 的说明）。
    pub(crate) run_epoch: Arc<std::sync::Mutex<crate::cm_agent::run_epoch::RunEpoch>>,
}

/// 由工作区覆盖 + 共享配置解析当前有效工作区根（空串表示未设置）。
pub(crate) async fn effective_workspace_path_from_override(
    workspace_override: &tokio::sync::RwLock<Option<String>>,
    cfg: &SharedAgentConfig,
) -> String {
    let guard = workspace_override.read().await;
    match guard.as_deref() {
        None => String::new(),
        Some(s) if s.trim().is_empty() => {
            let cfg = cfg.read().await;
            cfg.command_exec.run_command_working_dir.clone()
        }
        Some(s) => s.to_string(),
    }
}

/// 前端是否已经设置过明确工作区路径（`Some(non-empty)`）。
pub(crate) async fn workspace_is_set_from_override(
    workspace_override: &tokio::sync::RwLock<Option<String>>,
) -> bool {
    let guard = workspace_override.read().await;
    guard.as_deref().is_some_and(|s| !s.trim().is_empty())
}

impl AppStateHttpCore {
    /// 当前 Web 会话选中的工作区根路径（**未**调用 `POST /workspace` 成功设置前返回空串）。
    pub(crate) async fn effective_workspace_path(&self) -> String {
        effective_workspace_path_from_override(&self.workspace_override, &self.cfg).await
    }
}

/// `/chat` / `/chat/stream` 进程内队列及其 worker 依赖。
#[derive(Clone)]
pub(crate) struct AppStateChatRuntime {
    pub(crate) chat_queue: ChatJobQueue,
    pub(crate) chat_queue_job_deps: Arc<WebChatQueueDeps>,
}

/// `GET /workspace/git` 的缓存条目。
///
/// **单条即可**：一实例一工作区（#1183），切换工作区后 `workspace_root` 不匹配自然失效。
///
/// **拆成「骨架 + `Arc` 清单」（#1507）**：条目原先存整份 `WorkspaceGitStatusResponse`，
/// 每次命中都深拷贝一次；引入 `?files` 之后那次拷贝就是「最多 500 条 `String` 路径」，
/// 而侧栏 2 s 轮询的绝大多数请求根本不带列表，拷完立刻被丢弃。拆开后：
/// 无参命中只克隆骨架（几个小 `String`），清单按需从 `Arc` 复制。
#[derive(Clone)]
pub(crate) struct CachedWorkspaceGitStatus {
    pub(crate) workspace_root: String,
    /// 单调钟，用于 TTL；命中缓存时**不**推进 `skeleton.computed_at_unix`。
    pub(crate) computed_at: std::time::Instant,
    /// 出站响应的「骨架」：除 `files` 之外的一切，`files` **恒为 `None`**。
    pub(crate) skeleton: crate::cm_api_contract::workspace::WorkspaceGitStatusResponse,
    /// 文件清单（探测写缓存时**搬进来**，不复制）。不可用态为 `None`。
    pub(crate) files:
        Option<std::sync::Arc<crate::cm_api_contract::workspace::WorkspaceGitFileList>>,
}

/// git 投影的缓存与探针锁。
#[derive(Default)]
pub(crate) struct WorkspaceGitCache {
    pub(crate) entry: std::sync::Mutex<Option<CachedWorkspaceGitStatus>>,
    /// 探测串行化：TTL 到期瞬间的并发请求只实际探测一次（最小 single-flight）。
    pub(crate) probe_lock: tokio::sync::Mutex<()>,
}

/// `GET /status` tiktoken 新会话基线的缓存条目。
///
/// 命中条件：`cfg_fingerprint`（全量 `AgentConfig` Debug 哈希）与 `workspace_root` 一致
/// 且未超过 TTL（见 `health_status::BASELINE_CACHE_TTL`）；工作区文件内容变化不主动失效。
#[derive(Clone)]
pub(crate) struct CachedTiktokenBaseline {
    pub(crate) cfg_fingerprint: u64,
    pub(crate) workspace_root: String,
    pub(crate) computed_at: std::time::Instant,
    pub(crate) baseline_by_role: std::collections::BTreeMap<String, u32>,
}

/// 审批表、任务侧栏、SSE hub、异步作业等 Web 辅助状态。
#[derive(Clone)]
pub(crate) struct AppStateWebAux {
    pub(crate) approval_sessions: Arc<tokio::sync::RwLock<HashMap<String, ApprovalSessionSlot>>>,
    pub(crate) long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub(crate) llm_models_health_cache:
        Arc<std::sync::Mutex<Option<crate::health::CachedLlmModelsHealthProbe>>>,
    /// `GET /status` tiktoken 新会话基线缓存（大工作区同步分词可达数十秒，见 `health_status`）。
    pub(crate) tiktoken_baseline_cache: Arc<std::sync::Mutex<Option<CachedTiktokenBaseline>>>,
    pub(crate) sse_stream_hub: Arc<SseStreamHub>,
    pub(crate) process_handles: Arc<crate::process_handles::ProcessHandles>,
    pub(crate) async_chat_jobs: super::async_chat_job::AsyncChatJobsMap,
    /// 后台任务注册表（`run_command` 的 `async=true`）；启动时按 `[tool_registry]` 配置构建。
    pub(crate) tool_job_registry: std::sync::Arc<crate::jobs::tool_jobs::ToolJobRegistry>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) http: AppStateHttpCore,
    pub(crate) chat: AppStateChatRuntime,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) aux: AppStateWebAux,
}
