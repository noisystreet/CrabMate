# workflow_spec v2 示例夹具

**文档**：用户向教程见 **`docs/工作流编写教程.md`**；设计稿见 **`docs/工作流Markdown作者层设计.md`** §6.1–§6.3。用户向示例见 **`examples/workflows/`**。

| 文件 | 说明 |
|------|------|
| `01_serial_after.yaml` | 仅 `after` 串行 |
| `02_branch_when_success_failure.yaml` | `when.branch` 成功/失败二分支 |
| `03_branch_when_match.yaml` | `when.match` 多路分支 |
| `04_loop_for_each.yaml` | `for_each` + `max_items`（`json_path` 运行时展开） |
| `05_loop_repeat.yaml` | `repeat` 有界重试（`stop_on: success`） |
| `06_branch_and_loop_combined.yaml` | 分支 + `for_each` + `repeat` 组合 |
| `07_choice_node.yaml` | `kind: choice` 语法糖 |
| `08_nodes_only_today.yaml` | 已是 `workflow.nodes`（无 `steps` 编译） |
| `09_fenced_in_markdown.md` | Markdown 围栏提取 + 模板 |

每个 `*.yaml` / `*.md`（除仅作手写的说明外）配有同名 **`*.expected.json`**；回归测试：`cargo test golden_workflow_compile`。

## 本地试用

```bash
# 编译 steps → workflow JSON（stdout）
cargo run -- workflow compile fixtures/workflows/01_serial_after.yaml

# 校验 DAG + 工具参数 schema
cargo run -- workflow validate fixtures/workflows/01_serial_after.yaml

# 工作区默认 CI（仓库根）
cargo run -- workflow validate examples/workflows/ci.yaml

# Markdown 围栏
cargo run -- workflow validate fixtures/workflows/09_fenced_in_markdown.md

# doctor 会扫描工作区 .crabmate/workflows/*.{yaml,yml,md}
cargo run -- doctor
```

**已支持**：`when`（`run_if` + choice 剪枝）、`for_each`（`static_items` 编译期展开；`json_path` 写入 `for_each_pending`）、`kind: choice`、`repeat`（`count` + `stop_on: success|never`）。**不支持**：无界 `while`。

**契约**：根级 **`version: 2`**；steps / nodes 二选一。Schema 见 **`schema/`** 子目录；`workflow validate --json` 返回 `author_spec_version` 与 `author_mode`。
