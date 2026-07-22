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

# 运行单个文件
npx playwright test specs/mock-overlay-timing.spec.ts

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
    └── mock-overlay-timing.spec.ts  — mock SSE 回归测试
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

### 注意事项

- **持久化验证**：mock SSE 不包含 `conversation_saved` 事件，无法验证消息持久化。持久化回归由 `victauri_turn_layout.rs` 覆盖。
- **第二次 answer_phase**：无 delta 的第二次 `assistant_answer_phase` 后紧跟 `RUN_FINISHED`，可触发 `followup_pending` 在 `on_done` 中处理的路径（PR #678 修复二的精确场景）。
- **状态栏等待**：使用 `[data-testid="status-bar"]` 包含文本 "就绪" 判断流完成。
- **选择器偏好**：优先使用 `data-testid` 属性选择器，避免依赖文本或 CSS 类名。

## CI 集成

```yaml
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
| proxy 干扰 | 环境变量 `http_proxy` 影响本地连接 | `no_proxy=127.0.0.1,localhost` |
| 前端 WASM 未加载 | `frontend/dist/` 未构建 | `cd frontend && trunk build` |
