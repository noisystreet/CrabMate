# `ToolRunner` Legacy → Typed 迁移：现状与清单

**状态**：现状盘点 + 迁移清单（**未**承诺时间表；清单按 2026-10-27 代码快照生成）。**受众**：维护 `src/cm_tools/tools/runners.rs`、`tool_specs_registry` 及各工具实现的开发者。**语言**：中文。

**关联**：

- 工具**收敛方向**（薄封装 → `run_command`）是另一条线，不要与本文件混淆：[tool_calling_evolution.md](tool_calling_evolution.md) §8.5、[tool_thin_wrapper_candidates.md](tool_thin_wrapper_candidates.md)
- 内置工具契约：[工具说明](../工具说明.md)
- 架构与模块边界：[开发文档](../开发文档.md)

---

## 1. 三个变体与语义差异

`ToolRunner`（`src/cm_tools/tools/runners.rs`）三变体并存，是 strangler 迁移期的中间态：

| 变体 | 签名 | 成败判定 | 当前数量 |
|---|---|---|---|
| `Typed` | `fn(&str, &ToolContext<'_>) -> Result<String, ToolError>` | `Ok` 即成功；失败路径显式，`error_code` 来自 `ToolError` | **44** |
| `Legacy` | `fn(&str, &ToolContext<'_>) -> String` | 失败由 `parse_legacy_output` 从**正文关键词**推断 | **29** |
| `AsyncOnly` | 无执行体 | 仅 spec 占位（工具名 / schema / 摘要仍可用），须由异步调度执行 | **2**（`run_command`、`terminal_session`） |

**为什么要迁**：`Legacy` 的失败判定是启发式的——实现只返回一段文本，编排层靠关键词猜「这次算不算失败」。同一段输出换个措辞就可能翻车（"错误：xxx" 被写进正常结果、或 `exit 1` 的说明文字被当成成功）。`Typed` 把失败变成类型，`error_code` 可直接派生（如 `cargo_check_failed`），不再猜。

> 枚举注释里写明：**「strangler 迁移期并存；全部工具迁完后删除 `ToolRunner::Legacy`」**，且「**PR3+ 按族迁移**」。但截至本文件创建，`docs/` 下**没有**这份迁移计划的正文——本文件补上这一块。

---

## 2. 现状（2026-10-27）

73 个带 runner 的 `ToolSpec`：**Typed 44 / Legacy 29**（另有 2 个 `AsyncOnly`）。

### 2.1 按 spec 文件分布

| `specs/*.inc.rs` | Legacy | Typed | 说明 |
|---|---:|---:|---|
| `file_extra.inc.rs` | 0 | 6 | 已全迁 |
| `markdown_structured.inc.rs` | 0 | 5 | 已全迁 |
| `misc_basic.inc.rs` | 0 | 4 | 已全迁 |
| `file_core.inc.rs` | 1 | 12 | 仅 `codebase_semantic_search` 未迁 |
| `cargo_rust.inc.rs` | 1 | 5 | 4 个 cargo 主链路已迁；`rust_analyzer` 未迁（`rust_compiler_json` 已整工具删除） |
| `text_code_nav.inc.rs` | 1 | 2 | 仅 `code_nav` 未迁 |
| `basic_network.inc.rs` | 0 | 4 | 全部 Typed（`text_transform` 已于 2026-10-28 移除） |
| `github_cli.inc.rs` | 5 | 0 | 全 Legacy |
| `diagnostics_docs.inc.rs` | 4 | 4 | `long_term_*` ×3 + `summarize_experience` 已迁；`changelog_draft` / `license_notice` / `error_output_playbook` / `playbook_run_commands` 已整工具删除（见 §4.4） |
| `metrics.inc.rs` | 0 | 3 | 已全迁 |
| `process.inc.rs` | 2 | 0 | 全 Legacy |
| `schedule.inc.rs` | 2 | 0 | 全 Legacy |
| `self_config_info.inc.rs` | 0 | 2 | 已全迁（见 §4.8） |
| `archive.inc.rs` | 3 | 0 | 全 Legacy（受 `archive-tools` feature gate） |
| `frontend_python.inc.rs` | 2 | 0 | 全 Legacy |
| `frontend_audit_ci.inc.rs` | 1 | 0 | 仅 `workflow_execute`（`rust_backtrace_analyze` 已整工具删除，见 §7） |
| `format_lint.inc.rs` | 1 | 0 | 全 Legacy |
| `docs_health.inc.rs` | 1 | 0 | 全 Legacy |
| `git_read.inc.rs` | 1 | 0 | 全 Legacy |
| `exec_package.inc.rs` | 0 | 0 | 全 `AsyncOnly`（`run_command` / `terminal_session`） |
| **合计** | **29** | **44** | |

### 2.2 规律：已迁的是什么，没迁的是什么

- **已迁 44 个**几乎全是「**纯计算 / 文件 / 结构化**」：文本变换、JSON/YAML、正则、日期、表格、diff、文件读写、路径操作、`structured_*`、`markdown_check_links`、`cargo_*`。它们的失败天然是结构化的（参数不合法、路径越权、解析失败），实现里本来就有清晰的错误分支。
- **未迁 32 个**几乎全是「**网络 / 子进程 / 外部状态 / 编排**」：`gh_*`、`git`、`http_*`、`web_search`、`workflow_execute`、`background_job*`、`pytest_run`、`long_term_*`、`self_config_info`、`skill_manage`、`codebase_semantic_search`。它们的失败天然是「一段命令/HTTP 输出」，历史上直接把这段文本往上一抛。

**结论**：迁移进度不是按重要性排的，而是按「实现层是否容易表达 `Result`」自然筛出来的。剩下 32 个都要先改实现层，这是它长期停在半数以下的原因。

---

## 3. 迁移一个工具要动三处

1. **实现层**：新增 `*_try` 版本，返回 `Result<String, ToolError>`，把原先「拼一段错误字符串 `return`」的分支改成 `Err(ToolError::…)`；`Ok` 路径正文保持不变。
2. **`runners.rs`**：把条目从 Legacy 宏表挪到对应的 `_try` 孪生宏表（各形态一一对应，见 §3.1）；手写 runner 则改为返回 `Result` 并加 `#[allow(clippy::result_large_err)]`。
3. **spec**：`runner: ToolRunner::Legacy(x)` → `runner: ToolRunner::Typed(x_try)`。

### 3.1 孪生宏对照

| 形态 | Legacy 宏 | Typed 孪生 |
|---|---|---|
| `f(args, cwd, max_len)` | `define_runners_cwd_maxlen!` | `define_runners_cwd_maxlen_try!` |
| `f(args, cwd, ctx)` | `define_runners_cwd_ctx!` | `define_runners_cwd_ctx_try!` |
| `f(args, cwd)` | `define_runners_cwd!` | `define_runners_cwd_try!` |
| `f(args, &PathAccess)` | — | `define_runners_paths_try!` |
| `f(args, &PathAccess, ctx)` | — | `define_runners_paths_ctx_try!` |
| `f(args)`（args-only） | `define_runners_args_only!` | **无孪生**（4 个 misc 工具均为手写具名 runner） |
| git 族 | `define_git_runners!` | **无孪生** |
| gh 族 | `gh_runner!` | **无孪生** |

### 3.2 编译期 smoke

`runners.rs` 的 `mod try_macro_typecheck`（`#[cfg(test)]`）用**真实** `_try` 实现展开孪生宏，防止「宏配对写错但编译器不报」。目前覆盖 6 个 `_try` 函数（`read_file` / `search_in_files` / `glob_files` / `list_tree` / `read_dir` / `read_binary_meta`）。

新增孪生宏时**应同时在 smoke 里挂一个真实实现**，否则宏本身没有任何类型校验。

---

## 4. 32 个 Legacy 清单

按 spec 文件分组。**「形态」** 指 runner 的转发形态（决定改法），**「目标实现」** 指 runner 最终调用的函数。

### 4.1 `metrics.inc.rs`（3）— 形态 `cwd_maxlen`，纯读，最低风险

| 工具 | 目标实现 | 备注 |
|---|---|---|
| ~~`code_stats`~~ / ~~`dependency_graph`~~ / ~~`coverage_report`~~（**已迁 2026-10-27**） | 三个均已改 `*_try` | 口径：「没有这个东西」（无源码文件 / 无构建清单 / 未找到覆盖率文件）仍返回 `Ok`（合法答案，模型可据此换路径）；参数与路径问题才 `Err(invalid_args)`，读盘失败 → `coverage_report_io_failed` |

### 4.2 `archive.inc.rs`（3）— 形态 `cwd_ctx`，受 `archive-tools` feature gate

| 工具 | 目标实现 |
|---|---|
| `archive_pack` | `archive::archive_pack` |
| `archive_unpack` | `archive::archive_unpack` |
| `archive_list` | `archive::archive_list` |

### 4.3 `github_cli.inc.rs`（5）— 形态 `gh_runner!`（参数为 `GhCtx`）

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `gh_pr` | `github_cli::gh_pr` | action 分发器（7 个动作） |
| `gh_run` | `github_cli::gh_run` | action 分发器（2 个动作） |
| `gh_issue_create` | `github_cli::gh_issue_create` | |
| `gh_release_create` | `github_cli::gh_release_create` | |
| `gh_api` | `github_cli::gh_api` | 校验密集（path 白名单 + method 白名单 + body JSON） |

> 共同阻碍：这族底层统一走 `run_gh_vec` → `command::run`，返回的是一段格式化命令输出，失败靠正文判定。要迁 Typed，须让 `run_gh_vec` 能区分「`gh` 非零退出」与「参数构造/解析失败」。

### 4.4 `diagnostics_docs.inc.rs`（8）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| `diagnostic_summary` | 手写 | `diagnostics::diagnostic_summary(args, cwd, &[])` | |
| `repo_overview_sweep` | `cwd_maxlen` | `repo_overview::repo_overview_sweep` | |
| `crate_contract_map` | 手写 | `contract_map::crate_contract_map(args, ctx)` | |
| ~~`long_term_remember`~~ / ~~`long_term_forget`~~ / ~~`long_term_memory_list`~~ / ~~`summarize_experience`~~（**已迁 2026-10-27**） | 手写 | `dispatch_long_term_tool(...)` | 四个共用入口，一次改造覆盖；`LongTermMemoryToolHost::dispatch` 契约改为 `Result<String, ToolError>`；缺 `cfg` / host → `tool_context_incomplete`，记忆库 IO → `long_term_memory_io_failed` |
| `present_clarification_questionnaire` | `args_only` | `clarification_questionnaire::run_present_clarification_questionnaire` | 控制面，非普通工具 |

> `long_term_*` 四个共用 `dispatch_long_term_tool`，一次改造覆盖四个（已落地，见上）。**顺带修掉一个真缺陷**：旧实现里 JSON 解析失败返回 `参数 JSON 无效: …`，**没有「错误：」前缀**，会被 `parse_legacy_output` 启发式判成**成功**——即模型传错参数时拿到的是「成功」结果。迁 Typed 后该路径稳定为 `invalid_args`。

### 4.5 `basic_network.inc.rs`（4）— 全手写

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `get_weather` | `weather::run(args, ctx.weather_timeout_secs)` | 外部 API 失败 |
| `web_search` | `web_search::run(args, ctx)` | 外部 API / 浏览器后端 |
| `http_fetch` | `http_fetch::run_direct(args, ctx)` | feature `web-fetch-tools`；含审批语义 |
| `http_request` | `http_fetch::run_request_direct(args, ctx)` | 同上，含 METHOD 键 |

> 这族的「失败」包含**需要人工审批**这类中间态——审批不通过不算 `ToolError`，迁移时须区分「未批准」与「执行失败」。

### 4.6 `process.inc.rs`（2）— 手写，带 host

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `background_job` | `background_job_tools::background_job(args, host, cwd, max_len)` | 需 `ctx.tool_jobs_host` |
| `background_job_cancel` | `background_job_tools::background_job_cancel(args, host)` | 写副作用（保持独立工具） |

### 4.7 `schedule.inc.rs`（2）— 形态 `cwd`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `reminder` | `schedule_dispatch::reminder` | action 选择器（5 个动作）；`list` 只读、其余写 `.crabmate/*.json` |
| `event` | `schedule_dispatch::event` | action 选择器（4 个动作） |

### 4.8 `self_config_info.inc.rs`（2）— 手写，需 `ctx.cfg`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| ~~`self_config_info`~~（**已迁 2026-10-27**） | `self_config_info::self_config_info(cfg, sections)` | 已改 `runner_self_config_info_try`：缺 `cfg` → `tool_context_incomplete`（归 `Workspace`），`sections` 未匹配 → `invalid_args`；正文文案与旧实现逐字一致 |
| ~~`skill_manage`~~（**已迁 2026-10-27**） | `skill_manage::skill_manage(parsed, &cfg.skills, cwd)` | 内部 8 个 `Result<_, String>` helper 全部换成 `Result<_, ToolError>`：参数/目标类 → `invalid_args`，目录与文件 IO → 新增 `skill_manage_io_failed`。**测试断言一行未改**：helper 统一取 `e.message`，文案逐字不变 |

> **修正（2026-10-27，实测）**：原先估计「两个都几乎零成本」只对 `self_config_info` 成立——它只有 runner 层两个分支。`skill_manage` 内部虽已大量使用 `Result<T, String>`（`install_plan` / `install_layer` / `install_conflicts` / `write_skill_file` / `scan_entries` / `resolve_remove_target`），但错误类型是 `String`，**换类型要触及约 20 个错误点 + 3 处测试断言**，工作量与本档其余项不是一个量级，故单独切片。

### 4.9 `file_core.inc.rs`（1）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| `codebase_semantic_search` | 手写 | `ctx.codebase_semantic_host.run_search(...)` | 需注入 host + `fastembed` feature；未注入时直接返回错误串 |

### 4.10 `cargo_rust.inc.rs`（1）— 形态 `cwd`

| 工具 | 形态 | 目标实现 |
|---|---|---|
| `rust_analyzer` | `cwd` | `rust_ide::rust_analyzer` |

### 4.11 `text_code_nav.inc.rs`（1）— 形态 `cwd`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `code_nav` | `code_nav_dispatch::dispatch` | action 选择器（4 个动作） |

### 4.12 `git_read.inc.rs`（1）— 形态 `define_git_runners!`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `git` | `git::read_ops::dispatch(args, max_len, cwd)` | action 选择器（10 个动作）；与 `gh_*` 同类问题：底层是命令输出 |

### 4.13 `frontend_python.inc.rs`（2）— 手写

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `pytest_run` | `python_tools::pytest_run(...)` | 关联后台任务装配（`background_job_async_tools`） |
| `python_snippet_run` | `python_tools::python_snippet_run(...)` | 任意代码执行 |

### 4.14 `frontend_audit_ci.inc.rs`（2）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| `workflow_execute` | 手写 **stub** | 不执行，返回提示串 | 由 runtime 在 `run_agent_turn` 拦截，**迁移意义存疑**：同步面本就没有执行体，更接近 `AsyncOnly` |

### 4.15 `format_lint.inc.rs` / `docs_health.inc.rs`（各 1）— 形态 `cwd_maxlen`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `docs_health_sweep` | `docs_health_sweep::docs_health_sweep` | 聚合器（预览 + 拼写 + 链接） |

---

## 5. 建议切片顺序

按「实现层改动量 ÷ 风险」排，前两档可直接动手：

| 档 | 族 | 理由 |
|---|---|---|
| **A（低风险，建议先做）** | `metrics` 3 个、`self_config_info`、`long_term_*` 4 个、`skill_manage`（**全部已迁，A 档已清空**） | 纯读或纯本地；错误分支已存在，只需从「返回串」改成 `Err`；无外部进程、无后台装配 |
| **B（中风险）** | `archive` 3 个、`docs_health` 1 个聚合器、`code_nav` / `reminder` / `event` 3 个 action 选择器 | action 选择器需在分发层做「未知 action」的错误收敛；feature gate 需在 CI 覆盖 |
| **C（高风险，须先设计）** | `gh_*` 5 个、`git`、`http_fetch` / `http_request`、`web_search` / `get_weather`、`background_job*`、`pytest_run` | 失败语义混在命令/HTTP 输出里；`http_*` 还有「未审批」这一中间态；`pytest_run` 与后台任务装配耦合 |
| **D（不建议迁）** | `workflow_execute`（同步面是 stub，宜改 `AsyncOnly`）、`codebase_semantic_search`（依赖 host 注入 + feature） | 迁移不带来收益或受编译条件限制 |

**落地约定**：一个切片 = 一族 = 一个 PR；每族改完须同步 `runners.rs` 的 smoke 覆盖（§3.2），并跑 `pre-commit run --all-files` + 该族相关测试。

---

## 6. 已知坑

1. **错误码变化**：`Legacy` → `Typed` 后，`error_code` 从正文推断变为显式，可能**改变**已有错误码取值（如从 `tool_failed` 变成 `xxx_invalid_args`）。凡是按 `error_code` 分支的逻辑（重试策略、经验提炼）都要复查。
2. **`Ok` 路径必须逐字不变**：迁移只改失败路径；正文一旦变化，`fixtures/` 与 SSE 金样可能失配。
3. **手写 runner 里的隐式失败**：`self_config_info` / `skill_manage` / `codebase_semantic_search` 都存在「缺 `ctx.xxx` → 返回错误字符串」的分支，这些是 `Typed` 化的**首选目标**（改造成本最低、语义最正确）。
4. **`#[allow(clippy::result_large_err)]`**：所有 `Typed` runner 都要加（`ToolError` 含 legacy 解析快照），漏加会被 `-D warnings` 拦下。
5. **`AsyncOnly` 不是本文件的迁移目标**：`run_command` / `terminal_session` 跳过同步面，与 Legacy 无关。

---

## 7. 修订记录

| 日期 | 摘要 |
|---|---|
| 2026-10-27 | 初稿：按代码快照盘点 `ToolRunner` 三变体与 44/35/2 现状、按 spec 文件分布、「迁一个工具要动三处」与孪生宏对照、44 个逐条清单、四档切片建议与 5 条已知坑。此前该迁移计划只存在于 `runners.rs` 的注释里（「PR3+ 按族迁移」），`docs/` 无对应正文。 |
| 2026-10-27 | **A 档首项落地**：`self_config_info` 迁为 `Typed`（`runner_self_config_info_try`），Legacy 44 → 43；新增错误码 `tool_context_incomplete`（归 `ToolFailureCategory::Workspace`，用于「`ToolContext` 未注入该工具所需运行时」这一类）。基线守卫测试 `builtin_legacy_runner_count_matches_baseline` 同步 44/41 → 43/40。同项的 `skill_manage` 经实测**不属低风险**（内部 `Result<_, String>` 约 20 处），改为单独切片，§4.8 与 §5 已如实修正。 |
| 2026-10-27 | **A 档第二项落地**：`long_term_remember` / `long_term_forget` / `long_term_memory_list` / `summarize_experience` 四个迁为 `Typed`，Legacy 43 → 39。改动穿过三层：`cm_internal/long_term_memory_tools.rs`（4 个函数改返回 `Result<String, ToolError>`）、trait `LongTermMemoryToolHost::dispatch`（`String` → `Result`，唯一实现者同步）、`runners::dispatch_long_term_tool` 与 4 个 `*_try` runner。新增错误码 `long_term_memory_io_failed`（`External`）。**顺带修缺陷**：JSON 解析失败原返回「`参数 JSON 无效: …`」（无「错误：」前缀），被启发式判成成功；现稳定为 `invalid_args`。基线 43/40 → 39/36。 |
| 2026-10-27 | **A 档第三项落地**：`metrics` 族三个（`code_stats` / `coverage_report` / `dependency_graph`）迁为 Typed，Legacy 39 → 36。`code_stats` 与 `coverage_report` 在 `code_metrics.rs`，`dependency_graph` 在 `code_metrics_dependency_graph.rs`（经 `code_metrics` re-export），三者的 `*_try` 走同一孪生宏 `define_runners_cwd_maxlen_try!`。新增错误码 `coverage_report_io_failed`（`External`）。同时更新 `part_basic.inc.rs` 里 7 处指向旧函数名的 intra-doc link（否则 `cargo doc -D warnings` 失败）。基线 39/36 → 36/33。 |
| 2026-10-27 | **A 档第四项落地（A 档清空）**：`skill_manage` 迁为 Typed，Legacy 36 → 35。内部 8 个 `Result<_, String>` helper（`install_plan` / `install_layer` / `install_conflicts` / `write_skill_file` / `install_action` / `remove_action` / `remove_pick_target` / `remove_delete` / `layer_dir` / `validate_skill_id`）全部换成 `Result<_, ToolError>`；新增错误码 `skill_manage_io_failed`（`External`）。**原本预估要改 3 处 `starts_with("错误：")` 断言，实际一处未改**——测试 helper 改为成功取 `Ok`、失败取 `e.message`，文案逐字未变，原有 `contains` / `starts_with` 口径继续成立。基线 36/33 → 35/32。 |
| 2026-10-27 | **E 档：两个工具整工具删除**（`changelog_draft` / `license_notice`）—— 不属任何迁移档，直接退场：Legacy 35 → **33**（实现文件 `release_docs.rs` 整文件移除，含 9 个无其它消费者的 helper）。平替：`gh_release_create` 的 `auto_notes` / `run_command` 的 `git log --pretty` / `cargo deny check licenses`（明细表可走 `cargo metadata` + `jq`）。§2.1 表与 §4.4 清单同步为 6 个（`diagnostics_docs.inc.rs`）。 |
| 2026-10-27 | **F 档：`rust_compiler_json` 整工具删除**—— 同样不属迁移档：其 JSON 输出里的 `message.rendered` 就是 cargo 自己的人类可读块，spans / children 只是重复它，净增量仅「逐条编号 + `max_diagnostics` 上限」，与只读的 `cargo_check` 重合。Legacy 33 → **32**（`rust_ide.rs` 去掉约 230 行 cargo/rustc JSON 区块及其 smoke test）。同时从 `builtin_parallel_sync_denied_exact` 移除（它本就被排除并行只读批，因会起 cargo 子进程）。§2.1 与 §4.10 同步降为 1 个。 |
| 2026-10-27 | **整工具删除：`rust_backtrace_analyze`** —— 不属迁移档：纯计算分析器（`debug_tools.rs` 整文件 100 行、单函数、无配置键、无外部调用），模型可直读 panic 文本，或用 `run_command`（`c++filt` 在默认白名单）+ `code_nav` 定位帧。Legacy 32 → **31**（基线守卫同步 32/29 → 31/28）；同时从 `tests.rs` 的 `build_tools_names` 移除正断言、加入「已移除不应复活」名单。§2.1 表 `frontend_audit_ci.inc.rs` 降为 1，并补上此前缺失的 `exec_package.inc.rs` 行（2 个 `AsyncOnly`），§4.14 同步。 |
| 2026-10-27 | **整工具删除：`error_output_playbook` / `playbook_run_commands`** —— 两者共用「生态 × 类别」固定查表模板（`error_playbook.rs` 整文件 627 行），建议不是对具体错误的推理：只读那半不执行命令、执行那半又让调用方看不见将要跑什么，模型自行归类后直接调 `run_command` 更透明。Legacy 31 → **29**（基线守卫 29/26）；同步移除 `registry_policy` 的写副作用名单与并行同步拒绝名单、`codebase_semantic_invalidation` 两处、`dev_tag`、`tool_summary` 动态摘要、两个参数结构体、一个白名单过滤测试。同时改写 `AGENTS.md` 与 `tool_calling_evolution.md` 里「缺 CLI 安装提示由 `error_output_playbook` 提供」的旧论证。 |
