# Web API 第三方集成：架构备忘与增强方向

**状态**：设计备忘 / 路线图（**未**承诺实现顺序与版本日期）。**受众**：实现飞书、企业微信、钉钉等 **IM 桥接**、自动化流水线、或「OpenCode 式」多入口共用后端的维护者。  
**语言**：中文。  
**关联**：

- 路由与契约：**`docs/命令行与路由.md`**（`POST /chat`、`POST /chat/stream`、`GET /openapi.json` 等）
- SSE 控制面：**`docs/SSE协议.md`**、`crates/crabmate-sse-protocol`
- Web 鉴权实现：**`src/web/chat_handlers/auth.rs`**、`src/web/server.rs`
- CLI / Web 审批与会话差异：**`docs/命令行与路由.md`**「CLI 与 Web 能力对照」
- 路线图交叉：**`docs/待办清单.md`**（连接器生态、MCP 扩展等）

---

## 1. 目标与边界

### 1.1 本文讨论什么

- 外部系统**仅通过 HTTP** 调用 CrabMate（`serve` 暴露的 Axum 路由），不 fork 进程、不嵌入 `run_agent_turn` 库 API 时的集成模式。
- 与 **IM 机器人**、工单系统、CI Webhook 等「无浏览器 UI」客户端对接时的**缺口与可选增强**。

### 1.2 本文不讨论什么

- **前端 Leptos** 的交互与 `localStorage`（见 **`frontend-leptos/src/api/client_llm_storage.rs`**）。
- **MCP stdio** 客户端/服务端细节（见 **`docs/开发文档.md`** → `mcp/mod.rs`）；MCP 可作为「工具扩展」与 HTTP 集成**并行**存在，但不替代 IM 回调 → CrabMate 的主路径。
- 各 IM 厂商的具体签名算法与事件字段（由**桥接服务**实现）。

---

## 2. 推荐参考架构

```
┌─────────────┐     HTTPS      ┌──────────────────┐   Bearer / X-API-Key   ┌─────────────┐
│ 飞书/企微等  │ ──────────────► │ 桥接服务（自建）  │ ───────────────────► │ crabmate    │
│ 事件回调     │   验签/去重     │ 映射消息体        │   POST /chat 或       │ serve        │
└─────────────┘                 │ 会话 id 策略      │   POST /chat/stream   └─────────────┘
                                └──────────────────┘
```

**原则**：

1. **验签、幂等、限流、卡片渲染** 放在桥接层；CrabMate 保持「对话引擎 + 工具 + 工作区策略」。
2. CrabMate 进程应部署在**可信网络**；`web_api_require_bearer` 与 **`CM_WEB_API_BEARER_TOKEN`** / **`web_api_bearer_token`** 为默认安全基线（见 **`README.md`**、**`docs/配置说明.md`**）。
3. 使用 **`conversation_id`**（及可选 SQLite 会话存储）将同一 IM 线程映射到稳定会话；**`GET /conversation/messages`** 可用于桥接侧恢复或对账。

---

## 3. 当前能力基线（集成方可直接使用）

| 能力 | 说明 |
|------|------|
| **OpenAPI** | **`GET /openapi.json`**，便于生成客户端与契约测试。 |
| **非流式 JSON** | **`POST /chat`**：适合短问答、脚本聚合；需自行从响应 JSON 提取助手最终文本。 |
| **SSE 流式** | **`POST /chat/stream`**：`text/event-stream`、事件 **`id:`**、**`Last-Event-ID`** + 请求体 **`stream_resume`**、响应头 **`x-conversation-id`** / **`x-stream-job-id`**；控制面与正文区分见 **`docs/SSE协议.md`**。 |
| **会话与分叉** | **`conversation_id`**、**`POST /chat/branch`**、**`GET /conversation/messages`**（含 **`revision`**）。 |
| **审批** | SSE **`command_approval_request`** + **`POST /chat/approval`**；非流式 **`/chat`** 队列路径**未**挂载 `WebToolRuntime`，**不宜**作为需审批工具的集成入口。IM 桥接（**`crabmate-im-bridge`**）已改为消费 **`/chat/stream`** 并转发审批。 |
| **热重载** | **`POST /config/reload`**；**`web_api_bearer_token` 中间件是否生效**仍以 **`serve` 重启**为准（见配置重载说明）。 |
| **健康检查** | **`GET /health`**。 |

---

## 4. 已知约束（集成设计必须消化）

1. **SSE 断线重连缓冲为单进程内存**：进程重启或任务结束后重连可能收到 **410**（**`STREAM_JOB_GONE`**）。多副本负载均衡时，若无会话粘性，集成方须自行接受「重连仅同实例有效」或只用单实例承接流式任务。
2. **单一共享 Bearer 密钥**为常见部署形态：多租户、多 IM 机器人共用同一 CrabMate 时，**租户隔离**宜放在桥接或 API 网关（见第 5 节）。
3. **`client_llm` 请求体覆盖**：自动化场景可用 **`client_llm.api_key` / `api_base` / `model`** 做按次覆盖；须遵守日志与响应脱敏规则（**`.cursor/rules/secrets-and-logging.mdc`**）。
4. **工具审批无原生 IM UI**：桥接层须将审批映射为卡片交互、或预先收紧 **`allowed_commands`** / **`http_fetch_allowed_prefixes`** / 禁用工具，避免无人值守时卡死。

---

## 5. 建议的增强方向（按主题，不绑定实现顺序）

### 5.1 协议与「非浏览器」客户端体验

- **异步任务 + 回调或轮询**：IM 常先快速 ACK，再异步推送结果。可选方向：任务 ID + **`GET /chat/jobs/{id}`** 类轮询，或完成时 **Webhook POST** 到集成方 URL（需签名与超时策略）。
- **稳定「仅正文」响应**：为 **`POST /chat`** 提供可选 **`response_shape`**（例如仅返回 **`assistant_text`** + **`usage`**），减少集成方对内部 JSON 嵌套路径的耦合。
- **官方最小示例**：在仓库 **`examples/`** 或文档中提供 **curl / Python** 消费 SSE 的片段（拼接同块多行 `data:`、识别控制面 **`code`**、**`stream_ended`**）。

### 5.2 安全与多租户

- **多 API Key**：支持多枚 **`web_api_bearer_token`** 或网关下发 **`X-API-Key`** 与租户元数据映射；或与 **OAuth2 / mTLS** 在反向代理层统一。
- **密钥轮换窗口**：双密钥并行、文档化 **`serve` 重启** 与网关切换顺序。
- **若增加原生 Webhook 入口**：必须带**平台验签**、**时间戳防重放**、**body 大小上限**，避免未认证流量驱动模型。

### 5.3 可靠性与运维

- **幂等键**：请求头 **`Idempotency-Key`** 或与 IM **`message_id`** 绑定的去重，避免平台重试导致重复计费与重复回复。
- **限流**：按 Key、**`conversation_id`**、IP 的配额，与现有 **`chat_job_queue`** 并发策略在文档中对齐说明。
- **可观测性**：响应头 **`X-Request-Id`**（或等价）贯穿 access log 与 **`tracing`**，便于与 IM 侧 trace 对齐。

### 5.4 与 IM 产品形态贴合（主要在桥接层）

- 长文本拆分、Markdown → 卡片字段、typing 状态等多为**各平台 API 差异**，优先在桥接实现；CrabMate 侧保持 UTF-8 文本与 SSE 契约稳定即可。

### 5.5 用量与计费

- 若响应中尚未稳定暴露 **token 用量**，集成方难以做配额与账单；可对齐 OpenAI 兼容 **`usage`** 字段或单独扩展 JSON（变更时同步 **OpenAPI** 与 **`docs/SSE协议.md`** 若涉及流上字段）。

---

## 6. 落地时的文档与协议同步清单

凡新增或变更 HTTP 路由、请求/响应体、SSE 控制面键、错误码：

- 更新 **`src/web/openapi/`**、**`docs/命令行与路由.md`**、相关 **`docs/配置说明.md`** / **`README.md`** 使用者段落；
- SSE 变更遵循 **`.cursor/rules/api-sse-chat-protocol.mdc`**（含 **`golden_sse_control`** 等）。

---

## 7. 小结

- **短期**：桥接服务 + 现有 **`/chat` / `/chat/stream` + Bearer + `conversation_id`** 即可闭环绝大多数 IM 场景；幂等、验签、卡片在桥接完成。
- **仓库内参考实现**：workspace crate **`crates/crabmate-im-bridge`**（二进制 **`crabmate-im-bridge`**）提供飞书 MVP；使用说明、已知限制与**后续完善路线图**见 **`docs/design/feishu_bridge_mvp.md`**（「后续完善方向」一节）。
- **中长期**：若在仓库内减少桥接样板代码、支撑多租户与水平扩展，再按第 5 节逐项立项，并同步 OpenAPI 与 SSE 单一事实来源。
