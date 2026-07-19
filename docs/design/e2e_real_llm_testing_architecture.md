# E2E 真实 LLM 自动化测试架构

**决策记录（Architecture Decision Record）**

- **状态**：已完成
- **日期**：2026-07-19
- **驱动**：需要一套可重复、可度量的 e2e 测试体系，用于评估 agent 在真实 LLM 环境下的表现和排查回归问题。
- **实现分支**：`feat/e2e-llm-recording-infra`（PR #658）

---

## 上下文

CrabMate 作为 AI Agent，其行为高度依赖 LLM 输出。纯 mock 测试无法覆盖：
- LLM 网关兼容性（DeepSeek / MiniMax / GLM / Kimi 等不同厂商）
- 流式 SSE 协议正确性
- 多轮工具编排的实效性
- 工具报错后的自我恢复能力

需要一个**真实 LLM** 的 e2e 测试方案，同时兼顾 CI 可重复性和开发者本地调试效率。

## 决策：三层 e2e 架构

采用**分层测试**策略，与主流开源 agent 项目（LangChain、AutoGPT、OpenAI Agents SDK）一致：

```
Layer 3: Tauri UI ─── 桌面端 WebView 集成（Victauri）
Layer 2: 编排级 ───── system prompt → run_agent_turn → 消息记录
Layer 1: HTTP/SSE ─── 协议级：SSE 流解析、错误分类
```

三层相互独立，下层测试不依赖上层 UI 渲染。

### Layer 1：HTTP/SSE 协议级

- 文件：`tests/e2e_http_real_llm.rs`
- 启动真实 HTTP server → 通过 SSE 流发送请求 → 解析事件 → 验证流式完成
- 验证内容：`SsePayload` 各枚举变体、流式 token 累积、错误分类

### Layer 2：编排级（本期重点）

- 文件：`tests/e2e_orchestration_real_llm.rs` + `src/e2e_scenario.rs`
- 直接调用 `run_agent_turn()`，注入自定义 `llm_backend`
- 使用**生产路径**构建 system prompt（`compose_first_system_for_turn` + `compose_new_conversation_messages`）
- 产出结构化指标 `TestRunMetrics`（耗时、LLM 轮数、工具调用、报错恢复等）

### Layer 3：Tauri UI（Victauri）

- 已有 `victauri_real_llm.rs`，覆盖桌面 WebView 集成场景

## 录制/回放机制

```
        ┌──────────┐
        │ E2eMode  │
        ├──────────┤
        │ Real     │ → 直接调用 LLM
        │ Record   │ → 调用 LLM + 落盘
        │ Replay   │ → 回放录制数据
        └──────────┘
```

- 录制数据格式：请求 fingerprint（SHA-256）→ 响应 JSONL
- 回放匹配：fingerprint 精确匹配，支持顺序回放和随机访问
- 录制目录：`tests/fixtures/llm_recordings/<scenario_name>/`

## TestScenario 场景模型

```rust
pub struct TestScenario {
    pub name: &'static str,
    pub user_message: &'static str,
    pub workspace_files: &'static [(&'static str, &'static str)],
    pub expected_output_contains: &'static [&'static str],
    pub expected_tool_used: Option<&'static str>,
}
```

每个场景是自描述的：包含用户消息、可选工作区文件、期望条件。新增场景只需在列表中添加一项。

## TestRunMetrics 指标模型

```rust
pub struct TestRunMetrics {
    pub scenario: String,
    pub success: bool,
    pub duration_ms: u64,
    pub llm_rounds: usize,
    pub tool_call_count: usize,
    pub tool_errors: usize,
    pub tool_error_recovered: bool,
    pub tool_names: Vec<String>,
    pub total_messages: usize,
    pub final_output_preview: String,
    pub expected_output_matched: bool,
    pub error_message: Option<String>,
    // ...
}
```

支持 JSON 序列化，便于多轮对比和 CI 报告消费。

## CLI 子命令 `crabmate e2e`

将 e2e 场景框架从 `tests/` 提升到 `src/` 后，直接用 CLI 运行：

```bash
crabmate e2e                    # 真实 LLM 模式
crabmate e2e --mode record      # 录制模式
crabmate e2e --mode replay      # 回放模式
crabmate e2e --output-dir /tmp/e2e  # 指定输出目录
```

CLI 入口复用生产配置加载路径（`load_config` + `read_llm_api_key_from_env_lenient`），
不需要像 `#[ignore]` 测试那样手动传递 `REAL_LLM_E2E=1` 环境变量。

## 架构图

```
用户 / CI
    │
    ├── CLI: crabmate e2e
    │       │
    │       ├── load_config() ───→ 生产配置
    │       ├── E2eRunConfig ───→ 模式 / 路径 / API_KEY
    │       ├── run_e2e_cli()
    │       │       ├── preset_scenarios()
    │       │       ├── run_scenario()  (× N)
    │       │       └── generate_report()
    │       │
    │       └── artifact 输出
    │               ├── scenario_report.md
    │               ├── scenario_report.json
    │               └── <scenario>/{metrics,session,summary}.*
    │
    └── cargo test (REAL_LLM_E2E=1)
            │
            └── e2e_all_scenarios()
                    └── run_scenario() × N
```

## 输出产物

每次执行后 artifact 输出到 `.crabmate/e2e_artifacts/<scenario_name>/`：

| 文件 | 格式 | 用途 |
|------|------|------|
| `metrics.json` | JSON | 结构化指标（机器消费） |
| `summary.md` | Markdown | 简短摘要（人读） |
| `messages_final.md` | Markdown | 完整消息记录（人读排查） |
| `messages_final.json` | JSON | 完整消息记录（二次分析） |

根目录还包含 `scenario_report.md`（场景对比表）和 `scenario_report.json`（全部指标的序列化数组）。

## 替代方案与理由

| 方案 | 理由 |
|------|------|
| **不**使用 Python pytest | 项目是 Rust 单体，Python 层增加维护成本 |
| **不**使用 `#[ignore]` 以外的方式做测试 gating | 沿用已有模式，兼容 `cargo test` 工作流 |
| **不**把场景定义放在 YAML/JSON 文件 | Rust 内联编译期校验，避免解析运行时错误 |
| **选择**三层分层而非单层全链路 | 隔离 UI 渲染和 HTTP 协议问题，缩小排障范围 |

## 后续方向

- 并行执行场景（当前顺序执行以竞 LLM 配额）
- 场景录制夹具自动基线更新（nightly 跑真实 LLM → 更新 fixtures → PR）
- 更多场景：Git 操作、MCP 工具、多轮对话续聊
- 评分模型自动评估输出质量（LLM-as-judge）
- CI pipeline 集成：main PR 走 `--mode replay`，nightly 走 `--mode record`
