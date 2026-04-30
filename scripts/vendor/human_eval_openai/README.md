# Vendored OpenAI HumanEval `execution.py`

This directory contains a verbatim copy of
[`human_eval/execution.py`](https://github.com/openai/human-eval/blob/master/human_eval/execution.py)
from the OpenAI HumanEval repository (MIT-licensed upstream).

It is imported as package `human_eval_openai` by `scripts/humaneval_score_benchmark_results.py`
so scoring works **without** installing the `human-eval` PyPI package.

**Security:** `check_correctness` executes model-generated Python. Use an isolated environment or
sandbox when scoring untrusted completions. See the disclaimer at the top of `execution.py`.

When updating from upstream, replace `execution.py`, refresh this note, and run the scoring
script smoke test documented in `docs/基准测试规划.md`.
