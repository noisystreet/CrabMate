**语言 / Languages:** 中文（本页）· [English](en/TESTING.md)

# 测试与质量检查

本文汇总在仓库根目录（或注明子目录）执行的**自动化测试与常用检查命令**，覆盖 Rust 后端、Leptos 前端、浏览器 E2E 与依赖审计。更细的模块与协议说明见 [`DEVELOPMENT.md`](DEVELOPMENT.md)。

## 前置条件

- **Rust**：1.85+（edition 2024），见 [`README.md`](../README.md)。
- **端到端（E2E）**：Node.js、npm；首次需安装 Playwright 的 Chromium。
- **Web 静态资源**：E2E 与 `serve` 需要 **`frontend-leptos/dist/index.html`**，须先 **`cd frontend-leptos && trunk build`**（发布体积用 **`trunk build --release`**）。

## 提交前检查（pre-commit）

与 [`.pre-commit-config.yaml`](../.pre-commit-config.yaml) 对齐，建议在提交前执行：

```bash
pre-commit run --all-files
```

其中包含（节选）：

- **`cargo fmt --all`**
- **`cargo clippy --all-targets --all-features -- -D warnings`**
- **`cargo test golden_sse_control`**（当改动 `fixtures/sse_control_golden.jsonl` 或 `src/sse/control_dispatch_mirror.rs` 时由钩子条件触发）

未安装 pre-commit 时，可至少执行：

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

**说明**：`pre-commit run --all-files` **不会**跑 `commit-msg`；提交说明格式在 **`git commit`** 时由 conventional-pre-commit 校验（见 [`.cursor/rules/conventional-commits.mdc`](../.cursor/rules/conventional-commits.mdc)）。

## Rust：工作区单元测试与集成测试

在**仓库根**执行（默认覆盖 workspace 成员：`crabmate`、`crabmate-web-leptos`、`crabmate-sse-protocol`）：

```bash
cargo test
```

### 按包筛选

| 包 | 命令 | 说明 |
| --- | --- | --- |
| 主程序与后端库 | `cargo test -p crabmate` | 大部分 `src/` 与 `tests/` 测例在此包 |
| SSE 协议 crate | `cargo test -p crabmate-sse-protocol` | 协议版本与文档标记自检等 |
| Web 前端 crate | `cargo test -p crabmate-web-leptos` | 见下文「前端（Leptos）」 |

### 按名称过滤（示例）

```bash
cargo test golden_sse_control
cargo test control_dispatch_mirror
cargo test tool_result_envelope_golden
```

改动 **SSE 控制面**分支顺序时，须同步金样并跑 `golden_sse_control`（见 [`SSE_PROTOCOL.md`](SSE_PROTOCOL.md)）。**合并/发版前**若改动跨 crate 或公共 API，建议全量 **`cargo test`**（与 [`.cursor/rules/rust-clippy-and-tests.mdc`](../.cursor/rules/rust-clippy-and-tests.mdc) 一致）。

### 可选：nightly 测试

部分环境会用 nightly 跑全量：

```bash
cargo +nightly test
```

## 前端（Leptos / `frontend-leptos`）

### 宿主目标单元测试（默认）

在仓库根：

```bash
cargo test -p crabmate-web-leptos
```

或在 `frontend-leptos` 目录：

```bash
cd frontend-leptos && cargo test
```

覆盖 Markdown 净化、会话逻辑、`debounce_schedule` 等纯 Rust 逻辑（不启动浏览器）。

### WASM 目标测试（可选）

`wasm-bindgen-test` 用例需安装与 **`Cargo.lock`** 中 **`wasm-bindgen`** 版本一致的 **`wasm-bindgen-cli`**，并指定 test runner。当前锁文件中为 **0.2.114**，示例：

```bash
cargo install wasm-bindgen-cli --version 0.2.114 --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown -p crabmate-web-leptos
```

若锁文件升级了 `wasm-bindgen`，请将安装命令中的版本号改为锁文件中的版本。

### 编译检查（无测例）

协议或前端大改时，至少做一次 WASM 目标类型检查：

```bash
cd frontend-leptos && cargo check --target wasm32-unknown-unknown
```

### 构建静态包（非测试，但 E2E / serve 依赖）

```bash
cd frontend-leptos && trunk build
# 发布或对齐生产体积：
# cd frontend-leptos && trunk build --release
```

## 浏览器端到端（E2E，Playwright）

目录：**`e2e/`**。对 **`POST /chat/stream`**、**`/workspace`** 等使用 **route 桩**，**不调用真实 LLM**。用例见 **`e2e/tests/smoke.spec.ts`**；选择器优先使用 **`data-testid`**。

```bash
cd frontend-leptos && trunk build
cd ../e2e && npm ci
npx playwright install chromium
npm test
```

说明：

- **`playwright.config.ts`** 会启动 **`cargo run -- serve --port 18081`** 并等待 **`GET /health`**。
- 端口可通过环境变量 **`E2E_PORT`** 覆盖，例如：`E2E_PORT=19090 npm test`。
- 本地非 CI 时可能**复用**已在该端口运行的 `serve`（见配置中的 `reuseExistingServer`）。
- 调试：`cd e2e && npm run test:ui`。

Linux 上若 `cargo` 在 **wayland** 相关依赖处失败，见 [`DEVELOPMENT.md`](DEVELOPMENT.md) § E2E 中的 **`libwayland-dev`** 说明。

## 依赖安全与许可证（与 CI 对齐）

工作流见 [`.github/workflows/dependency-security.yml`](../.github/workflows/dependency-security.yml)。本地需安装 **`cargo-audit`**、**`cargo-deny`** 后执行：

```bash
cargo audit
cargo deny check licenses bans sources
```

配置见根目录 **`deny.toml`**。此类检查**未**接入 pre-commit，避免每次提交拉取 advisory 数据库。

## 非自动化项

- **样式与布局手测清单**：[`docs/frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md`](frontend-leptos/VISUAL_REGRESSION_CHECKLIST.md)（仓库内无自动化截图对比流水线）。

## 另见

- 架构与 E2E 细节：[`DEVELOPMENT.md`](DEVELOPMENT.md)（§ `frontend-leptos`、`E2E`）
- SSE 契约与金样：[`SSE_PROTOCOL.md`](SSE_PROTOCOL.md)
- 调试与日志：[`DEBUG.md`](DEBUG.md)
