# 飞书桥接 MVP（`crabmate-im-bridge`）

**状态**：已实现最小可用路径；后续可扩展企微/钉钉等（见 **`docs/design/web_api_integration.md`**）。

## 作用

独立二进制 **`crabmate-im-bridge`**（workspace crate **`crates/crabmate-im-bridge`**）：

1. 监听 **`POST /feishu/events`**，处理飞书 **事件订阅** 回调。
2. **`url_verification`**：返回 JSON **`{"challenge":"..."}`**（与飞书控制台配置回调 URL 时的校验一致）。
3. **`im.message.receive_v1`**（文本）：解析 `event.message.content` 内 JSON → 调 CrabMate **`POST /chat`**（`message` + **`conversation_id`** = `feishu:<chat_id>`）→ 使用 **`tenant_access_token`** 调用飞书 **[回复消息](https://open.feishu.cn/document/server-docs/im-v1/message/reply)**。

## 编译与运行

```bash
cargo build -p crabmate-im-bridge --release
# 另一终端先启动 CrabMate serve，并配置好 web_api_bearer_token
export CRABMATE_BASE_URL="http://127.0.0.1:8080"
export CRABMATE_WEB_API_BEARER="YOUR_SHARED_BEARER"
export FEISHU_APP_ID="cli_xxx"
export FEISHU_APP_SECRET="YOUR_APP_SECRET"
# 可选：与飞书事件订阅 Encrypt Key 一致，用于校验 X-Lark-Signature（URL 校验请求可能无签名头，桥接会跳过）
# export FEISHU_ENCRYPT_KEY="YOUR_ENCRYPT_KEY"
export RUST_LOG=info
cargo run -p crabmate-im-bridge
```

默认监听 **`127.0.0.1:9988`**，可用 **`LISTEN_ADDR`** 覆盖。

飞书开发者后台「事件与回调」→ 请求 URL：`https://<公网或穿透域名>/feishu/events`。

## 飞书侧配置摘要

- 启用**机器人**；订阅 **「接收消息 v2.0」**（`im.message.receive_v1`），见 [接收消息](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive)。
- 应用须具备发消息相关权限（如 **im:message** / **以应用身份发消息** 等，以控制台为准）。
- 将机器人拉入目标群；群场景通常需 **@ 机器人** 权限（只收 @ 消息时申请对应只读权限即可）。

## 已知限制（MVP）

- **仅文本** `message_type == text`；其它类型会回复一条说明文本。
- **加密事件体**（仅 `encrypt` 字段、无明文 JSON）：本 MVP **未实现解密**，请在飞书侧使用可解析的明文 challenge/事件体，或后续扩展解密逻辑（见飞书「事件订阅」文档）。
- **工具审批**：若 CrabMate 配置启用了需审批的工具，IM 侧无自动审批；生产环境应收窄 **`allowed_commands`** 或对 `/chat/stream` 做卡片化审批（见 **`web_api_integration.md`** §4）。
- **幂等**：同一 **`message_id`** 在约 **10 分钟**内去重（防飞书重复推送）。

## 代码入口

- 库根：`crates/crabmate-im-bridge/src/lib.rs`
- 飞书 HTTP：`crates/crabmate-im-bridge/src/feishu.rs`
- CrabMate 客户端：`crates/crabmate-im-bridge/src/crabmate.rs`
- 二进制与环境变量说明：`crates/crabmate-im-bridge/src/main.rs`
