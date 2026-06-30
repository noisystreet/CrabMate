//! 将进程内 **`ChatCompletionsBackend`** 引用包装为可在 `tokio::spawn` 间共享的 [`Arc`]。
//!
//! 调用方须保证被包装的后端在返回的 [`Arc`] 存活期间一直有效（单轮 `run_agent_turn` 作用域内满足）。

use std::sync::Arc;

use async_trait::async_trait;

use crabmate_types::{ChatRequest, Message};

use super::backend::ChatCompletionsBackend;
use super::chat_params::StreamChatParams;

/// 经原始指针持有后端；[`shared_chat_backend`] 的调用方保证所指对象在 [`Arc`] 存活期间有效。
struct SharedChatBackendPtr {
    ptr: *const dyn ChatCompletionsBackend,
}

// SAFETY: 与 `run_agent_turn` / 分层子任务相同契约——后端在整轮编排结束前保持有效。
unsafe impl Send for SharedChatBackendPtr {}
unsafe impl Sync for SharedChatBackendPtr {}

#[async_trait]
impl ChatCompletionsBackend for SharedChatBackendPtr {
    async fn stream_chat(
        &self,
        params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        // SAFETY: `shared_chat_backend` 调用方保证 `ptr` 在 `self` 存活期间指向有效后端。
        let backend = unsafe { &*self.ptr };
        backend.stream_chat(params, req).await
    }
}

/// 把后端引用包装为可在并行分层子任务间共享的 [`Arc`]。
///
/// `backend` 须在返回的 [`Arc`] 被 drop 之前一直有效（单轮 `run_agent_turn` 内成立）。
pub fn shared_chat_backend(
    backend: &(dyn ChatCompletionsBackend + 'static),
) -> Arc<dyn ChatCompletionsBackend> {
    Arc::new(SharedChatBackendPtr {
        ptr: backend as *const dyn ChatCompletionsBackend,
    })
}

/// 与 [`shared_chat_backend`] 相同；保留旧名以免调用点大面积重命名。
pub fn shared_static_chat_backend(
    backend: &(dyn ChatCompletionsBackend + 'static),
) -> Arc<dyn ChatCompletionsBackend> {
    shared_chat_backend(backend)
}
