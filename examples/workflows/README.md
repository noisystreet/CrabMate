# 工作流示例（workflow_author v2）

仓库内示例，路径：**`examples/workflows/`**。在仓库根作为工作区时可：

```bash
cargo run -- workflow validate examples/workflows/ci.yaml
cargo run -- workflow run examples/workflows/serial_check.yaml
```

复制到项目工作区时，可放到 **`.crabmate/workflows/`** 或任意相对路径，再通过 `workflow_file` 引用。

**教程**：[docs/工作流编写教程.md](../../docs/工作流编写教程.md)  
**回归夹具**：[fixtures/workflows/](../../fixtures/workflows/)（含更多边界用例）

| 文件 | 说明 |
|------|------|
| `ci.yaml` | 内置模板 `rust_ci_light` |
| `serial_check.yaml` | 串行：diff → clippy → test |
| `on_failure_test.yaml` | `when`：仅 clippy 失败时跑 test |
| `repeat_flaky_test.yaml` | `repeat`：最多 3 次 test |
| `code_review.yaml` | 内置模板 `code_review` |
| `for_each_static.yaml` | `for_each` + `static_items` |
| `choice_lint.yaml` | `kind: choice` 语法糖 |
| `ci_from_markdown.md` | Markdown `` ```crabmate-workflow `` 围栏 |

`doctor` 默认扫描工作区下的 **`.crabmate/workflows/`**，不扫描本 `examples/` 目录。
