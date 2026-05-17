# 项目 CI（Markdown 作者层示例）

本页正文仅供人读；**执行时只解析下方 `crabmate-workflow` 围栏内的 YAML**。

在合并前可本地校验：

```bash
cargo run -- workflow validate examples/workflows/ci_from_markdown.md
```

```crabmate-workflow
version: 2
workflow:
  fail_fast: true
  workflow_template: rust_ci_light
```

也可改用内联 `steps`，参见 [工作流编写教程.md](../../docs/工作流编写教程.md) 与 `serial_check.yaml`。
