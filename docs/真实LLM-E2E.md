# 真实 DeepSeek E2E 自动测试

Playwright 用例 **`REAL_LLM_E2E=1`** 时才会执行；默认 CI / `npm test` **不会**调用真实模型。用于验证 Web 流式、`/chat/stream` 与 **Turn 布局导出** 在真实厂商（如 DeepSeek OpenAI 兼容网关）下的行为。

与 [`测试指南.md`](测试指南.md) § 浏览器 E2E 的区别：**不**安装 `/chat/stream` route 桩，会消耗 API 配额，耗时可至 **数分钟～十余分钟**（含工具调用与编译类任务）。

---

## 用例一览

| 文件 | 场景 | 超时（约） | 断言要点 |
|------|------|------------|----------|
| [`real-llm-smoke.spec.ts`](../e2e/tests/real-llm-smoke.spec.ts) | 单轮「你有哪些技能」 | 5 分钟 | 流式完成、发送/停止按钮、无「对话失败」 |
| [`real-llm-turn-layout-analyze.spec.ts`](../e2e/tests/real-llm-turn-layout-analyze.spec.ts) | 单轮「分析当前目录」 | 6 分钟 | 导出 ≥1 条 `## 助手` |
| [`real-llm-turn-layout-compile.spec.ts`](../e2e/tests/real-llm-turn-layout-compile.spec.ts) | 单轮「编译 hpcg」+ 导出 MD | 6 分钟 | 编译轮 batch/final 分节、非巨泡 |
| [`real-llm-turn-layout.spec.ts`](../e2e/tests/real-llm-turn-layout.spec.ts) | 两轮 analyze → compile + 导出 | 11 分钟 | 同上（完整集成） |

共享逻辑见 [`e2e/tests/helpers/real-llm.ts`](../e2e/tests/helpers/real-llm.ts)（`sendAndWaitForStream`、`assertCompileTurnLayoutExport`、artifact 落盘）。

Turn 布局用例依赖工作区内 **HPCG 相关 tar 包**（默认 `/home/gzz/test` 含 `hpcg-*.tar.gz`）；可换目录，见 [`REAL_LLM_WORKSPACE`](#环境变量)。**已编译**（存在 `bin/xhpcg`）时模型措辞可能不含「解压」，断言用关键词集合而非固定原文。

---

## 一键脚本（P1）

仓库根目录：

```bash
# 需已 trunk build + 手动 serve（见下）；默认 E2E_PORT=18888、Tauri user-data
./scripts/real-llm-e2e.sh smoke      # 连通性
./scripts/real-llm-e2e.sh analyze    # 单轮分析（快）
./scripts/real-llm-e2e.sh compile    # 单轮编译 + Turn 布局
./scripts/real-llm-e2e.sh layout     # 两轮完整
./scripts/real-llm-e2e.sh all        # 全部

# 或在 e2e/ 下：
npm run test:real-llm:compile
```

可选：`REAL_LLM_GREP=compile ./scripts/real-llm-e2e.sh all`

---

## 失败 artifact（P0）

失败时（或 **`REAL_LLM_CAPTURE=1`** 时即使通过）写入：

```text
e2e/artifacts/real-llm/<ISO时间>_<用例名>/
├── meta.json                 # 端口、model、workspace 前置、api_key 是否 ok（无密钥值）
├── turn-layout-report.json   # 编译轮：工具前/后 assistant 条数、摘要、megaBubbleSuspected
├── stream-layout-report.json # 流式 DOM 采样：violations、stable_traces、samples_excerpt
├── export.md                 # 导出 Markdown 全文
├── export.json               # 导出 JSON（若可用）
└── playwright-error.txt
```

排查顺序：先看 `stream-layout-report.json`（生成中跳变）→ `turn-layout-report.json` → `export.md` 编译轮 `## 助手` 节 → 对照桩测 `sse-turn-layout-interleaved.spec.ts` → `cargo test -p crabmate-turn-layout golden_turn_project_web`。

目录已 `.gitignore`；**勿**提交含密钥或敏感路径的 artifact。

---

## 前置条件

1. **Rust 工具链**与仓库依赖可正常 `cargo build`。
2. **前端静态包**：`frontend/dist/index.html` 存在（须先 `trunk build`）。
3. **Playwright**：在 `e2e/` 下已 `npm ci` 且 `npx playwright install chromium`。
4. **模型密钥**（见下一节）：Playwright 每次启动**全新的浏览器上下文**（临时 profile），localStorage 中无 API key。因此 `serve` 进程**必须**设置 **`API_KEY`** 环境变量（方式 B），或通过 `CM_CRABMATE_USER_DATA_DIR` 使密钥对 serve 可用（Playwright 配置中的 `webServer.env` 会传递该变量，但手动 serve 时须自行设置）。
5. **`NO_COLOR`**：若 shell 里设了 `NO_COLOR=1`，Trunk/Cargo 可能报错；执行前 **`unset NO_COLOR`**。

---

## 密钥与模型配置

### 方式 A：Tauri / 桌面壳 user-data（与本机 Web UI 一致）

```text
~/.local/share/crabmate/
├── llm_overrides.json
└── secrets/client_llm
```

```bash
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
```

**注意**：此方式仅使密钥对 `serve` 进程可读，但 Playwright 浏览器上下文是全新的，不会自动获取该密钥。配合 Playwright 自启动 serve（见 `playwright.config.ts` `webServer.env`）时可借助 `CM_CRABMATE_USER_DATA_DIR` 传递；手动 serve 时仍须额外设置 **`API_KEY`** 环境变量（见方式 B）。

### 方式 B：环境变量 `API_KEY`（推荐，适用于 Playwright E2E）

```bash
export API_KEY="YOUR_DEEPSEEK_API_KEY"   # 占位符；勿写入仓库
```

`serve` 进程设有 `API_KEY` 时，所有经 `/chat/stream` 的请求使用该密钥作为默认值，Playwright 浏览器无需额外配置。**所有真实 LLM E2E 用例均建议此方式**。

---

## 工作区

```bash
export REAL_LLM_WORKSPACE=/home/gzz/test   # 可选
cargo run -- --workspace "$REAL_LLM_WORKSPACE" serve --port 18888 --host 127.0.0.1
```

`setupRealLlmWorkspace` 会在控制台打印 `has_hpcg_tar` / `has_xhpcg`（仅观测，不断言）。

---

## 推荐流程：手动 serve + Playwright

```bash
unset NO_COLOR && cd frontend && trunk build

# 终端 1
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
export API_KEY="$(cat $CM_CRABMATE_USER_DATA_DIR/secrets/client_llm)"  # 从 user-data 读入密钥
cargo run -- --workspace /home/gzz/test serve --port 18888 --host 127.0.0.1

# 终端 2
export CM_CRABMATE_USER_DATA_DIR="$HOME/.local/share/crabmate"
./scripts/real-llm-e2e.sh compile
```

**注意**：`serve` 启动时须有 **`API_KEY`** 环境变量，否则 Playwright 浏览器（全新 profile，无 localStorage）的 `/chat/stream` 请求会因缺少密钥而失败，表现为用例超时或 "Target page, context or browser has been closed" 错误。

---

## 环境变量

| 变量 | 必需 | 说明 |
|------|------|------|
| **`REAL_LLM_E2E=1`** | 是 | 未设置时 spec 内 `test.skip` |
| **`E2E_PORT`** | 否 | 默认 `18081`；与 `serve --port` 一致 |
| **`CM_CRABMATE_USER_DATA_DIR`** | 推荐 | Tauri 同目录 |
| **`REAL_LLM_WORKSPACE`** | 否 | 默认 `/home/gzz/test` |
| **`REAL_LLM_TIMEOUT`** | 否 | 毫秒；默认 `300000` |
| **`REAL_LLM_CAPTURE=1`** | 否 | 通过时也写 artifact |
| **`REAL_LLM_MEGA_BUBBLE_CHARS`** | 否 | 默认 `2500`；report 中 mega 阈值 |
| **`REAL_LLM_GREP`** | 否 | 传给 `playwright -g` |
| **`REAL_LLM_STREAM_MONITOR`** | 否 | 默认 `1`；compile/layout 轮流式 DOM 采样；设 `0` 关闭 |
| **`REAL_LLM_STRICT_STREAM_LAYOUT`** | 否 | 设 `1` 时 violations 非空则用例失败 |
| **`REAL_LLM_STREAM_POLL_MS`** | 否 | DOM 采样间隔，默认 `400` |
| **`API_KEY`** | **编译/布局用例** | serve 进程凭证；Playwright 浏览器上下文无 localStorage，须靠 serve 侧的 `API_KEY` 认证。smoke 用例亦可用此方式备用 |

---

## 断言说明（Turn 布局 · compile 轮）

[`assertCompileTurnLayoutExport`](../e2e/tests/helpers/real-llm.ts) 检查导出 MD：

- 全文 **`## 助手`** ≥ 2；
- 存在用户轮「编译…hpcg」与 **`## 工具`**；
- 工具 **前/后** 各 ≥1 条 `## 助手`，且摘要不同；
- 工具后 assistant 含 `编译|xhpcg|HPCG|总结|完成|成功|make` 之一；
- 单节长度 ≤ `REAL_LLM_MEGA_BUBBLE_CHARS`（否则 `mega_bubble_suspected`）。

形态 B 下首个无旁注工具可在 batch 前；不要求 batch 一定在第一个 `## 工具` 之前。见 [`Turn布局设计.md`](Turn布局设计.md)。

---

## 流式 DOM 监控（气泡跳变 / 消失重现）

HPCG **compile** 等长流式轮次在生成过程中，Turn 投影（`sync_turn_projection`、batch 行重排、loading 尾泡 `pin_loading_tail`）可能导致聊天列 **块顺序回跳** 或 **`turn-batch-narration` / `turn-final-answer` 短暂消失再出现**。

[`sendAndWaitForStreamWithLayoutMonitor`](../e2e/tests/helpers/stream-layout-monitor.ts) 在 `/chat/stream` 期间每 **`REAL_LLM_STREAM_POLL_MS`**（默认 400ms）采样：

- **块序**：`.messages-inner` 子块键（`msg-*` / 工具组 `tg:`）
- **消息 id**：块内全部 `#msg-*`（含工具组内各卡），并读取 **`.msg-loading`** 区分尾泡

流结束后分析：

| violation | 含义 |
|-----------|------|
| `reorder` | stable 投影行（batch/final）块下标 **回跳** |
| `vanished` | 曾以 **非 loading** 出现的消息 id（含 user、工具卡、batch/final）从 DOM **消失** |
| `vanish_reappear` | 已 committed 的消息 id 消失后 **再次出现** |
| `stable_order_inversion` | 同帧 **final 在 batch 前** |
| `committed_reorder` | 任意已 commit id（非 stable 行）块下标 **回跳** |
| `multiple_loading` | 同帧 **多于一个** 非工具 assistant `.msg-loading`（工具卡 loading 与尾泡可并存） |
| `loading_after_stream` | 流结束末帧仍有 `.msg-loading` |

**loading 尾泡**（`.msg-loading`）仅流式占位，允许消失；**user / 工具卡 / stable 行** 一旦出现即 committed，不得消失（ interim assistant `s_*` 行可被投影合并，不纳入 committed）。

**默认仅写 report、不断言**（避免模型/网络 flake 误杀）。修复 UI 回归时可设：

```bash
export REAL_LLM_STRICT_STREAM_LAYOUT=1
./scripts/real-llm-e2e.sh compile
```

**肉眼复现 + 录像**（与自动化共用同一 serve）：

```bash
# 终端 1：serve（见上文）
# 终端 2
export REAL_LLM_E2E=1 REAL_LLM_CAPTURE=1 E2E_PORT=18888
cd e2e && npx playwright test tests/real-llm-turn-layout-compile.spec.ts \
  --headed --workers=1 --trace on --video on
```

离线校验分析器（不进 CI 默认真 LLM）：`cd e2e && npx playwright test tests/stream-layout-monitor.spec.ts`

Artifact：`e2e/artifacts/real-llm/<run>/stream-layout-report.json`（`REAL_LLM_CAPTURE=1` 或失败时）。

---

## 排障

| 现象 | 处理 |
|------|------|
| 用例被 skip | `REAL_LLM_E2E=1` |
| 密钥 / 4xx | `CM_CRABMATE_USER_DATA_DIR` + `secrets/client_llm` |
| 超时 | 用 `./scripts/real-llm-e2e.sh analyze` 或 `compile` 分层跑；加大 `REAL_LLM_TIMEOUT` |
| 超时 / "Target page, context or browser has been closed" | `serve` 进程缺少 `API_KEY`（Playwright 浏览器上下文无 localStorage，无法提供密钥）；重启 serve 时带上 `API_KEY` 即可 |
| 导出仍巨泡 | 最新 `trunk build`；看 `e2e/artifacts/real-llm/…`；桩测 `morph B` |
| 生成中气泡跳变/消失 | `REAL_LLM_CAPTURE=1` + compile；看 `stream-layout-report.json` 的 `vanished` / `vanish_reappear` |
| 断言「compile turn not found」 | 导出用户节为 `## 用户\n\n`；用户消息是否含「编译」「hpcg」 |

---

## 相关文件

- Helper：[`e2e/tests/helpers/real-llm.ts`](../e2e/tests/helpers/real-llm.ts)、[`stream-layout-monitor.ts`](../e2e/tests/helpers/stream-layout-monitor.ts)
- 脚本：[`scripts/real-llm-e2e.sh`](../scripts/real-llm-e2e.sh)
- Playwright：[`e2e/playwright.config.ts`](../e2e/playwright.config.ts)
- 桩 + 金样：[`sse-turn-layout-interleaved.spec.ts`](../e2e/tests/sse-turn-layout-interleaved.spec.ts)、`golden_turn_project_web`
- 英文简述：[`docs/en/REAL_LLM_E2E.md`](en/REAL_LLM_E2E.md)

真实 LLM E2E **未**接入默认 CI；本地或 `REAL_LLM_CAPTURE` 归档后可将事件形态**蒸馏**进 `fixtures/turn_project_web_golden.jsonl`（勿提交 export 全文）。
