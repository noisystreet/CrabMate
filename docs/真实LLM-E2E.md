# 真实 DeepSeek E2E 自动测试

Playwright 用例 **`REAL_LLM_E2E=1`** 时才会执行；默认 CI / `npm test` **不会**调用真实模型。用于验证 Web 流式、`/chat/stream` 与 **Turn 布局导出** 在真实厂商（如 DeepSeek OpenAI 兼容网关）下的行为。

与 [`测试指南.md`](测试指南.md) § 浏览器 E2E 的区别：**不**安装 `/chat/stream` route 桩，会消耗 API 配额，耗时可至 **数分钟～十余分钟**（含工具调用与编译类任务）。

---

## 用例一览

| 文件 | 场景 | 超时（约） | 断言要点 |
|------|------|------------|----------|
| [`e2e/tests/real-llm-smoke.spec.ts`](../e2e/tests/real-llm-smoke.spec.ts) | 单轮「你有哪些技能」 | 3.5 分钟 | 流式完成、发送/停止按钮状态、无「对话失败」 |
| [`e2e/tests/real-llm-turn-layout.spec.ts`](../e2e/tests/real-llm-turn-layout.spec.ts) | 两轮「分析当前目录」→「编译 hpcg」+ 导出 MD | 11 分钟 | 导出 ≥2 条 `## 助手`、工具段存在、旁注/终答分节（非巨泡） |

Turn 布局用例依赖工作区内 **HPCG 相关 tar 包**（默认 `/home/gzz/test` 含 `hpcg-*.tar.gz`）；可换目录，见 [`REAL_LLM_WORKSPACE`](#环境变量)。

---

## 前置条件

1. **Rust 工具链**与仓库依赖可正常 `cargo build`。
2. **前端静态包**：`frontend/dist/index.html` 存在（须先 `trunk build`）。
3. **Playwright**：在 `e2e/` 下已 `npm ci` 且 `npx playwright install chromium`。
4. **模型密钥**（见下一节）：Web 请求 `/chat/stream` 时前端会带 **`client_llm.api_key`**；`serve` 进程本身可不设 `API_KEY`。
5. **`NO_COLOR`**：若 shell 里设了 `NO_COLOR=1`，Trunk/Cargo 可能报错；执行前 **`unset NO_COLOR`**（`playwright.config.ts` 已对 webServer 子进程 unset，手动 `trunk build` 时仍需注意）。

---

## 密钥与模型配置

### 方式 A：Tauri / 桌面壳 user-data（与本机 Web UI 一致）

CrabMate 桌面版写入的用户目录（示例 Linux）：

```text
~/.local/share/crabmate/
├── llm_overrides.json      # api_base、model 等（如 deepseek-v4-flash）
└── secrets/client_llm      # API Key（勿提交、勿贴进 issue）
```

E2E 启动 `serve` 时指定同一目录，Playwright 通过 `/user-data/*` 与 Web 侧栏逻辑一致地带上密钥：

```bash
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
```

`llm_overrides.json` 中 **`api_base`** 需指向 DeepSeek 兼容根（如 `https://api.deepseek.com/v1` 或控制台给出的路径）；**`model`** 与账号可用模型一致。

### 方式 B：环境变量 `API_KEY`

CLI / 部分诊断命令读 **`API_KEY`**（与 Web `client_llm` **不是**同一变量名）。仅跑 smoke、且不用 user-data 时可：

```bash
export API_KEY="YOUR_DEEPSEEK_API_KEY"   # 占位符；勿写入仓库
```

Turn 布局长用例仍建议 **方式 A**，与 Tauri 行为对齐。

### 方式 C：Web 侧栏手动填写

若 `serve` 用临时 `CM_CRABMATE_USER_DATA_DIR`，可在浏览器设置里填 API 密钥；Playwright 无头跑时通常仍依赖 **方式 A** 预置 `secrets/client_llm`。

---

## 工作区

Turn 布局用例会在测试前 **`POST /workspace`** 绑定目录：

- 默认：**`REAL_LLM_WORKSPACE=/home/gzz/test`**（含 `hpcg-HPCG-release-3-1-0.tar.gz` 等）。
- 可改为任意**可信**目录；第二轮「编译 hpcg」会真实解压/编译，会改动该目录。

`serve` 建议与 Playwright 使用**相同**工作区：

```bash
cargo run -- --workspace /home/gzz/test serve --port 18888 --host 127.0.0.1
```

---

## 推荐流程：手动 serve + Playwright 复用（Turn 布局）

长用例、与 Tauri 配置对齐时推荐此方式（`playwright.config.ts` 在本地非 CI 下 **`reuseExistingServer: true`**）。

### 1. 构建前端

```bash
cd /path/to/crabmate_agent
unset NO_COLOR
cd frontend && trunk build
```

### 2. 启动 serve（终端 1）

```bash
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"

cargo run -- --workspace /home/gzz/test serve --port 18888 --host 127.0.0.1
```

确认：`curl -s http://127.0.0.1:18888/health` 返回 JSON（`api_key` 未设时可为 `degraded`，不影响 Web 侧栏密钥）。

### 3. 运行 Playwright（终端 2）

```bash
cd e2e

export REAL_LLM_E2E=1
export E2E_PORT=18888
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
# 可选：export REAL_LLM_WORKSPACE=/path/to/your/test/workspace

# 连通性 smoke（约 1～3 分钟）
npx playwright test tests/real-llm-smoke.spec.ts --workers=1 --retries=0

# Turn 布局 + 导出（约 5～15 分钟，视模型与编译而定）
npx playwright test tests/real-llm-turn-layout.spec.ts --workers=1 --retries=0
```

失败时报告与截图：`e2e/playwright-report/`、`e2e/test-results/`。

---

## 备选流程：Playwright 自带 webServer（Smoke）

不另开终端时，Playwright 会在 **`E2E_PORT`**（默认 **18081**）执行 `cargo run -- serve`。此时 webServer 使用**临时** `CM_CRABMATE_USER_DATA_DIR`（见 `playwright.config.ts`），**不会**自动读取 Tauri 的 `~/.local/share/crabmate`，除非显式传入：

```bash
cd frontend && trunk build
cd ../e2e

export REAL_LLM_E2E=1
export API_KEY="YOUR_DEEPSEEK_API_KEY"   # 或 export CM_CRABMATE_USER_DATA_DIR=...

npx playwright test tests/real-llm-smoke.spec.ts --retries=0
```

Turn 布局长用例仍建议 **手动 serve + 固定 user-data + 固定工作区**，避免临时目录无密钥或工作区不对。

---

## 环境变量

| 变量 | 必需 | 说明 |
|------|------|------|
| **`REAL_LLM_E2E=1`** | 是 | 未设置时两个 spec 内 `test.skip` |
| **`E2E_PORT`** | 否 | 默认 `18081`；与手动 `serve --port` 一致 |
| **`CM_CRABMATE_USER_DATA_DIR`** | 推荐 | Tauri 同目录；含 `llm_overrides.json` 与 `secrets/client_llm` |
| **`REAL_LLM_WORKSPACE`** | 否 | Turn 布局工作区，默认 `/home/gzz/test` |
| **`API_KEY`** | 可选 | CLI/ smoke 备用；Web 流式优先 `client_llm` |
| **`CM_E2E_FIXTURES=1`** | 否 | 真实 LLM 用例**不依赖** E2E 夹具路由；Playwright 默认 webServer 会设，手动 serve 可不设 |

---

## 断言说明（Turn 布局）

[`real-llm-turn-layout.spec.ts`](../e2e/tests/real-llm-turn-layout.spec.ts) 在会话列表导出 **MD** 后检查：

- Markdown 中 **`## 助手`** 至少 **2** 节（批说明 + 终答，而非单条巨泡）；
- 存在 **`## 工具`** 段；
- 正文含「解压」类旁注，且终答相关关键词（如「编译完成」「xhpcg」）出现在其后；
- 第一个 **`## 工具`** 之前至少有一条 **`## 助手`**。

形态 B（无 `turn_segment_*`、仅 plain delta + `tool_call` + `turn_tool_phase_end`）下，**首个无旁注工具可能排在 batch 之前**；断言不要求 batch 一定在 `## 工具` 之前，但要求 **batch 与终答分节**。UI 上仍应看到工具批旁注与终答分开展示（见 [`Turn布局设计.md`](Turn布局设计.md)）。

---

## 排障

| 现象 | 处理 |
|------|------|
| 用例被 skip | 确认 `REAL_LLM_E2E=1` |
| `LLM_API_KEY_REQUIRED` / 流式 4xx | 检查 `CM_CRABMATE_USER_DATA_DIR` 下 `secrets/client_llm` 或 Web 设置 |
| Trunk 失败 | `unset NO_COLOR` 后重跑 `trunk build` |
| 端口占用 / 连不上 | 换 `E2E_PORT` 或 `pkill` 旧 `serve`；`curl` `/health` |
| 超时（>11 分钟） | 模型慢或工具链编译久；可仅跑 smoke；或临时加大 spec 内 `REAL_LLM_TIMEOUT` |
| 导出仍像巨泡 | 确认已 `trunk build` 最新前端；对照 [`Turn布局设计.md`](Turn布局设计.md) § Phase 8–9；用桩用例 `sse-turn-layout-interleaved.spec.ts` 先隔离 |
| `trunk` / wayland 链接错误 | 见 [`开发文档.md`](开发文档.md) E2E 小节 **`libwayland-dev`** |

密钥与日志：**勿**在日志、截图、PR 中粘贴真实 API Key（见 [`.cursor/rules/secrets-and-logging.mdc`](../.cursor/rules/secrets-and-logging.mdc)）。

---

## 相关文件

- Playwright 配置：[`e2e/playwright.config.ts`](../e2e/playwright.config.ts)
- 默认 E2E（桩）：[`scripts/e2e.sh`](../scripts/e2e.sh)、[`测试指南.md`](测试指南.md)
- Turn 布局桩 + 金样：[`e2e/tests/sse-turn-layout-interleaved.spec.ts`](../e2e/tests/sse-turn-layout-interleaved.spec.ts)、`cargo test -p crabmate-turn-layout golden_turn_project_web`
- 英文简述：[`docs/en/REAL_LLM_E2E.md`](en/REAL_LLM_E2E.md)

**说明**：真实 LLM E2E **未**接入 GitHub Actions CI（成本与 flaky）；仅本地或手动 opt-in 运行。
