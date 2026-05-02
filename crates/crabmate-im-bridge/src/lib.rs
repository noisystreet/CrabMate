//! IM 桥接库：当前内置 **飞书** Webhook MVP（事件订阅 HTTP 回调 → CrabMate → 回复消息）。
//!
//! 二进制入口见 **`crabmate-im-bridge`**（`src/main.rs`）。设计背景见仓库根目录
//! **`docs/design/web_api_integration.md`**。

pub mod crabmate;
pub mod feishu;
mod feishu_decrypt;
mod feishu_message_content;
mod feishu_workspace;
mod sse_consumer;

pub use crabmate::CrabmateClient;
pub use feishu::{FeishuBridgeConfig, FeishuBridgeState, FeishuToolApprovalMode, build_router};
