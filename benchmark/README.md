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
- `python3` 可用（判分与转换脚本依赖 Python 标准库）。

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
