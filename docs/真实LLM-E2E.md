# 真实 LLM E2E 自动测试

CrabMate 采用**三层 e2e 架构**，详见 [`docs/design/e2e_real_llm_testing_architecture.md`](design/e2e_real_llm_testing_architecture.md)。

所有真实 LLM 测试默认被 `#[ignore]`；CI / `cargo test` **不会**调用真实模型。

---

## Layer 2：编排级（Orchestration）

直接调用 `run_agent_turn()`，注入录制/回放后端，使用生产路径构建 system prompt。不依赖 WebView 或 SSE 协议，适合评估 agent 工具编排和回复质量。

### 文件

| 文件 | 说明 |
|------|------|
| `src/e2e_scenario.rs` | 场景定义、指标收集、统一 runner（CLI 和测试共用） |
| `tests/e2e_orchestration_real_llm.rs` | 编排级集成测试用例 |

### 用例一览

| 场景 | 覆盖要点 |
|------|----------|
| `orch_single_agent_smoke` | 简单问候，验证一轮 LLM 调用后正常结束 |
| `orch_single_agent_tool` | 工具调用（`get_current_time`），验证工具调用 → 终答闭环 |
| `orch_single_agent_skills` | 技能询问（"你有哪些技能"），验证 agent 能描述自身能力 |
| `orch_cpp_cmake` | 多步工具编排（read_file → run_command），验证编译执行全流程 |

### 运行

```bash
# CLI 子命令（推荐——自动加载配置 `/ API_KEY）
crabmate e2e

# 录制模式
crabmate e2e --mode record

# 回放模式
crabmate e2e --mode replay

# 指定输出目录
crabmate e2e --output-dir /tmp/e2e-results

# 或通过 cargo test（需 --include-ignored）
REAL_LLM_E2E=1 cargo test e2e_all_scenarios -- --include-ignored --nocapture

# 单个场景
REAL_LLM_E2E=1 cargo test e2e_single_agent_smoke -- --include-ignored --nocapture
```

### 输出

每次执行后 artifact 输出到 `.crabmate/e2e_artifacts/<scenario_name>/`：

- `metrics.json` — 结构化指标
- `summary.md` — 简短摘要
- `messages_final.{md,json}` — 完整消息记录

根目录还生成 `scenario_report.md`（场景对比表）和 `scenario_report.json`。

### 指标字段

| 字段 | 说明 |
|------|------|
| `success` | 是否整体成功 |
| `duration_ms` | 耗时（毫秒） |
| `llm_rounds` | LLM 调用轮数 |
| `tool_call_count` | 工具调用次数 |
| `tool_errors` | 工具报错次数 |
| `tool_error_recovered` | 报错后是否自我恢复 |
| `tool_names` | 使用到的工具名（去重） |
| `expected_output_matched` | 最终回复是否包含期望关键词 |
| `expected_tool_matched` | 是否使用了期望工具 |

---

## Layer 1：HTTP/SSE 协议级

通过 HTTP SSE 流直接验证 `/chat/stream` 协议正确性。

| 文件 | 场景 | 断言要点 |
|------|------|----------|
| `e2e_http_real_llm.rs` | SSE 流接收完成 | 收到 `OkFinish` 事件 |
| `e2e_http_real_llm.rs` | 错误分类 | 错误事件落入正确分类 |

```bash
REAL_LLM_E2E=1 cargo test e2e_http_ -- --include-ignored --nocapture
```

---

## Layer 3：Tauri/WebView 级（Victauri）

**`REAL_LLM_E2E=1`** 时才会执行；用于验证 Tauri WebView 中流式渲染在真实厂商下的行为。

| 文件 | 场景 | 超时（约） | 断言要点 |
|------|------|------------|----------|
| `victauri_real_llm.rs` | 单轮「你有哪些技能」 | 5 分钟 | 流式完成、助手气泡出现、无错误 |
| `victauri_real_llm.rs` | 单轮「编译 hpcg」+ 流转 | 5 分钟 | 助手回复存在、无错误 |

### 前置条件

1. **Rust 工具链**与仓库依赖可正常 `cargo build`。
2. **前端静态包**：`frontend/dist/index.html` 存在（须先 `trunk build`）。
3. **Tauri CLI**：`cargo install tauri-cli --version "^2"`。
4. **模型密钥**：设置 `API_KEY` 环境变量（Tauri WebView 无 localStorage，密钥需由后端进程继承）。
5. **`NO_COLOR`**：执行前 `unset NO_COLOR`。

### 运行

```bash
# 终端 1：启动 Tauri 桌面应用
cd desktop-tauri/src-tauri
CM_DESKTOP_BACKEND_BIN=/path/to/target/debug/crabmate cargo tauri dev

# 终端 2：运行真实 LLM 测试
cd desktop-tauri/src-tauri
VICTAURI_E2E=1 CM_E2E_FIXTURES=1 REAL_LLM_E2E=1 cargo test --test victauri_real_llm -- --nocapture
```

---

## 录制/回放

三种模式由 [`E2eMode`](https://github.com/noisystreet/CrabMate/blob/main/crates/crabmate-llm/src/lib.rs) 控制：

| 环境变量 | 效果 |
|----------|------|
| 默认（无） | `Real` 模式，直接调用 LLM |
| `CM_E2E_RECORD=1` | `Record` 模式，调用 LLM 并落盘到 `tests/fixtures/llm_recordings/` |
| `CM_E2E_MODE=replay` | `Replay` 模式，从录制数据回放 |

录制数据基于请求 fingerprint（SHA-256）匹配回放，保证 CI 可重复性。

---

## 密钥配置

CLI 子命令 `crabmate e2e` 自动通过 `load_config` + `read_llm_api_key_from_env_lenient` 加载密钥（环境变量 `API_KEY` 或配置路径）。

集成测试通过 `REAL_LLM_E2E=1` 环境变量启用，密钥同样来自 `API_KEY` 或回退到 `~/.local/share/crabmate/secrets/client_llm`。

---