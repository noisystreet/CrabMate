# Benchmark 基线记录

记录 `crabmate bench` 的**可复现对照**（模型、子集、日期、分数）。**不要**把 API 密钥、完整 `Authorization`、或含密钥的结果 JSONL 提交进仓库。

跑法见 **`benchmark/README.md`** 与 **`docs/基准测试规划.md`** §5。

## HumanEval（tiny 冒烟）

仓库夹具 **`fixtures/benchmark/humaneval_tiny_tasks.jsonl`**（1 题 `tiny/add`），**不是**官方 164 题，不能与论文 pass@k 对比。

| 日期 (UTC+8) | git | 模型 | 旗标 | 通过 / 计分 | pass_rate | 备注 |
|---|---|---|---|---|---|---|
| 2026-08-13 | `feat/bench-humaneval-prompt`（基于 `a38255ed`） | `deepseek-v4-flash` | 全局 `--no-tools` | 1 / 1 | 1.0 | 适配器续写指令 + `extract_humaneval_completion`；墙钟约 3.2s；外挂 `scripts/humaneval_score_benchmark_results.py` |

合入 `main` 后请把 **git** 列改成该提交的短 SHA。官方子集与 pass@k 另开行，勿覆盖本行。

## HumanEval（官方子集 · 前 30 题）

官方 HumanEval 前 **30** 题（`HumanEval/0`..`HumanEval/29`，**非**全量 164 题），用于建立可引用的「官方子集」pass@1 基线。公开 benchmark 存在模型可能见过题的反泄漏风险（见 **`docs/评测任务集设计.md`** §2）。

| 日期 (UTC+8) | git | 模型 | 旗标 | 通过 / 计分 | pass@1 | 备注 |
|---|---|---|---|---|---|---|
| 2026-08-14 | `9d3e7c90`（`feat/run-command-bash-c`） | `deepseek-v4-flash`（`https://api.deepseek.com/v1`，temperature 0.7 默认） | 全局 `--no-tools`；`--max-tool-rounds 0`；`--task-timeout 120` | 30 / 30 | 1.0 | 前 30 题多为简单题；判分 `scripts/humaneval_score_benchmark_results.py`（vendored `check_correctness`，pass@1） |

## HumanEval（官方全量 164 题）

官方 HumanEval 全量 **164** 题（`HumanEval/0`..`HumanEval/163`）。公开 benchmark 存在模型可能见过题的反泄漏风险（见 **`docs/评测任务集设计.md`** §2），该分数为单次采样 **pass@1**，不宜当作严格无泄漏的泛化度量。

| 日期 (UTC+8) | git | 模型 | 旗标 | 通过 / 计分 | pass@1 | 备注 |
|---|---|---|---|---|---|---|
| 2026-08-14 | `9d3e7c90`（`feat/run-command-bash-c`） | `deepseek-v4-flash`（`https://api.deepseek.com/v1`，temperature 0.7 默认） | 全局 `--no-tools`；`--max-tool-rounds 0`；`--task-timeout 120` | 151 / 164 | 0.9207 | 判分 `scripts/humaneval_score_benchmark_results.py`（vendored `check_correctness`，pass@1） |
