# Benchmark 功能与测试规划

本文档专门约束 **Agent 批量测评（`crabmate bench`）** 的路线、范围与测试策略，避免与 **通用对话 / Web / CLI / TUI** 的产品文档（`README.md`、`docs/CLI.md`、`docs/DEVELOPMENT.md` 等）混写一堆「评测专用」细节。

**代码入口**：`src/runtime/benchmark/`、`src/lib.rs` 中 `bench` 分支；**CLI**：`docs/CLI.md` § Benchmark。

---

## 1. 范围边界

| 属于本文档 / benchmark 子系统 | 不属于（见通用文档） |
|------------------------------|----------------------|
| `--benchmark` / `--batch`、JSONL 任务形状、`BenchmarkResult` 产出 | `serve` / `repl` / `chat` 的交互与 SSE |
| 各 adapter（SWE-bench / GAIA / HumanEval / generic）的输入校验、工作区 setup、产物抽取 | `run_agent_turn` 内部通用规划、工具白名单（仅在被 bench 调用时涉及） |
| 与 **开源 benchmark 官方 harness** 的对接、数据转换脚本、CI 门禁中的「评测子集」 | 全量 `cargo test`、前端 E2E、业务功能 TODOLIST |

---

## 2. 当前能力（基线）

- **CLI**：`cargo run -- bench --benchmark <type> --batch <input.jsonl> [--batch-output …] [--task-timeout …] [--max-tool-rounds …] [--resume] [--bench-system-prompt <file>] [--no-tools]`。
- **类型**：`swe_bench` | `gaia` | `human_eval` | `generic`（解析见 `src/runtime/benchmark/types.rs::BenchmarkKind`）。
- **单条任务**：`BenchmarkTask`（JSONL 每行一个对象）；字段定义以 **`types.rs`** 为准。
- **执行**：每任务 `run_agent_turn`（bench 参数见 `RunAgentTurnParams::benchmark_batch`），非流式、无终端渲染、可超时取消。
- **输出**：`benchmark_results.jsonl` + 汇总文件；**不**内置 SWE-bench / HumanEval **官方判分**（与论文指标对齐须外挂或后续扩展）。

---

## 3. 目标优先级（建议顺序）

1. **HumanEval（推荐先做）**  
   - 优势：无 `git clone`、任务轻；适配器见 `adapter.rs::HumanEvalAdapter`。  
   - 缺口：**官方 `check` 判分**未集成；`task_id` / `entry_point` / `test` 在 `BenchmarkTask` 中可选存在，但适配器当前仅强校验 `prompt`。  
   - 规划项：见下文 §5。

2. **generic**  
   - 用于冒烟与 CI（最小 JSONL + 可选 mock/跳 LLM 策略若未来引入）。  

3. **GAIA**  
   - 依赖 `FINAL ANSWER:` 抽取（`artifact::extract_final_answer`）；附件路径需与工作区一致。  

4. **SWE-bench**  
   - 最重：clone、checkout、patch 抽取；适合在 HumanEval 管线跑通后再扩展。  

---

## 4. 测试策略（与通用单测分离）

### 4.1 单元测试（已有 / 应延续）

- **`src/runtime/benchmark/artifact.rs`**：`#[cfg(test)]` 覆盖 `extract_final_answer`、`extract_code_completion` 等；改抽取逻辑时必须更新。  
- **`BenchmarkKind::parse`**：错误分支与别名（`human_eval` / `humaneval` 等）可在 `types.rs` 或独立测试模块补强。

### 4.2 不调用 LLM 的契约测试（推荐新增）

- **JSONL → `BenchmarkTask`**：用最小 fixture（1～2 行）断言反序列化与各 `validate_task` 行为。  
- **adapter 纯函数**：`build_user_prompt` / `system_prompt_suffix` 对固定 `BenchmarkTask` 的快照或包含断言（避免与 `run_agent_turn` 耦合）。

### 4.3 集成 / 端到端（可选、成本高）

- **有 `API_KEY` 的环境**：对 `generic` 跑单行 JSONL，断言进程退出码与输出文件非空（宜放 nightly 或手动文档步骤，默认 CI 不强依赖）。  
- **HumanEval 官方分数**：在 CI 外或单独 workflow：bench 产出 → Python 官方 `evaluation` 脚本判分（文档化命令，不把密钥写进仓库）。

### 4.4 与 pre-commit / CI 的约定

- 若修改 **`fixtures/intent_regression.jsonl`** 或 intent 管线，遵循现有 hook；**benchmark 专用 fixture** 建议放在 **`fixtures/benchmark/`**（新建目录时在本文件与 `docs/DEVELOPMENT.md` 索引中登记）。  
- 仅在变更 **benchmark 行为或契约** 时，要求跑：`cargo test -p crabmate` 中与 `runtime::benchmark` 相关的测试子集（或全量，由 PR 说明）。

---

## 5. HumanEval 专项清单（待办）

- [ ] **数据转换**：官方 JSONL → 本仓库 `BenchmarkTask` JSONL（`instance_id`、`prompt` 必填；建议携带 `entry_point` 与 `test` 供判分侧使用，即使当前 serde 忽略多余字段也可写入同一文件供外挂脚本读取——或扩展 `BenchmarkResult` 透传引用 id）。  
- [ ] **判分**：二选一或并存——  
  - **外挂**：`benchmark_results.jsonl` + 原始任务表 → Python 调用官方 `check`；  
  - **内置**（可选）：子命令或 `bench --evaluate-human-eval` 读取 `test` 执行 `check`。  
- [ ] **适配器**：`validate_task` 是否要求 `entry_point`；`extract_code_completion` 与「仅函数体」提示的一致性（避免重复 `def` 导致官方 `check` 失败）。  
- [ ] **基线**：记录 pass@k 与模型/配置版本到 **`docs/BENCHMARK_RESULTS.md`**（可选新建，与本文档区分：本文档=规划，该文件=基线记录）。  
- [ ] **文档**：在 **`docs/CLI.md`** 保留简短命令示例；HumanEval JSONL 完整示例与本节清单以本文档为准。

---

## 6. SWE-bench / GAIA 备忘（浅规划）

- **SWE-bench**：`repo` / `base_commit` / `problem_statement`；网络与磁盘；结果 patch 来自 `git diff HEAD`；官方验证器对接待单独小节（后续追加到本文档 §7 等）。  
- **GAIA**：`prompt` + `file_attachments`；最终答案格式与 `extract_final_answer` 对齐；多模态/附件路径与安全策略需单列评审。

---

## 7. 文档维护约定

- **新增** benchmark 类型、JSONL 字段、CLI flag、或判分流程：**先更新本文档**，再改 `docs/CLI.md` / `docs/DEVELOPMENT.md` 中的**交叉引用**（保持通用架构文档简短）。  
- **实现完成**的规划条目：从本文档 **删除或改写为「已完成」简述** 均可；若采用「删除」策略，依赖 **Git 历史** 追溯（与 `docs/TODOLIST.md` 仓库规则一致）。  
- **用户可见 CLI 变更**（新子命令、必选参数）：仍须同步 **`docs/CLI.md`** 与 **`README.md`** 中面向使用者的说明。

---

## 8. 相关代码索引

| 路径 | 说明 |
|------|------|
| `src/runtime/benchmark/mod.rs` | 子系统说明 |
| `src/runtime/benchmark/types.rs` | `BenchmarkKind`、`BenchmarkTask`、`BenchmarkResult`、`BatchRunConfig` |
| `src/runtime/benchmark/runner.rs` | `run_batch`、加载 JSONL、写结果 |
| `src/runtime/benchmark/adapter.rs` | 各 benchmark 适配器 |
| `src/runtime/benchmark/artifact.rs` | 从模型输出抽取 patch / 答案 / 代码 |
| `src/runtime/benchmark/metrics.rs` | 单任务与批次指标 |
| `src/lib.rs` | `bench_args` 分派与 `run_batch` 调用 |

---

**版本说明**：本文档随 benchmark 子系统演进迭代；与实现对齐的责任在修改 `src/runtime/benchmark/` 的变更作者。
