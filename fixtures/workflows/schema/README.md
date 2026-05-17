# workflow_author JSON Schema（v2）

| 文件 | 模式 | 说明 |
|------|------|------|
| `workflow_author_v2_steps.schema.json` | **steps** | 顶层或 `workflow` 内 `steps[]`，编译为 `nodes` |
| `workflow_author_v2_nodes.schema.json` | **nodes** | `workflow.nodes` 或 `workflow.workflow_template` |

两种模式均要求根对象 **`version: 2`**（整数）。校验入口：`compile_workflow_author_yaml` / `workflow validate` / `doctor`（工作流段）。

运行时由 `src/agent/workflow/author_validate.rs` 加载并执行 `jsonschema` 校验。
