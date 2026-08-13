# Benchmark 基线记录

记录 `crabmate bench` 的**可复现对照**（模型、子集、日期、分数）。**不要**把 API 密钥、完整 `Authorization`、或含密钥的结果 JSONL 提交进仓库。

跑法见 **`benchmark/README.md`** 与 **`docs/基准测试规划.md`** §5。

## HumanEval（tiny 冒烟）

仓库夹具 **`fixtures/benchmark/humaneval_tiny_tasks.jsonl`**（1 题 `tiny/add`），**不是**官方 164 题，不能与论文 pass@k 对比。

| 日期 (UTC+8) | git | 模型 | 旗标 | 通过 / 计分 | pass_rate | 备注 |
|---|---|---|---|---|---|---|
| 2026-08-13 | `feat/bench-humaneval-prompt`（基于 `a38255ed`） | `deepseek-v4-flash` | 全局 `--no-tools` | 1 / 1 | 1.0 | 适配器续写指令 + `extract_humaneval_completion`；墙钟约 3.2s；外挂 `scripts/humaneval_score_benchmark_results.py` |

合入 `main` 后请把 **git** 列改成该提交的短 SHA。官方子集与 pass@k 另开行，勿覆盖本行。
