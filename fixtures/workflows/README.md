# workflow_spec v2 示例夹具

**状态**：作者层 / `compile_spec` **设计稿**；除 `08_nodes_only_today.yaml` 外，**不能**直接交给当前 `parse_workflow_spec`（仅认 `workflow.nodes`）。

**文档**：`docs/工作流Markdown作者层设计.md` §6.1–§6.3。

| 文件 | 说明 |
|------|------|
| `01_serial_after.yaml` | 仅 `after` 串行；有 `01_serial_after.expected.json` |
| `02_branch_when_success_failure.yaml` | `when.branch` 成功/失败二分支 |
| `03_branch_when_match.yaml` | `when.match` 多路分支 |
| `04_loop_for_each.yaml` | `for_each` + `max_items` |
| `05_loop_repeat.yaml` | `repeat` 固定次数 |
| `06_branch_and_loop_combined.yaml` | 分支 + 循环组合 |
| `07_choice_node.yaml` | `kind: choice` 语法糖 |
| `08_nodes_only_today.yaml` | **今日可用** `workflow.nodes` 形态 |
| `09_fenced_in_markdown.md` | Markdown 围栏提取示例 |

实现 `compile_spec` 后：为各 YAML 补充 `*.expected.json`，并增加 `compile_golden` 测试。

## 本地试用（MVP）

```bash
# 编译 steps → workflow JSON（stdout）
cargo run -- workflow compile fixtures/workflows/01_serial_after.yaml

# 校验 DAG + 工具参数 schema
cargo run -- workflow validate fixtures/workflows/01_serial_after.yaml

# Markdown 围栏（见 09_fenced_in_markdown.md）
cargo run -- workflow validate fixtures/workflows/09_fenced_in_markdown.md
```

**MVP 已支持**：`when`（编译为 `run_if`，运行时 choice 剪枝 + `skipped` trace）、`for_each`（`static_items` 编译期展开；`json_path` 写入 `for_each_pending` 并在前驱完成后运行时展开）、`kind: choice`（编译期展平为带 `when` 的 steps）。**仍未实现**：`repeat`、无界 `while`。
