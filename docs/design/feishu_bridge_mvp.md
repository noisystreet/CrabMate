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

## 后续完善方向（路线图）

以下按**建议优先级**排列（与 **`docs/design/web_api_integration.md`** 中「增强方向」互补：该文偏 CrabMate HTTP 契约与多租户；本文偏飞书桥接进程本身）。

### 1. 飞书协议与投递语义（当前最大缺口）

| 项 | 说明 |
|----|------|
| **加密事件体** | 控制台启用 Encrypt Key 后，回调可能仅含 **`encrypt`**；需按飞书文档 **AES 解密** 后再解析 JSON；**`url_verification`** 亦可能为密文，解密后回传 **`challenge`**。 |
| **ACK 与超时** | HTTP 回调须在飞书要求时间内响应；大模型慢时可 **先 200 ACK**，再异步调 CrabMate、异步调「回复/编辑消息」接口（需自建任务队列与重试，并处理飞书限频）。 |
| **消息类型扩展** | 支持 **`post`**、图片、文件等，或统一抽取为纯文本再送入模型；参见 [接收消息内容](https://open.feishu.cn/document/server-docs/im-v1/message/events/message_content)。 |
| **群噪声控制** | 可配置「仅处理 **@ 机器人** 的消息」「忽略 `sender_type=bot`」等，减少无关调用与费用。 |

### 2. 安全与运维

| 项 | 说明 |
|----|------|
| **防重放** | 在已有 **`X-Lark-Signature`** 校验基础上，校验 **`X-Lark-Request-Timestamp`** 窗口；对 **`X-Lark-Request-Nonce`** 做短期去重（与仅 `message_id` 幂等互补）。 |
| **入口加固** | 公网 **HTTPS**；桥接可再加一层 **自定义请求头密钥**（飞书控制台配置）或前置 **API 网关**，避免回调 URL 泄露即被滥用。 |
| **密钥与日志** | 密钥不落盘明文日志；对齐仓库 **`.cursor/rules/secrets-and-logging.mdc`**。 |
| **限流** | 按 **`chat_id`** / 用户维度限流；与 CrabMate **`chat_job_queue`** 并发策略协调，避免打满上游。 |

### 3. 与 CrabMate 行为对齐

| 项 | 说明 |
|----|------|
| **工具与审批** | 非流式 **`POST /chat`** 遇需审批工具时易卡住：生产应 **收窄 `allowed_commands` / `http_fetch_allowed_prefixes`**，或桥接消费 **`POST /chat/stream`** 并将 **`command_approval_request`** 映射为 **飞书卡片交互** 再调 **`POST /chat/approval`**（工作量大，见 **`web_api_integration.md`** §4）。 |
| **流式体验** | 用 **`/chat/stream`** 分段解析 SSE，向飞书 **更新同一条消息** 或 **多条短消息** 推送增量（注意飞书 **5 QPS** 等限制）。 |
| **工作区** | 若需工具读仓库：在可信流程中调用 CrabMate **`POST /workspace`**，且路径落在 **`workspace_allowed_roots`** 内。 |
| **按租户覆盖模型** | 通过请求体 **`client_llm`** 等为不同租户指定网关/模型（密钥勿入日志）。 |

### 4. 多 IM 架构（代码演进）

| 项 | 说明 |
|----|------|
| **trait 边界** | 抽象 **`ImInbound`**（验签、解析、稳定线程 ID）与 **`ImOutbound`**（发消息/编辑），飞书 / 企微 / 钉钉分模块实现。 |
| **Cargo features** | 如 **`feishu`**、**`wework`** 等 optional feature，避免默认 `cargo build` 拉齐所有厂商依赖。 |
| **配置外置** | 由纯环境变量演进为 **TOML/YAML**（多应用、多机器人、路由前缀、各 IM 开关）。 |

### 5. 可观测与测试

| 项 | 说明 |
|----|------|
| **结构化日志** | 关联 **`event_id`**、**`message_id`**、**`conversation_id`**、CrabMate 若将来提供 **`X-Request-Id`** 则一并打印。 |
| **指标** | Prometheus：`events_total`、处理延迟、CrabMate 4xx/5xx、`tenant_access_token` 刷新失败率等。 |
| **自动化测试** | 使用 **wiremock** 等模拟飞书与 CrabMate，覆盖 **`url_verification`**、签名失败、幂等命中等路径。 |

### 6. 发布与交付

| 项 | 说明 |
|----|------|
| **独立制品** | 可选单独 **容器镜像 / `.deb`**，与主 **`crabmate`** 二进制生命周期解耦。 |
| **文档同步** | 行为或环境变量变更时同步本文与 **`web_api_integration.md`**、**`README.md`** 相关段落。 |

## 代码入口

- 库根：`crates/crabmate-im-bridge/src/lib.rs`
- 飞书 HTTP：`crates/crabmate-im-bridge/src/feishu.rs`
- CrabMate 客户端：`crates/crabmate-im-bridge/src/crabmate.rs`
- 二进制与环境变量说明：`crates/crabmate-im-bridge/src/main.rs`
