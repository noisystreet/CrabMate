**语言 / Languages:** 中文（本页）· [English](en/CLI_CONTRACT.md)

# CLI 契约参考（退出码、JSON、`chat` 输出）

面向脚本与 CI：与 `src/runtime/cli_exit.rs`、`src/config/cli.rs`（clap）及 `crabmate --help` 中的 **after_help** 对齐。流式 **Web** 侧错误码见 **[`docs/SSE_PROTOCOL.md`](SSE_PROTOCOL.md)**（「流错误 `code` 枚举」一节）。

## `chat` 进程退出码

| 码 | 含义 | 典型场景 |
|----|------|----------|
| 0 | 成功 | 回合完成且未触发「全拒」分支 |
| 1 | 一般错误 | I/O、配置、未归类失败 |
| 2 | 用法 / 输入错误 | 参数不合法、JSON/JSONL 解析失败 |
| 3 | 模型 / 解析错误 | 网关错误体、响应无法解析、部分规划无效前缀（启发式归类见 `classify_model_error_message`） |
| 4 | 本回合内全部 `run_command` 均被审批拒绝 | 管道下未输入 `y`/`a`、或交互全选拒绝 |
| 5 | 配额 / 限流 | HTTP 429、402、部分 503 等（启发式） |
| 6 | 工具重放与录制不一致 | `tool-replay run --compare-recorded` 下至少一步输出与 `recorded_output` 字符串不等 |

实现常量：`EXIT_GENERAL`、`EXIT_USAGE`、`EXIT_MODEL_ERROR`、`EXIT_TOOLS_ALL_RUN_COMMAND_DENIED`、`EXIT_QUOTA_OR_RATE_LIMIT`、`EXIT_TOOL_REPLAY_MISMATCH`（`src/runtime/cli_exit.rs`）。契约测试：`tests/cli_contract.rs`。

## SSE / 流式错误码（Web `POST /chat/stream`）

控制面 JSON 中 **`error` + 非空 `code`** 表示流级失败（与模型正文中的 `{"error":"…"}` 区分）。常见 **`code`** 见 **[`docs/SSE_PROTOCOL.md`](SSE_PROTOCOL.md)** 中「流错误 `code` 枚举」表，例如：

| `code` | 含义（摘要） |
|--------|----------------|
| `INTERNAL_ERROR` | 队列或内部未预期错误 |
| `CONVERSATION_CONFLICT` | 会话版本冲突 |
| `plan_rewrite_exhausted` | 终答规划重写次数用尽（可选 `reason_code`，见 `docs/SSE_PROTOCOL.md`） |
| `SSE_ENCODE` | 控制面 JSON 序列化失败（兜底） |

**`INTERNAL_ERROR`** 仅出现在 **SSE 流** 场景，**不**映射为 `chat` 子进程的上述数字退出码；`chat` 失败仍由 `classify_model_error_message` 等对**错误字符串**归类。

**HTTP JSON（非 SSE `data:`）**：`POST /chat`、`POST /chat/stream` 在握手阶段返回的 **`ApiError.code`** 全集以 **`web/chat_handlers`** 与 OpenAPI 为准；与 **SSE 协议版本**相关的补充码见 **[`docs/SSE_PROTOCOL.md`](SSE_PROTOCOL.md)**（`SSE_CLIENT_TOO_NEW`、`INVALID_SSE_CLIENT_PROTOCOL`、`STREAM_JOB_GONE` 等）。

## `chat --output json` 每行结果（稳定形状）

每轮结束后向 **stdout** 打印**一行** JSON（UTF-8），便于 `jq` / 脚本解析。

### 顶层字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `type` | string | 固定 **`crabmate_chat_cli_result`** |
| `v` | number | 模式版本，当前 **`1`** |
| `reply` | string | 本轮最后一条 `assistant` 的 `content`（无则空串） |
| `model` | string | 当前配置中的模型 id |
| `batch_line` | number? | 仅 **`--message-file`** 批跑时出现，为 JSONL **1-based** 行号 |

### 示例

单轮：

```json
{"type":"crabmate_chat_cli_result","v":1,"reply":"你好。","model":"deepseek-chat"}
```

批跑某行：

```json
{"type":"crabmate_chat_cli_result","v":1,"reply":"…","model":"deepseek-chat","batch_line":3}
```

### 演进

增加字段时应保持 **`v`** 递增或保持后向兼容；破坏性变更须在本文档与 **`crabmate --help`** 的 cross-link 中说明。

## 相关文档

- 子命令与选项：**[`docs/CLI.md`](CLI.md)**
- 流式协议与 `tool_result`：**[`docs/SSE_PROTOCOL.md`](SSE_PROTOCOL.md)**
