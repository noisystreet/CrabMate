# 飞书桥接 MVP（`crabmate-im-bridge`）

**状态**：已实现最小可用路径；后续可扩展企微/钉钉等（见 **`docs/design/web_api_integration.md`**）。

## 作用

独立二进制 **`crabmate-im-bridge`**（workspace crate **`crates/crabmate-im-bridge`**）：

1. 监听 **`POST /feishu/events`**，处理飞书 **事件订阅** 回调。
2. **加密体**：若请求 JSON 顶层含 **`encrypt`**（Base64），则使用 **`FEISHU_ENCRYPT_KEY`** 按飞书文档 **AES-256-CBC** 解密后再解析（密钥为 **`SHA256(Encrypt Key 字符串 UTF-8)`**，密文为 **`base64(iv(16) || ciphertext)`**，**PKCS#7** 去填充）。算法与官方一致：[事件解密](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/encrypt-key-encryption-configuration-case?lang=zh-CN)。
3. **`url_verification`**：在解密（若需要）后的 JSON 上读取 **`challenge`**，返回 **`{"challenge":"..."}`**。
4. **`im.message.receive_v1`**：默认 **先入有界内存队列并立即 HTTP 200**（飞书异步 ACK），单 worker 顺序消费：解析 →（可选）**`POST /workspace`**（见 **`FEISHU_WORKSPACE_ROOT_TEMPLATE`**）→ CrabMate **`POST /chat/stream`**（`conversation_id` = `feishu:<chat_id>`，带 **`approval_session_id`** 以支持工具审批，见下文）→ 解析 SSE 累积终答 → **`tenant_access_token`** → [回复消息](https://open.feishu.cn/document/server-docs/im-v1/message/reply)（长文按约 **3500 字符**分段多条回复，**`uuid`** 除首条外使用 **`{message_id}-{序号}`** 去重）。队列满返回 **503**（`FEISHU_EVENT_QUEUE_FULL`）以便飞书重试；可用 **`FEISHU_ASYNC_WORKER=0`** 关闭为同步处理。
5. **安全**：若配置了 **`FEISHU_VERIFICATION_TOKEN`**，则对**除 URL 校验外**的所有事件校验 JSON 内 **`header.token`**（或顶层 **`token`**）与之相等。若已完成 **`X-Lark-Signature`** 验签，则默认校验 **`X-Lark-Request-Timestamp`** 偏差（**`FEISHU_REPLAY_MAX_SKEW_SECS`**，默认 600s）并对 **`X-Lark-Request-Nonce`** 去重（**`FEISHU_NONCE_DEDUP_SECS`**，默认 900s）。群聊可设 **`FEISHU_GROUP_REQUIRE_BOT_MENTION=1`** + **`FEISHU_BOT_OPEN_ID`**，仅处理 **`mentions`** 中含本机器人的消息。
6. **（可选）人工工具审批 HTTP**：在 **`FEISHU_TOOL_APPROVAL_MODE=wait_http`** 且已设置 **`FEISHU_TOOL_DECISION_SECRET`** 时，桥接暴露 **`POST /feishu/tool-decision`**（`Bearer` 或 **`X-API-Key`** 携带该密钥），用于提交 CrabMate **`POST /chat/approval`** 所需的决策。
7. **工具审批交互卡片**：在 **`wait_message` / `wait_http`** 下，桥接会 **`reply` 一条 `msg_type: interactive` 消息**（含三个按钮）。用户点击后飞书推送 **`card.action.trigger`** 至与 **`/feishu/events`** 相同的 URL；桥接解析按钮 **`value`** 并 **`POST /chat/approval`**。须在开发者后台**订阅该事件**并配置**卡片回调地址**。

## 编译与运行

```bash
cargo build -p crabmate-im-bridge --release
# 另一终端先启动 CrabMate serve，并配置好 web_api_bearer_token
export CRABMATE_BASE_URL="http://127.0.0.1:8080"
export CRABMATE_WEB_API_BEARER="YOUR_SHARED_BEARER"
export FEISHU_APP_ID="cli_xxx"
export FEISHU_APP_SECRET="YOUR_APP_SECRET"
# 可选：与飞书「加密策略」中的 Encrypt Key 一致；**加密回调体解密**与 **`X-Lark-Signature`** 签名校验均依赖此值（URL 校验可能无签名头则跳过签名校验）
# export FEISHU_ENCRYPT_KEY="YOUR_ENCRYPT_KEY"
# 可选：与控制台 Verification Token 一致时，校验事件 JSON 的 header.token（URL 校验 challenge 响应亦须带 token）
# export FEISHU_VERIFICATION_TOKEN="..."
# 可选：防重放（仅当本次请求已完成 X-Lark-Signature 验签时生效；0 关闭）
# export FEISHU_REPLAY_MAX_SKEW_SECS=600
# export FEISHU_NONCE_DEDUP_SECS=900
# 可选：群聊仅 @ 机器人时回复（需机器人 open_id）
# export FEISHU_GROUP_REQUIRE_BOT_MENTION=1
# export FEISHU_BOT_OPEN_ID="ou_..."
# 可选：异步 ACK（默认开启）；队列容量（默认 100）
# export FEISHU_ASYNC_WORKER=1
# export FEISHU_EVENT_QUEUE_CAPACITY=100
# 可选：每会话在调用 CrabMate 前设置 Web 工作区；{chat_id} 为飞书 message.chat_id（须落在 CrabMate workspace_allowed_roots）
# export FEISHU_WORKSPACE_ROOT_TEMPLATE="/data/chats/{chat_id}"
# 可选：工具审批模式（默认 wait_message）；wait_http 须同时设置 FEISHU_TOOL_DECISION_SECRET
# export FEISHU_TOOL_APPROVAL_MODE="wait_message"
# export FEISHU_TOOL_DECISION_SECRET="YOUR_RANDOM_SECRET"
# export FEISHU_TOOL_DECISION_TIMEOUT_SECS=600
cargo run -p crabmate-im-bridge
```

默认监听 **`127.0.0.1:9988`**，可用 **`LISTEN_ADDR`** 覆盖。

飞书开发者后台「事件与回调」→ 请求 URL：`https://<公网或穿透域名>/feishu/events`。

## 工作区（与 CrabMate Web 对齐）

若设置 **`FEISHU_WORKSPACE_ROOT_TEMPLATE`**（非空），桥接在每次调用 CrabMate 前会 **`POST /workspace`**，body 为 **`{"path":"<展开后的绝对路径>"}`**，与 Web 侧栏「工作区」一致。模板中可使用 **`{chat_id}`**，在运行时替换为当前消息的 **`message.chat_id`**（单聊、群聊均适用）。

- 展开后的路径须为**已存在目录**，且落在 CrabMate 配置的 **`workspace_allowed_roots`**（未配置时仅允许 **`run_command_working_dir`** 及其子目录）内，否则 CrabMate 返回 **400/403**，桥接会记 **warn** 且本轮不调用模型。
- 同一进程内对**相同展开路径**会跳过重复的 **`POST /workspace`**；不同会话切换目录时会再次调用。

## 用户可见状态与工具审批

- **进度**：桥接解析 CrabMate SSE 中的 **`tool_call`**、**`tool_running`**、**`parsing_tool_calls`**、**`timeline_log`** 等控制帧，并以**后续飞书回复**形式推送短行状态（注意飞书 **QPS** 与消息条数）。
- **工具审批（CrabMate 契约）**：非流式 **`POST /chat`** 不接审批通道；桥接使用 **`POST /chat/stream`**，并为每通用户消息设置 **`approval_session_id = "feishu:<飞书 message_id>"`**，遇敏感工具时由 CrabMate 下发 **`command_approval_request`**，桥接再 **`POST /chat/approval`** 提交决策。
- **模式 `FEISHU_TOOL_APPROVAL_MODE`**（默认 **`wait_message`**）：
  - **`deny_all`**：不传 **`approval_session_id`**；遇审批时桥接会尝试 **`deny`**（等价于敏感工具通常无法执行）。
  - **`default_allow_once`**：收到审批帧后**自动** `allow_once`（**仅可信环境**）。
  - **`wait_http`**：挂起并回复 **交互卡片**（**允许一次 / 永久允许 / 拒绝**）；用户点击按钮触发 **`card.action.trigger`**（须订阅且回调 URL 与 **`/feishu/events`** 一致）。仍可用 **`POST /feishu/tool-decision`**（须 **`FEISHU_TOOL_DECISION_SECRET`**）。超时按 **`deny`**。
  - **`wait_message`**：同上 **交互卡片** + 文字指令 **`!允许一次`** 等。超时同上。

## 飞书侧配置摘要

- 启用**机器人**；订阅 **「接收消息 v2.0」**（`im.message.receive_v1`），见 [接收消息](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive)。使用工具审批**交互卡片**时，另订阅 **`card.action.trigger`**（[卡片回传交互](https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN)），并将 **卡片回调请求地址** 配置为与事件 **`POST /feishu/events`** **相同**的 HTTPS URL。
- 应用须具备发消息相关权限（如 **im:message** / **以应用身份发消息** 等，以控制台为准）。
- 将机器人拉入目标群；群场景通常需 **@ 机器人** 权限（只收 @ 消息时申请对应只读权限即可）。

## 已知限制（MVP）

- **消息类型**：已将多种 **`message_type`** 转为送入模型的**纯文本**（**`text`**、**`post`** 富文本递归提取 **`tag:text`/`title`**、**`image`/`sticker`/`file`/`audio`/`media`** 占位 + key、**`interactive`/`share_*`/其它** 的 `content` JSON 截断摘要；长度 **`FEISHU_MAX_MESSAGE_JSON_CHARS`**）。**不**下载或内联图片/文件/语音/视频二进制。
- **工作区**：模板展开路径须**已存在**且落在 CrabMate **`workspace_allowed_roots`**；桥接**不会** `mkdir`；**`/workspace` 失败时本轮不调用模型**（仅 warn）。
- **工具审批 UX**：**`wait_message` / `wait_http`** 下推送 **交互卡片**（按钮回传 **`card.action.trigger`**）；失败时回退为纯文本。审批后卡片不会自动变灰（可后续用 **`event.token`** 调延时更新接口增强）。
- **幂等**：同一 **`message_id`** 在约 **10 分钟**内去重（防飞书重复推送）。

## 后续完善方向（路线图）

以下按**建议优先级**排列（与 **`docs/design/web_api_integration.md`** 中「增强方向」互补：该文偏 CrabMate HTTP 契约与多租户；本文偏飞书桥接进程本身）。

### 1. 飞书协议与投递语义（当前最大缺口）

| 项 | 说明 |
|----|------|
| **加密事件体** | ~~待实现~~ **已实现**：顶层 **`encrypt`** → **`FEISHU_ENCRYPT_KEY`** + AES-256-CBC + PKCS#7（见上文官方文档链接）。 |
| **ACK 与超时** | **部分缓解**：默认 **先入队再 200**；队列满 **503** 触发重试。仍非持久队列（进程重启丢件）；大并发可前置网关或多实例 + 外部队列（Kafka/Redis）。 |
| **消息类型扩展** | **基础已支持**（见上「已知限制」）；细粒度 **@ 人/链接/at 结构** 与卡片模板语义化解析仍可增强；参见 [接收消息内容](https://open.feishu.cn/document/server-docs/im-v1/message/events/message_content)。 |
| **群噪声控制** | 可配置「仅处理 **@ 机器人** 的消息」「忽略 `sender_type=bot`」等，减少无关调用与费用。 |

### 2. 安全与运维

| 项 | 说明 |
|----|------|
| **防重放** | ~~仅规划~~ **已实现（默认）**：签名校验通过后校验 **`X-Lark-Request-Timestamp`** 窗口与 **`X-Lark-Request-Nonce`** 去重（环境变量可调）；与仅 `message_id` 幂等互补。 |
| **Verification Token** | **已实现（可选）**：配置 **`FEISHU_VERIFICATION_TOKEN`** 后比对事件内 **`header.token`**（明文或解密后）。 |
| **群 @ 过滤** | **已实现（可选）**：**`FEISHU_GROUP_REQUIRE_BOT_MENTION`** + **`FEISHU_BOT_OPEN_ID`**，群聊仅处理 **`mentions`** 中含 `mentioned_type=bot` 且 `open_id` 匹配的消息。 |
| **入口加固** | 公网 **HTTPS**；桥接可再加一层 **自定义请求头密钥**（飞书控制台配置）或前置 **API 网关**，避免回调 URL 泄露即被滥用。 |
| **密钥与日志** | 密钥不落盘明文日志；对齐仓库 **`.cursor/rules/secrets-and-logging.mdc`**。 |
| **限流** | 按 **`chat_id`** / 用户维度限流；与 CrabMate **`chat_job_queue`** 并发策略协调，避免打满上游。 |

### 3. 与 CrabMate 行为对齐

| 项 | 说明 |
|----|------|
| **工具与审批** | **已实现**：**`wait_message` / `wait_http`** 下发 **交互卡片**按钮 + **`card.action.trigger`** 解析；**`POST /chat/stream`** + **`approval_session_id`** + **`POST /chat/approval`**。可增强：点击后 **更新卡片**（`event.token`）、卡片 JSON 2.0 模板化。 |
| **流式体验** | **部分实现**：SSE 解析后推送**多条短回复**（终答分段）；**未**使用飞书「编辑同一条消息」的增量 UI。 |
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
- 飞书审批交互卡片：`crates/crabmate-im-bridge/src/feishu_tool_card.rs`
- 飞书加密体解密：`crates/crabmate-im-bridge/src/feishu_decrypt.rs`
- 飞书消息 content 解析：`crates/crabmate-im-bridge/src/feishu_message_content.rs`
- 飞书工作区模板：`crates/crabmate-im-bridge/src/feishu_workspace.rs`
- CrabMate 客户端：`crates/crabmate-im-bridge/src/crabmate.rs`
- 二进制与环境变量说明：`crates/crabmate-im-bridge/src/main.rs`
