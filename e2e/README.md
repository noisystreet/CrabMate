# CrabMate E2E Tests

基于 Playwright 的端到端测试，覆盖前端 Web UI 的核心交互路径。

## 定位

与 `desktop-tauri/src-tauri/tests/` 中 victauri_test 的分工：

| 维度 | 本目录（Playwright） | victauri_test（Rust） |
|------|---------------------|----------------------|
| 运行环境 | headless Chromium + `cargo run -- serve` | 真实 Tauri WebView |
| 覆盖范围 | 纯前端逻辑（overlay 消费、气泡布局） | Tauri 特有行为（IPC、对话框、窗口） |
| mock SSE | ✅ `page.route()` 拦截 | ✅ `eval_js` 注入 fetch |
| 真实 LLM | ✅ 支持 | ✅ 需 `REAL_LLM_E2E=1` |
| console.log | ✅ `page.on('console')` | ❌ 不支持 |
| CI 友好度 | 高（headless，无 GUI 依赖） | 低（需 X display + Tauri debug 编译） |
| 编译开销 | 仅编译后端 | 需编译 Tauri |

**核心原则**：纯前端行为的回归测试优选 Playwright，Tauri 特有行为留 victauri_test。

## 前置条件

```bash
# 1. 后端运行（默认 127.0.0.1:8080）
cargo run -- serve

# 2. 前端已构建（Web 服务自动 serve frontend/dist/）
#    若首次运行或前端有改动，先构建：
cd frontend && trunk build && cd ..
```

## 快速开始

```bash
cd e2e
npm install
npx playwright test
```

### 常用选项

```bash
# 列出所有测试
npx playwright test --list

# 运行 mock SSE 回归测试（CI 中的标准模式）
npx playwright test specs/mock-overlay-timing.spec.ts

# 运行真实 LLM 测试（测试进程请显式设置 API_KEY；不会导出系统钥匙串明文）
npx playwright test specs/real-llm-*.spec.ts

# 运行单个用例（按名称过滤）
npx playwright test --grep "final_response"

# 显示浏览器窗口（调试用）
npx playwright test --headed

# 查看测试报告
npx playwright test --reporter=html
npx playwright show-report playwright-report/
```

## 目录结构

```
e2e/
├── package.json           — npm 项目配置（@playwright/test）
├── playwright.config.ts   — Playwright 配置（baseURL, 超时, reporter）
├── .gitignore
├── fixtures/
│   └── helpers.ts         — 公共辅助函数
└── specs/
    ├── mock-overlay-timing.spec.ts  — mock SSE 回归测试（CI 运行）
    └── real-llm-zero-tool.spec.ts   — 真实 LLM 终答验证（本地运行）
```

## 测试编写指南

### 添加新测试

1. 在 `specs/` 下创建 `*.spec.ts`
2. 引入 `fixtures/helpers.ts` 中的辅助函数
3. 使用 Playwright 标准 `test` / `expect` API

### 公共辅助函数

```typescript
// 创建空会话并加载页面
await seedSession(page, 's_e2e_my_test');

// 发送消息
await sendMessage(page, '你好');

// 拦截 /chat/stream POST 返回 mock SSE
await installMockSse(page, sseBody);

// 终端判断
await expect(page.locator('[data-testid="chat-messages-scroller"]'))
  .toContainText('期望文本', { timeout: 5000 });
```

### Mock SSE 协议格式

前端使用 **AG-UI（V2）** 协议解析 SSE。事件格式：

```json
// 正文相开始（相当于 V1 assistant_answer_phase）
{"type":"CUSTOM","customType":"assistant_answer_phase"}

// 正文增量（纯文本也可直接放到 data 行）
{"type":"TEXT_MESSAGE_CONTENT","delta":"回复内容"}
// 或纯文本：
data: 回复内容

// final_response（timeline_log 类型）
{"type":"CUSTOM","customType":"timeline_log",
 "data":{"kind":"","title":"final_response","detail":"内容"}}

// 工具调用
{"type":"TOOL_CALL_RESULT","toolCallId":"t1","content":"输出",
 "metadata":{"name":"read_file","ok":true}}

// 流结束
{"type":"RUN_FINISHED"}
```

**必须的响应头**：

```
content-type: text/event-stream; charset=utf-8
x-conversation-id: e2e-conv
x-stream-job-id: 1
```

完整示例见 `specs/mock-overlay-timing.spec.ts`。

## 真实 LLM 测试

`specs/real-llm-*.spec.ts` 需要真实 LLM 后端，不在 CI 中运行。

### 密钥解析

Playwright 测试进程不读取或导出 CrabMate 系统钥匙串。请通过环境变量 `API_KEY`（推荐）或仅用于本机测试且已忽略提交的配置提供密钥。

无密钥时测试自动跳过。

### 本地运行

```bash
# 确保后端运行（如系统有 http_proxy，须排除 LLM API 域名防超时）
no_proxy=api.deepseek.com,localhost,127.0.0.1 cargo run -- serve

# 运行真实 LLM 测试
cd e2e && npx playwright test specs/real-llm-*.spec.ts
```

### 注意事项

- **持久化验证**：mock SSE 不包含 `conversation_saved` 事件，无法验证消息持久化。持久化回归由 `victauri_turn_layout.rs` 覆盖。
- **第二次 answer_phase**：无 delta 的第二次 `assistant_answer_phase` 后紧跟 `RUN_FINISHED`，可触发 `followup_pending` 在 `on_done` 中处理的路径（PR #678 修复二的精确场景）。
- **状态栏等待**：使用 `[data-testid="status-bar"]` 包含文本 "就绪" 判断流完成。
- **选择器偏好**：优先使用 `data-testid` 属性选择器，避免依赖文本或 CSS 类名。
- **TUI 流式闪烁**：`specs/mock-tui-stream-flicker.spec.ts` 用 8ms 采样 + `innerHTML` 钩子检测「正文首次出现后短暂消失」；含 delayed `conversation_saved` → `GET /conversation/messages`（revision+1）竞态。`specs/mock-ready-bubble-stability.spec.ts` 冻结已定稿 `section.chat-tui-turn` 的 `data-tui-msg-id`，断言流式中不消失。
- **助手正文清空再出现**：`specs/mock-assistant-content-blank.spec.ts` 冻结首次出现旁白的 `data-tui-msg-id`；旧 mid 移交后变空可接受，但旁白标记不得整段消失再出现。
- **空助手壳先于正文**：`specs/mock-empty-assistant-shell.spec.ts`
  - 发送后～首 token、工具结果后～下一轮正文：MutationObserver 采样；空 `.chat-tui-body` 的 `chat-tui-turn--assistant.is-loading` 即失败 — TUI 不挂载空 Loading 壳（有 overlay 正文后再出现）
- **工具前旁白恰好一条**：`specs/mock-commentary-no-duplicate.spec.ts` 断言流结束后旁白仍可见且 DOM/持久化恰好一条；并覆盖「上轮已有 commentary」时本轮不得被掏空。
- **晚到旁注（形态 A）**：`specs/mock-late-commentary.spec.ts` 对齐金样 `late_commentary_delta_after_tool_call`：工具先于旁白 delta，断言旁白仍在锚定工具之前且恰好一条。
- **流中顺序：描述先于工具**：`specs/mock-commentary-before-tool-order.spec.ts`
  - 旁白先到 / 中间过程第二步：防回归（当前绿）
  - **晚到旁白**（工具 SSE 先于 delta）：MutationObserver 采样；若出现「工具在上、旁白在下」则失败 — 锚点工具已存在时 open 旁白 upsert 到工具前（勿挂 loading 尾）
- **中间过程旁白不双写**：`specs/mock-mid-process-commentary-duplicate.spec.ts` 按 `chat_export_20260729_210001.md` 多工具时序；**流中采样**（每段旁白出现后 + 工具可见后再断言 DOM 恰好 1）；**就绪瞬间**（不等 hydration）断言 DOM / 持久化 / 导出形段各恰好 1；**重载后再断言**恰好 1。共享断言见 `fixtures/session_assertions.ts`（`sampleCommentaryStepsDuringStream`）。
- **真实 LLM 就绪瞬间成对双写**：`specs/real-llm-bubble-layout.spec.ts` 在 `waitForStableSessionMessages` **之前**检查相邻助手正文不得完全相同；其后仍比对重载前后 stored 一致（防 hydrate 拆合）。
- **导出会话「分析当前项目」**：`specs/mock-export-analyze-project-flicker.spec.ts` 按 `chat_export_*.md` 时序重放开场白 + `parsing_tool_calls` + 6× `read_file` + 分块长终答，断言助手正文气泡不归零。
- **真实红测派生 mock**：`specs/mock-real-tool-bubble-vanish.spec.ts` 回放 `real-llm-tool-bubble-vanish` 的两条时序，共同根因是旁白离开 overlay 后在 canonical 里还没有工具锚点，`project_turn_web_v2` 便投影不出 `turn-commentary-*` 行（详见 `docs/Turn布局设计.md` §14 I15）：
  - **无 START 的 `TOOL_CALL_RESULT`** —— `drain(clear=true)` 掏空 overlay，而 canonical 无该工具步；
  - **`turn_segment_start{beforeToolCallId}` 先于 `TOOL_CALL_START`**（真实 SSE 的实际形态，两者相隔约 475ms）—— `reset_loading_tail_streaming_text` 已清 overlay，pending 旁白要等 `ToolCall` 才被吸收。
- **真实 LLM 工具回合闪没**：`specs/real-llm-tool-bubble-vanish.spec.ts` 用导出同款提示「分析一下当前目录下的源码」+ 迷你 `cpp-demo` 工作区；流中采样正文助手不得归零/闪回。需 `API_KEY` 或本机钥匙串 `client_llm`。

### 监控分层（旁白 / 导出）

| 层 | 何时取样 | 查什么 | 主要用例 |
|----|----------|--------|----------|
| 0 流中 | 每段旁白出现后、工具可见后再采 | 已见旁白 DOM count===1；persist 不得 ≥2 | mid-process `sampleCommentaryStepsDuringStream` |
| A 就绪瞬间 | status=就绪后立刻 | 同文旁白 count===1；相邻助手正文不重复 | mid-process、real-llm early |
| B 重载后 | reload + 稳定 | 仍恰好 1；与 A 对比不得「水合才修好」 | mid-process reload |
| C 重载前后一致 | stable → reload → stable | role/is_tool/text 对齐 | real-llm（旧 hydrate 拆合） |
| D 写路径单测 | cargo test | `allow_final_answer` 门控；终答同文移交 | frontend `turn_layout` |
## CI 集成

GitHub Actions：`.github/workflows/e2e-playwright.yml`（PR → `main`）。当前跑布局回归基线（流中采样）+ overlay：

- `specs/mock-overlay-timing.spec.ts`
- `specs/mock-mid-process-commentary-duplicate.spec.ts`
- `specs/mock-commentary-before-tool-order.spec.ts`
- `specs/mock-empty-assistant-shell.spec.ts`
- `specs/mock-real-tool-bubble-vanish.spec.ts`
- `specs/mock-tool-call-scenarios.spec.ts`
- `specs/mock-approval-scenarios.spec.ts`
- `specs/mock-multi-turn.spec.ts`
- `specs/mock-storage-consistency.spec.ts`
- `specs/mock-v2-multi-turn-boundaries.spec.ts`

本地全量 mock：先 `cd frontend && trunk build`，再 `cargo run -- serve`，然后：

`cd e2e && no_proxy=127.0.0.1,localhost npx playwright test`

```yaml
# 示意；以仓库内 workflow 为准
# .github/workflows/e2e-playwright.yml
name: E2E Playwright
on: [pull_request]
jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Build frontend
        run: cd frontend && trunk build
      - name: Start backend
        run: cargo run -- serve &
      - name: Run Playwright tests
        run: cd e2e && npm ci && npx playwright test
      - name: Upload report
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: playwright-report
          path: e2e/playwright-report/
```

## 故障排除

| 问题 | 原因 | 解决 |
|------|------|------|
| `net::ERR_CONNECTION_REFUSED` | 后端未运行 | `cargo run -- serve` |
| 测试超时 20s+ | 状态栏卡住或 SSE mock 未生效 | 检查响应头是否包含 `x-conversation-id` |
| `waitForFunction` timeout | 终答内容未出现 | 确认 SSE 使用 AG-UI V2 格式 |
| proxy 干扰（浏览器） | 环境变量 `http_proxy` 使浏览器无法访问本地后端 | `no_proxy=127.0.0.1,localhost` |
| proxy 干扰（后端 LLM） | 环境变量 `http_proxy` 使后端调用 LLM API 走代理超时 | 启动后端时设置 `no_proxy=api.deepseek.com,localhost,127.0.0.1`（替换为实际 `api_base` 域名） |
| 前端 WASM 未加载 | `frontend/dist/` 未构建 | `cd frontend && trunk build` |
