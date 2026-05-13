//! 飞书开放平台 **事件订阅（HTTP Webhook）**：
//! - **明文**：直接解析 JSON。
//! - **加密体**（顶层 **`encrypt`**）：按飞书文档 **AES-256-CBC** 解密后再解析（需配置 **`FEISHU_ENCRYPT_KEY`**）。
//! - `url_verification`：返回 **`{"challenge":"..."}`**。
//! - `im.message.receive_v1`（文本）：默认 **先入队再立即 HTTP 200**（异步 ACK），后台 worker 调 **`POST /chat/stream`** 并回复飞书；可关闭为同步处理（见配置）。队列可选 **内存 `mpsc`** 或 **`FEISHU_EVENT_QUEUE_SQLITE`** 的 **SQLite 持久化**（进程重启不丢）。若配置 **`FEISHU_WORKSPACE_ROOT_TEMPLATE`**，则在调用 CrabMate 前 **`POST /workspace`** 对齐工作区（支持 **`{chat_id}`** 占位）。可选 **`FEISHU_IN_PLACE_PROGRESS_CARD`**：先发可 **PATCH** 的占位交互卡片，结束时 **`PATCH /im/v1/messages/:message_id`** 原地更新为结果摘要。
//! - **工具审批**：通过 **`approval_session_id`** 走 CrabMate **`/chat/stream`** + **`POST /chat/approval`**；**`wait_message` / `wait_http`** 下会回复含按钮的 **交互卡片**（`msg_type: interactive`）。须在飞书开发者后台订阅 **`card.action.trigger`**，且 **卡片回调 URL** 与事件 **`POST /feishu/events`** 使用同一地址；可选 **`FEISHU_TOOL_APPROVAL_MODE=wait_http`** 时仍支持 **`POST /feishu/tool-decision`**。
//!
//! 签名校验（可选）：若配置了 **Encrypt Key**，且请求带 **`X-Lark-Signature`** 等头，则按飞书文档
//! `SHA256(timestamp + nonce + encrypt_key + body)` 十六进制小写比对（**原始 HTTP body 字符串**；**URL 校验请求可能无签名头**，此时跳过校验）。
//!
//! 签名校验通过后可选 **防重放**：校验 **`X-Lark-Request-Timestamp`** 与服务器时间偏差，并对 **`X-Lark-Request-Nonce`** 做短期去重（见配置项）。
//!
//! 实现拆分为 **`feishu_parts/router_state.rs`**（配置、状态、路由与队列 worker）与 **`feishu_parts/handlers_*.inc.rs`**（事件处理与 CrabMate 调用，按依赖顺序 `include!`），以降低单文件行数。

include!("feishu_parts/router_state.rs");
include!("feishu_parts/handlers_util.inc.rs");
include!("feishu_parts/handlers_reply.inc.rs");
include!("feishu_parts/handlers_tool_approval.inc.rs");
include!("feishu_parts/handlers_flow.inc.rs");
include!("feishu_parts/handlers_tests.inc.rs");
