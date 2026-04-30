# Benchmark 执行指南

本目录用于集中放置 CrabMate benchmark 的执行说明与脚本，避免在 `docs/`、`scripts/` 和命令行参数之间来回查找。

## 常见基准简介

### 本项目已支持（`crabmate bench --benchmark`）

- `human_eval`
  - 面向 Python 函数补全与单元测试通过率，任务轻、链路短，适合作为首选冒烟和回归基准。
  - 本文档主流程即基于该基准。

- `swe_bench`
  - 面向真实仓库问题修复（代码修改 + patch 产出）。
  - 评测成本高（仓库准备、执行耗时、环境依赖重），更适合稳定后做阶段性评估。
  - **逐步命令见下文「SWE-bench 流程」**。

- `gaia`
  - 面向更综合的任务完成与最终答案抽取（包含多步推理/工具使用场景）。
  - 重点在任务完成质量与答案提取稳定性。

- `generic`
  - 通用 JSONL 任务入口，适合自定义内部评测集（回归集、领域题集、AB 对比样本）。
  - 推荐用于团队自建“持续回归小样本”。

### 业界常见基准（本项目当前未内置专用适配）

- `MBPP`
  - Python 编程题基准，规模较小，常用于快速验证代码生成能力。
- `APPS`
  - 覆盖更复杂编程题，难度跨度大，通常需要更强执行与验证链路。
- `LiveCodeBench`
  - 更关注时效性与“未见题”评测，常用于评估模型在新题上的泛化能力。
- `MultiPL-E`
  - 多语言代码生成评测，适合看跨语言能力而非单一 Python 表现。

> 建议：
> - 日常回归：`human_eval` + 小规模 `generic` 内部集；
> - 阶段评估：补充 `swe_bench` / `gaia`；
> - 对外对标：再引入 MBPP/APPS/LiveCodeBench（需额外适配和判分流程）。

### 基准选择速查表

| 基准 | 主要目标 | 执行速度 | 环境成本 | 真实工程性 | 可复现性 | 适用场景 |
|---|---|---|---|---|---|---|
| `human_eval` | 函数级代码正确性 | 快 | 低 | 中 | 高 | 日常冒烟、模型版本回归 |
| `generic` | 自定义业务回归 | 快-中 | 低-中 | 取决于数据集 | 高 | 团队内部持续回归、AB 对比 |
| `gaia` | 综合任务完成质量 | 中 | 中 | 中-高 | 中 | 阶段性能力评估 |
| `swe_bench` | 真实仓库修复能力 | 慢 | 高 | 高 | 中 | 里程碑评估、发布前压测 |
| `MBPP`* | 小规模编程能力 | 快 | 低 | 低-中 | 中-高 | 快速对外参考 |
| `APPS`* | 复杂编程题能力 | 慢 | 中-高 | 中 | 中 | 深度能力对比 |
| `LiveCodeBench`* | 时效性泛化能力 | 中-慢 | 中 | 中-高 | 中 | 评估“新题泛化” |
| `MultiPL-E`* | 多语言代码生成 | 中 | 中 | 中 | 中 | 跨语言能力对比 |

\* 当前本项目未内置专用适配流程（需要额外数据转换与判分集成）。

### 选型建议（按目标）

- **追求执行快、可日常跑**：优先 `human_eval` + 小型 `generic`。
- **追求真实工程修复能力**：增加 `swe_bench`，但建议降低频率（如周/里程碑）。
- **追求综合任务表现**：补 `gaia`，与代码类基准形成互补。
- **追求对外论文/社区可比性**：再接入 MBPP/APPS/LiveCodeBench，但要先统一判分与环境基线。

## 当前推荐流程（HumanEval）

### 0) 获取 `HumanEval.jsonl`（官方来源）

OpenAI HumanEval 官方仓库：

- 仓库主页：<https://github.com/openai/human-eval>
- 数据文件（gzip）：<https://github.com/openai/human-eval/blob/master/data/HumanEval.jsonl.gz>
- 原始下载直链：<https://raw.githubusercontent.com/openai/human-eval/master/data/HumanEval.jsonl.gz>

下载并解压：

```bash
mkdir -p benchmark/data
curl -L "https://raw.githubusercontent.com/openai/human-eval/master/data/HumanEval.jsonl.gz" \
  -o benchmark/data/HumanEval.jsonl.gz
gzip -dc benchmark/data/HumanEval.jsonl.gz > benchmark/data/HumanEval.jsonl
```

### 1) 准备任务文件

如果你已有官方 `HumanEval.jsonl`，先转换为 CrabMate 的任务格式：

```bash
python3 scripts/humaneval_official_to_crabmate_jsonl.py \
  --input benchmark/data/HumanEval.jsonl \
  --output benchmark/humaneval_tasks.jsonl
```

### 2) 执行 benchmark

```bash
cargo run -- bench \
  --benchmark human_eval \
  --batch benchmark/humaneval_tasks.jsonl \
  --batch-output benchmark/humaneval_results.jsonl
```

### 3) 执行外挂判分

```bash
python3 scripts/humaneval_score_benchmark_results.py \
  --tasks benchmark/humaneval_tasks.jsonl \
  --results benchmark/humaneval_results.jsonl \
  --output benchmark/humaneval_scores.jsonl
```

### 4)（可选）断点续跑

长任务中断后可使用 `--resume`，避免从头重跑：

```bash
cargo run -- bench \
  --benchmark human_eval \
  --batch benchmark/humaneval_tasks.jsonl \
  --batch-output benchmark/humaneval_results.jsonl \
  --resume
```

## SWE-bench 流程（`swe_bench`）

本节说明如何用 CrabMate **批量跑 SWE-bench 风格任务**：每条任务会 **克隆上游仓库、checkout 到基线提交、在工作区内调用 Agent 修改代码**，最后在结果 JSONL 中写入 **`model_patch`**（由工作区 **`git diff`** 抽取）。**不包含** SWE-bench 官方 Docker 环境与单元测试判分；若要与论文 / 排行榜指标对齐，须在 **`bench` 产出 patch 之后**，使用 **上游 SWE-bench 仓库提供的评测 harness**（自行对接）。

### 0) 前置（在 HumanEval 共用前提之上）

- **`git`** 已在 `PATH` 中，且本机可访问 GitHub（或你的 `repo` URL）。每条任务会执行 **`git clone`** / **`git checkout`**。
- **磁盘空间**：每条实例会在配置的 **`run_command_working_dir`**（见 `docs/配置说明.md` / `run_command_working_dir`）下创建 **`<instance_id 安全化>/`** 目录并保留完整克隆；全量数据集体积很大，建议使用专用目录与大磁盘。
- **工作区策略**：批量 runner 以配置中的 **`run_command_working_dir`** 为「父目录」存放各实例克隆（见 `src/runtime/benchmark/runner.rs`）。不要在同一目录手动删改正在跑的实例目录，除非你清楚 **`--resume`** 与缓存目录语义。
- **模型与密钥**：与其它 benchmark 相同，需可用的 **`API_KEY`**（若使用 `bearer`）、`api_base`、`model` 等；可先 `cargo run -- probe` 验证联通。

### 1) 准备任务 JSONL（CrabMate `BenchmarkTask`）

每行一个 JSON 对象，**必填字段**如下（与 `src/runtime/benchmark/types.rs` / `SweBenchAdapter` 一致）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `instance_id` | string | 稳定唯一 ID；用于结果主键及本地目录名（`/`、`\\`、空格会被替换为 `_`）。 |
| `repo` | string | `owner/repo`（如 `django/django`），或 **`http(s)://` 开头的完整 clone URL**。非 URL 时等价于克隆 `https://github.com/{repo}.git`。 |
| `base_commit` | string | 基线提交的完整 SHA（或可被 `git checkout` 接受的引用）。 |
| `problem_statement` **或** `prompt` | string | Issue / 任务描述；二者至少填其一（适配器优先使用 `problem_statement`）。 |
| `hints_text` | string（可选） | 额外提示，会追加进发给模型的用户消息。 |

**最小示例一行**（虚构小号仓库，仅演示格式；路径换行仅为可读性，实际 JSONL 需单行或合法 JSON）：

```json
{"instance_id":"demo-owner_demo-repo_42","repo":"demo-owner/demo-repo","base_commit":"abcd1234deadbeef","problem_statement":"修复：某条件下函数返回错误。\n\n复现步骤：…"}
```

将多行合法 JSONL 存为文件，例如 **`benchmark/swe_tasks.jsonl`**。

### 2) 与官方 SWE-bench 数据集对齐（无内置转换脚本）

本仓库 **未** 附带「官方 `instances.jsonl` → CrabMate JSONL」脚本（HumanEval 才有 `scripts/humaneval_official_to_crabmate_jsonl.py`）。若你手上的官方任务文件字段名 **已与上表一致**，可直接作为输入；否则请用 **`jq` / 自写 Python** 等生成上述列。

常见映射思路（以官方实例中含 `instance_id`、`repo`、`base_commit`、`problem_statement` 为例）：

```bash
# 示例：从上游 JSONL 抽出 CrabMate 所需列（按你的文件结构调整 jq 过滤器）
jq -c '{instance_id, repo, base_commit, problem_statement}' \
  /path/to/instances.jsonl > benchmark/swe_tasks.jsonl
```

若上游字段名不同（例如仓库字段拆开），先在脚本里拼成 `repo` 字符串再写出 JSONL。

### 3) 执行 benchmark

在项目根目录执行（路径可按需修改）：

```bash
mkdir -p benchmark

cargo run -- bench \
  --benchmark swe_bench \
  --batch benchmark/swe_tasks.jsonl \
  --batch-output benchmark/swe_results.jsonl \
  --task-timeout 600
```

**类型别名**：`--benchmark swe_bench` 与 `swebench`、`swe-bench`（解析时归一为 `swe_bench`）等价。

**可选常用参数**：

- **`--resume`**：跳过输出 JSONL 中已有 `instance_id` 的任务，适合长跑中断后续跑。
- **`--max-tool-rounds N`**：限制 Agent 工具轮次（默认视配置而定；SWE 场景通常需要工具读写仓库，勿过小）。
- **`--bench-system-prompt <file>`**：追加自定义系统提示文件。
- **`--no-tools`**：关闭工具（一般 **不适合** SWE-bench，除非纯文本补丁策略实验）。

建议阶段评估时适当增大 **`--task-timeout`**（例如 `900` 或更高），避免大仓库慢冷启动导致误超时。

### 4) 输出说明与官方判分

- **`benchmark/swe_results.jsonl`**：每任务一行 **`BenchmarkResult`**；关注字段 **`model_patch`**（成功时多为 unified diff 文本）、**`status`**、**`error`**、**`raw_reply`**。
- 另会生成与 **`--batch-output`** 同目录、同主文件名的 **汇总文件**（runner 内由输出路径派生，终端结束时会打印路径）。
- **官方分辨率 / pass@k**：请将本文件中的 patch 按 **SWE-bench 官方流程** 传入其验证器（Docker、测试命令等）；详见上游项目文档（如 [SWE-bench](https://github.com/princeton-nlp/SWE-bench)）及 **`docs/基准测试规划.md`** §6。

### 5) SWE-bench 常见问题

- **`git clone` 失败或超时**：检查网络、代理、`repo` URL；GitHub API 限流时可配置凭据或使用镜像。
- **磁盘占满**：缩小任务子集、清理旧的 `<instance_id>/` 克隆目录，或将 **`run_command_working_dir`** 指到大分区。
- **`model_patch` 为空**：Agent 未修改跟踪文件、或修改未反映为相对 HEAD 的 diff；可调提示词、允许编辑的文件范围或工具策略。
- **与 HumanEval 的差异**：SWE-bench **不要**指望使用 `humaneval_score_benchmark_results.py`；判分链路在上游 harness。

## 快速冒烟（仓库内置 tiny 夹具）

```bash
cargo run -- bench --benchmark human_eval \
  --batch fixtures/benchmark/humaneval_tiny_tasks.jsonl \
  --batch-output /tmp/humaneval_tiny_results.jsonl

python3 scripts/humaneval_score_benchmark_results.py \
  --tasks fixtures/benchmark/humaneval_tiny_tasks.jsonl \
  --results /tmp/humaneval_tiny_results.jsonl \
  --output /tmp/humaneval_tiny_scores.jsonl
```

## 前置条件

- `cargo` 与 Rust 环境可用。
- 模型调用配置可用（如 `API_KEY`、`api_base`、`model`）。
- `python3` 可用（HumanEval 判分与转换脚本依赖 Python 标准库）。
- 跑 **`swe_bench`** 时还需 **`git`** 与足够磁盘/网络（见上文「SWE-bench 流程」）。

## 推荐可复现实验参数

为便于横向对比，建议固定同一套参数（示例）：

```bash
cargo run -- bench \
  --benchmark human_eval \
  --batch benchmark/humaneval_tasks.jsonl \
  --batch-output benchmark/humaneval_results.jsonl \
  --task-timeout 120 \
  --max-tool-rounds 0
```

说明：
- `--task-timeout`：控制单题上限时间，避免单题挂死拖垮整批。
- `--max-tool-rounds 0`：HumanEval 场景通常不需要工具循环，可减少不稳定因素。
- 需要可比结果时，务必记录 `model`、`api_base`、`temperature` 与代码提交哈希。

## 输出文件说明

主要产物：

- `benchmark/humaneval_results.jsonl`：`crabmate bench` 原始输出（每题一行）。
- `benchmark/humaneval_scores.jsonl`：外挂判分后每题结果（每题是否通过、错误信息等）。

建议在评测目录额外记录：
- 运行命令（完整参数）
- 运行时间
- 模型与配置（`model`、`api_base`、`temperature`）
- 当前 Git 提交哈希

## 结果汇总（快速统计）

### 方案 A：直接看判分脚本 stdout 汇总

`humaneval_score_benchmark_results.py` 默认会在 stdout 打印汇总 JSON（例如总题数、通过数、通过率）。

### 方案 B：用 Python 统计 `humaneval_scores.jsonl`

```bash
python3 - <<'PY'
import json
from pathlib import Path
p = Path("benchmark/humaneval_scores.jsonl")
rows = [json.loads(x) for x in p.read_text(encoding="utf-8").splitlines() if x.strip()]
total = len(rows)
passed = sum(1 for r in rows if r.get("passed") is True)
rate = (passed / total * 100.0) if total else 0.0
print({"total": total, "passed": passed, "pass_rate_percent": round(rate, 2)})
PY
```

## 常见问题排查

- **`API_KEY`/模型配置错误**
  - 现象：`bench` 很快失败，报鉴权或上游请求错误。
  - 处理：先用 `cargo run -- probe` / `cargo run -- models` 验证连通性与鉴权，再跑 benchmark。

- **`cargo run -- bench` 过程中超时**
  - 现象：单题中断或批次过慢。
  - 处理：提高 `--task-timeout`；先用 tiny 夹具确认链路；必要时降低并发压力（分批跑输入 JSONL）。

- **判分脚本报执行错误**
  - 现象：`humaneval_score_benchmark_results.py` 抛 Python 异常。
  - 处理：检查 `--tasks` 与 `--results` 是否一一对应；先用 `fixtures/benchmark/humaneval_tiny_*.jsonl` 冒烟验证脚本链路。

- **输出为空或条目明显不足**
  - 现象：`results.jsonl` 为空或题数不全。
  - 处理：确认输入 JSONL 每行是合法 JSON；优先跑 tiny 夹具定位是输入问题还是运行时问题；中断场景配合 `--resume`。

- **`swe_bench`：`git clone` / patch 为空 / 磁盘**
  - 处理：见上文「SWE-bench 流程」§5；通用连通性问题仍可用 `cargo run -- probe`。

## 安全注意

HumanEval 判分会执行模型生成的 Python 代码（通过 vendored `execution.py` 的 `check_correctness`）。  
请在隔离环境（容器/沙箱）中运行，不要在生产机器直接执行不可信结果。

最低建议：
- 在专用容器或临时环境中执行；
- 仅挂载任务与结果目录；
- 无必要时关闭外网访问。

## 相关文档

- `docs/基准测试规划.md`
- `docs/命令行与路由.md`（`bench` 子命令）
- `scripts/vendor/human_eval_openai/README.md`
- SWE-bench 上游（数据集与官方评测）：<https://github.com/princeton-nlp/SWE-bench>
