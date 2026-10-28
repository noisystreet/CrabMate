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
| `Typed` | `fn(&str, &ToolContext<'_>) -> Result<String, ToolError>` | `Ok` 即成功；失败路径显式，`error_code` 来自 `ToolError` | **48** |
| `Legacy` | `fn(&str, &ToolContext<'_>) -> String` | 失败由 `parse_legacy_output` 从**正文关键词**推断 | **25** |
| `AsyncOnly` | 无执行体 | 仅 spec 占位（工具名 / schema / 摘要仍可用），须由异步调度执行 | **2**（`run_command`、`terminal_session`） |

**为什么要迁**：`Legacy` 的失败判定是启发式的——实现只返回一段文本，编排层靠关键词猜「这次算不算失败」。同一段输出换个措辞就可能翻车（"错误：xxx" 被写进正常结果、或 `exit 1` 的说明文字被当成成功）。`Typed` 把失败变成类型，`error_code` 可直接派生（如 `cargo_check_failed`），不再猜。

> 枚举注释里写明：**「strangler 迁移期并存；全部工具迁完后删除 `ToolRunner::Legacy`」**，且「**PR3+ 按族迁移**」。但截至本文件创建，`docs/` 下**没有**这份迁移计划的正文——本文件补上这一块。

---

## 2. 现状（2026-10-27）

> **计数已过期**：本节的 44/29 与 §2.1 的分表是 2026-10-27 的快照。以基线守卫
> `builtin_legacy_runner_count_matches_baseline` 为准，**当前 Legacy = 4**
> （2026-10-28：`reminder` / `event`、`background_job*`、`workflow_execute`、`rust_analyzer`、
> `gh_pr` / `gh_run` / `gh_issue_create` / `gh_release_create` / `gh_api`、`git`、
> `pytest_run` / `python_snippet_run`、`diagnostic_summary` / `repo_overview_sweep` /
> `crate_contract_map`、`get_weather` / `web_search`、`http_fetch` / `http_request`
> 陆续迁完后，23 → 19 → 14 → 13 → 11 → 8 → 6 → 4）。
> §2.1 各行的「已迁 / 未迁」结论仍可参考，但数字不要再引用。

73 个带 runner 的 `ToolSpec`：**Typed 44 / Legacy 29**（另有 2 个 `AsyncOnly`）。

### 2.1 按 spec 文件分布

| `specs/*.inc.rs` | Legacy | Typed | 说明 |
|---|---:|---:|---|
| `file_extra.inc.rs` | 0 | 6 | 已全迁 |
| `markdown_structured.inc.rs` | 0 | 2 | 已全迁（`structured_*` 四个于此合并为单个 `structured`，2026-10-28，5 → 2） |
| `misc_basic.inc.rs` | 0 | 4 | 已全迁 |
| `file_core.inc.rs` | 1 | 12 | 仅 `codebase_semantic_search` 未迁 |
| `cargo_rust.inc.rs` | 0 | 6 | 已全迁（`rust_analyzer` 于 2026-10-28 迁 Typed，见 §4.10；`rust_compiler_json` 已整工具删除） |
| `text_code_nav.inc.rs` | 1 | 2 | 仅 `code_nav` 未迁 |
| `basic_network.inc.rs` | 0 | 4 | 已全迁（`get_weather` / `web_search` 于 2026-10-28 迁 Typed，见 §4.17；`http_fetch` / `http_request` 同日迁 Typed，见 §4.18。`text_transform` 已于 2026-10-28 移除） |
| `github_cli.inc.rs` | 0 | 5 | 已全迁（2026-10-28，C 档；见 §4.3） |
| `diagnostics_docs.inc.rs` | 1 | 7 | `long_term_*` ×3 + `summarize_experience` 已迁；`diagnostic_summary` / `repo_overview_sweep` / `crate_contract_map` 于 2026-10-28 迁 Typed（见 §4.16），仅剩 `present_clarification_questionnaire`；`changelog_draft` / `license_notice` / `error_output_playbook` / `playbook_run_commands` 已整工具删除 |
| `metrics.inc.rs` | 0 | 3 | 已全迁 |
| `process.inc.rs` | 0 | 2 | 已全迁（2026-10-28：`background_job` / `background_job_cancel` 走手写 try runner；原先 `参数错误：…` 开头被判成功） |
| `schedule.inc.rs` | 0 | 2 | 已全迁（2026-10-28，`reminder` / `event` 走 `define_runners_cwd_try!`） |
| `self_config_info.inc.rs` | 0 | 2 | 已全迁（见 §4.8） |
| `archive.inc.rs` | 0 | 3 | 已全迁（2026-10-28，B 档首段，受 `archive-tools` feature gate） |
| `frontend_python.inc.rs` | 0 | 2 | 已全迁（2026-10-28，C 档；见 §4.13） |
| `frontend_audit_ci.inc.rs` | 0 | 1 | 已全迁（2026-10-28：`workflow_execute` 同步面占位改 Typed，返回 `workflow_execute_not_intercepted`；`rust_backtrace_analyze` 已整工具删除，见 §7） |
| `docs_health.inc.rs` | 1 | 0 | 全 Legacy |
| `git_read.inc.rs` | 0 | 1 | 已全迁（2026-10-28，C 档；见 §4.12） |
| `exec_package.inc.rs` | 0 | 0 | 全 `AsyncOnly`（`run_command` / `terminal_session`） |
| **合计** | — | — | 表内各行是 2026-10-27 快照且已局部过期（且与 `AsyncOnly` 混算），**不要引用合计**；以基线守卫 `builtin_legacy_runner_count_matches_baseline` 为准（当前 Legacy = 8） |

### 2.2 规律：已迁的是什么，没迁的是什么

- **已迁 44 个**几乎全是「**纯计算 / 文件 / 结构化**」：文本变换、JSON/YAML、正则、日期、表格、diff、文件读写、路径操作、`structured`（原 `structured_*` 四个已合并为单工具 + `action`）、`cargo_*`。它们的失败天然是结构化的（参数不合法、路径越权、解析失败），实现里本来就有清晰的错误分支。
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
| `f(args, cwd, max_len)` | `define_runners_cwd_maxlen!`（无实例，已删除） | `define_runners_cwd_maxlen_try!` |
| `f(args, cwd, ctx)` | `define_runners_cwd_ctx!` | `define_runners_cwd_ctx_try!` |
| `f(args, cwd)` | `define_runners_cwd!` | `define_runners_cwd_try!` |
| `f(args, &PathAccess)` | — | `define_runners_paths_try!` |
| `f(args, &PathAccess, ctx)` | — | `define_runners_paths_ctx_try!` |
| `f(args)`（args-only） | `define_runners_args_only!`（无实例，已删除） | `define_runners_args_only_try!` |
| git 族 | `define_git_runners!`（迁完后无实例，已删除） | `define_git_runners_try!` |
| gh 族 | `gh_runner!` | **无孪生** |

### 3.2 编译期 smoke

`runners.rs` 的 `mod try_macro_typecheck`（`#[cfg(test)]`）用**真实** `_try` 实现展开孪生宏，防止「宏配对写错但编译器不报」。目前覆盖 6 个 `_try` 函数（`read_file` / `search_in_files` / `glob_files` / `list_tree` / `read_dir` / `read_binary_meta`）、git 族的 `define_git_runners_try!`（`git::dispatch`），以及 args-only 形态的 `define_runners_args_only_try!`（`present_clarification_questionnaire`，2026-10-28 补）。

新增孪生宏时**应同时在 smoke 里挂一个真实实现**，否则宏本身没有任何类型校验。

---

## 4. 32 个 Legacy 清单

按 spec 文件分组。**「形态」** 指 runner 的转发形态（决定改法），**「目标实现」** 指 runner 最终调用的函数。

### 4.1 `metrics.inc.rs`（3）— 形态 `cwd_maxlen`，纯读，最低风险

| 工具 | 目标实现 | 备注 |
|---|---|---|
| ~~`code_stats`~~ / ~~`dependency_graph`~~ / ~~`coverage_report`~~（**已迁 2026-10-27**） | 三个均已改 `*_try` | 口径：「没有这个东西」（无源码文件 / 无构建清单 / 未找到覆盖率文件）仍返回 `Ok`（合法答案，模型可据此换路径）；参数与路径问题才 `Err(invalid_args)`，读盘失败 → `coverage_report_io_failed` |

### 4.2 `archive.inc.rs`（3）— **已迁 2026-10-28**

`archive_pack` / `archive_unpack` / `archive_list` 三个入口改为 `Result<String, ToolError>`
（`define_runners_paths_ctx_try!`，随 `archive-tools` feature gate）——归档的读（源目录 / 待解压
文件）与写（输出归档 / 解压目录）都过 `PathAccess`，越界与绝对路径在校验层挡住。

迁移动机不是「形状统一」，而是**静默成功**：`detect_format` 对表外后缀（`.tar.zst` /
`.tar.lz` / `.rar5` …）返回 `unknown`，三个入口都答 `不支持的归档格式: {name}`——这段正文
既无 `错误` 也无 `失败`，`parse_legacy_output` 的关键词表因此判成 **`ok: true`**，调用方
无法区分「已打包」与「不认识这个格式」。

错误码：参数 / 未知格式 → `invalid_args`；归档不存在 → **`archive_not_found`**；打包 / 解压 /
列目录失败 → **`archive_pack_failed`** / **`archive_unpack_failed`** / **`archive_list_failed`**
（四者均在 `failure_category_for_error_code` 显式登记为 `external`）。成功正文逐字未变。

归档本体的 7z / unrar 子进程失败仍属外部工具输出，未拆更细的 code。

### 4.3 `github_cli.inc.rs`（5）— **已迁 2026-10-28**（形态 `gh_runner_try!`，参数为 `GhCtx`）

| 工具 | 实现 | 目标 runner |
|---|---|---|
| `gh_pr` | `github_cli::gh_pr_try` | action 分发器（7 个动作） |
| `gh_run` | `github_cli::gh_run_try` | action 分发器（2 个动作） |
| `gh_issue_create` | `github_cli::gh_issue_create_try` | |
| `gh_release_create` | `github_cli::gh_release_create_try` | |
| `gh_api` | `github_cli::gh_api_try` | 校验密集（path 白名单 + method 白名单 + body JSON） |

**原先的共同障碍**：这族底层统一走 `run_gh_vec` → `command::run`，返回的是一段格式化命令输出，
失败靠正文判定。

**怎么解开的**：`CommandOutcome` 本来就带 `status: ExitStatusKind`，`status.exit_code()` 一直可用，
只是旧代码把它 `render` 成文本就丢掉了。于是 `run_gh_vec` 改成 `Result<String, ToolError>`：

- `parse_command_spec` / `command::run` 的 `RunCommandError` 直接 `into_tool_error()`（现成方法，
  含 `command_not_found` 的安装提示）；
- `exit_code == -1` → `timeout`（`ExitStatusKind::exit_code` 对超时/取消的历史约定）；
- 其它非零 → `gh_{子命令}_failed`，子命令取自 `argv[0]`，所以 17 个调用点签名不变，只改返回类型。

**两个不能一刀切的地方**：

1. **`gh pr checks` 的 1 / 8**：它对「有检查失败」返回 1、「仍有 pending」返回 8，stdout 仍是合法
   JSON。一律当失败会把正常的 CI 状态读成工具故障，于是拆出
   `run_gh_vec_allow_exit(argv, ctx, &[0, 1, 8])`，**只有**它用。
2. **`gh < 2.50` 的 `unknown flag: --json`**：它同样以非零退出，而 `gh_pr_checks` 原先靠「输出里
   有没有这句话」回退表格。现在改为先看 `Err.message` 再决定回退，兼容路径与提示文案都保住。

另外 `gh_api` 不经 `run_gh_vec`（body 要经 stdin 传入，自己 `spawn`），退出码判定补在
`format_gh_api_process_output` 里，码为 `gh_api_failed`。

**审计表**：`gh_*` 五条从 `LEGACY_BASELINE` 用例中删除（工具已非 Legacy），失败分支改由各文件单测
钉住：`gh_pr_edit` / `gh_pr_checks` 断言 `code == "command_not_allowed"`，`dispatch_tests::call_pr` /
`call_run` 取 `err.message` 后原断言一行未改。

### 4.4 `diagnostics_docs.inc.rs`（8）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| ~~`diagnostic_summary`~~（**已迁 2026-10-28**） | 手写 | `diagnostics::diagnostic_summary_try(args, cwd, &[])` | 只有参数 JSON 非法才是 `invalid_args`；`sections` / `extra_env_vars` 类型不对、工作区根不可解析都只降级（见 §4.16） |
| ~~`repo_overview_sweep`~~（**已迁 2026-10-28**） | `cwd_maxlen_try` | `repo_overview::repo_overview_sweep_try` | 同上；字段类型不对退回默认值、缺失路径在正文标「不存在」 |
| ~~`crate_contract_map`~~（**已迁 2026-10-28**） | 手写 | `contract_map::crate_contract_map_try(args, ctx)` | 同上；越界阈值 clamp、非法 `extra_paths` 只跳过 |
| ~~`long_term_remember`~~ / ~~`long_term_forget`~~ / ~~`long_term_memory_list`~~ / ~~`summarize_experience`~~（**已迁 2026-10-27**） | 手写 | `dispatch_long_term_tool(...)` | 四个共用入口，一次改造覆盖；`LongTermMemoryToolHost::dispatch` 契约改为 `Result<String, ToolError>`；缺 `cfg` / host → `tool_context_incomplete`，记忆库 IO → `long_term_memory_io_failed` |
| ~~`present_clarification_questionnaire`~~（**已迁 2026-10-28**） | `args_only_try` | `clarification_questionnaire::run_present_clarification_questionnaire` | 控制面，非普通工具；失败改显式 `invalid_args`，成功正文首行 `退出码：0` 不可动（见 §4.20） |

> `long_term_*` 四个共用 `dispatch_long_term_tool`，一次改造覆盖四个（已落地，见上）。**顺带修掉一个真缺陷**：旧实现里 JSON 解析失败返回 `参数 JSON 无效: …`，**没有「错误：」前缀**，会被 `parse_legacy_output` 启发式判成**成功**——即模型传错参数时拿到的是「成功」结果。迁 Typed 后该路径稳定为 `invalid_args`。

### 4.5 `basic_network.inc.rs`（4）— **已全迁 2026-10-28**

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `get_weather` | `weather::run_try(args, ctx.weather_timeout_secs)` | **已迁**；外部 API 失败见 §4.17 |
| `web_search` | `web_search::run_try(args, ctx)` | **已迁**；worbrow / Brave / Tavily 分支见 §4.17 |
| `http_fetch` | `http_fetch::run_direct(args, ctx)` → `..._try` | **已迁**；含审批语义，见 §4.18 |
| `http_request` | `http_fetch::run_request_direct(args, ctx)` → `..._try` | **已迁**；同上，含 METHOD 键 |

> 本族「未匹配白名单」是**策略拒绝**而不是执行失败：同步路径没有审批通道，`Err` 带
> `*_url_not_allowed`（`PolicyDenied`、不可重试），Web 流式 / CLI 异步路径另走 `tool_approval`。

### 4.6 `process.inc.rs`（2）— 手写，带 host

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `background_job` | `background_job_tools::background_job(args, host, cwd, max_len)` | 需 `ctx.tool_jobs_host` |
| `background_job_cancel` | `background_job_tools::background_job_cancel(args, host)` | 写副作用（保持独立工具） |

### 4.7 `schedule.inc.rs`（2）— **已迁 2026-10-28**（形态 `cwd`）

| 工具 | 目标实现 | 备注 |
|---|---|---|
| ~~`reminder`~~（**已迁**） | `schedule_dispatch::reminder` | action 选择器（5 个动作）；`list` 只读、其余写 `.crabmate/*.json` |
| ~~`event`~~（**已迁**） | `schedule_dispatch::event` | action 选择器（4 个动作） |

两族共用 `schedule.rs` 的 `ensure_data_dir` / `read_json` / `write_json`，一次改造覆盖两者，
签名统一为 `Result<String, ToolError>`（runner 侧直接进 `define_runners_cwd_try!`，无需新宏）。
**迁移动机是静默成功**：dispatch 层新增的三类必填校验（`add` 缺 `title`、缺 `id`）与
「`id` 未命中」分支的正文都不含「错误 / 失败 / 超时」，`parse_legacy_output` 因此判成
**`ok: true`**——模型传错参数却拿到「成功」。新增错误码：`invalid_args`（复用）、
**`reminder_not_found`** / **`event_not_found`**（`InvalidInput`：id 只可能来自调用方，
模型可先 `list` 自查）、**`schedule_io_failed`** / **`schedule_parse_failed`**（`External`）、
`schedule_serialize_failed`（`Internal`，由既有 `ends_with("_serialize_failed")` 分支归类）。
`list` 的「参数 JSON 坏掉 → 退化为无过滤条件」宽容语义**刻意保留**；成功正文逐字未变。

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

### 4.10 `cargo_rust.inc.rs`（0）— **已迁 2026-10-28**（形态 `cwd_try`）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| ~~`rust_analyzer`~~（**已迁**） | `cwd` | `rust_ide::rust_analyzer_try` | action 选择器（8 个 LSP 操作）；runner 是具名函数（需把 `working_dir` 透传到 LSP 子进程） |

`rust_analyzer` 是 **`KNOWN_BUG_BUDGET` 的最后一格**（1 → 0），也是「不能只改措辞」的典型：

1. **`路径无法解析: No such file or directory`**（`resolve_rel_file` 的 `canonicalize` 失败）
   ——以「路径」开头、不含「失败 / 超时」，legacy 判 `ok: true`。模型会把「文件根本不存在」
   读成「这个位置没有信息」。
2. **`rust_analyzer hover: LSP error: …`**（`lsp_response_format` 的 5 个格式化函数）
   ——同样一条关键词都不命中。审计表只盖住了第 1 处，第 2 处是迁移时读代码才发现的：
   同一工具、两种触发点、同一个故障模式。

实现层原本已有 `Result<_, String>` 骨架（`lsp_rust_analyzer_handshake` / `read_response_until_id` /
`parse_ra_lsp_json_inputs` / `load_ra_workspace_sources` / `spawn_rust_analyzer_server`），
所以这轮是「换错误类型」而不是「补错误返回」。错误码划分：

| 码 | 分类 | 触发点 |
|---|---|---|
| `invalid_args`（复用） | `InvalidInput` | 缺 / 未知 `action`、参数 JSON 非法、缺 `path` / `line` / `query`、绝对路径与 `..`、路径越界或指向非文件、文件 >512KiB |
| `rust_analyzer_file_not_found` | `External` | `canonicalize` 失败（绝大多数是目标文件不存在）——**必须显式登记分类表**，它不以 `_failed` 结尾 |
| `rust_analyzer_read_failed` | `External` | `didOpen` 前读取源文件失败 |
| `rust_analyzer_spawn_failed` | `External` | `rust-analyzer` 起不来（未安装 / 无权限 / `server_path` 错） |
| `rust_analyzer_lsp_failed` | `External` | LSP 读写 / 协议错误（`Content-Length` 缺失或非法、JSON 解析失败），以及响应自带 `error` 字段 |
| `rust_analyzer_workspace_resolve_failed` | `Workspace` | 工作区根自身不可解析（改 `args` 无用） |
| `timeout`（复用） | `Timeout` | LSP 读取超时（25s） |
| `rust_analyzer_internal` | `Internal` | `Stdio::piped()` 装配缺失导致取不到 stdin/stdout（内部不变量破坏） |

成功正文逐字未变（`format_lsp_*` 只在 `error` 分支改类型）。回归测试：
`ra_every_action_fails_on_missing_file`（8 个 action 全覆盖）、
`ra_lsp_error_response_is_a_failure`（5 个格式化函数）、
`ra_rejects_absolute_and_parent_paths`、`ra_unresolvable_workspace_is_a_workspace_error`。

### 4.11 `text_code_nav.inc.rs`（1）— 形态 `cwd`

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `code_nav` | `code_nav_dispatch::dispatch` | action 选择器（4 个动作） |

### 4.12 `git_read.inc.rs`（1）— **已迁 2026-10-28**（形态 `define_git_runners_try!`）

| 工具 | 实现 | 目标 runner |
|---|---|---|
| `git` | `git::read_ops::dispatch` → `Result<String, ToolError>` | action 选择器（10 个动作） |

**原先的共同障碍**：与 `gh_*` 同构——每个 action 最后都落到 `helpers::run_and_format` →
`output_util::run_command_output_formatted`，退出码被写进正文 `title (exit=N)` 后就丢掉。

**怎么解开的**：在 `output_util` 里新增 `run_command_output_try(cmd, …, failure_code)`，
把「渲染」抽成私有的 `render_process_output`，两个入口共用——`Ok` 与 `Err` 的正文逐字一致，
只多一层「退出码非 0 即 `Err(external_code_owned(failure_code))`」的判定。
`helpers::run_and_format` 因此只多一个 `failure_code` 参数，10 个 action 各传自己的码名。

**为什么没有豁免名单**：`gh pr checks` 的 1 / 8 那种情况在只读 git 子命令里不存在——
需要看退出码的用法（`--exit-code` 等）都是写操作、已收敛到 `run_command`，
而 `status` / `branch` / `remote` 恒为 0。**代价**要说清楚：空仓库（尚无提交）的
`log` / `show` 返回 128，现在算失败，git 自己的 `fatal: … does not have any commits yet`
仍在正文里——这是「统一按非零即失败」换来的确定性，不是疏漏。

**错误码**：

| 码 | 分类 | 触发点 |
|---|---|---|
| `invalid_args`（复用） | `InvalidInput` | 缺 / 未知 `action`、参数 JSON 非法、`path` 非相对路径或含 `..`、`mode` 非法 |
| `git_not_a_repo` | `Workspace` | `rev-parse --is-inside-work-tree` 不为 `true`（**必须显式登记**，不以 `_failed` 结尾） |
| `command_not_found`（复用） | `External` | 本机无 `git`（带安装提示） |
| `git_{action}_failed` | `External` | 子命令非零退出（`git_status_failed` / `git_diff_failed` / `git_clean_check_failed` / `git_log_failed` / `git_show_failed` / `git_blame_failed` / `git_file_history_failed` / `git_branch_list_failed` / `git_remote_status_failed` / `git_remote_list_failed`） |

**保留的行为**：成功正文逐字未变（`git status (exit=0):…`、`git clean check (exit=1)：存在未提交改动：…`
等历史文案都保留）；`ensure_git_repo` 留作 `String` 薄壳供 `github_cli::pr_body` 使用
（那边把失败包成自己的 `gh_git_failed`，不动它以免混淆两族的码）。

**测试**：`git::read_ops::tests` 从 3 条扩到 8 条——`non_repo_directory_reports_git_not_a_repo`、
`nonzero_exit_reports_git_action_failed`（`git show deadbeef` → `git_show_failed`）、
`log_in_empty_repository_is_a_failure` 与配对的 `log_after_first_commit_is_success`（证明不是把正常路径判坏）、
`status_on_repository_is_success`（钉 `Ok` 正文前缀）。审计表删掉 `git` 条目——工具已非 Legacy。

### 4.13 `frontend_python.inc.rs`（2）— **已迁 2026-10-28**（手写 runner）

| 工具 | 实现 | 目标 runner |
|---|---|---|
| `pytest_run` | `python_tools::pytest_run_try` | 关联后台任务装配（`background_job_async_tools`） |
| `python_snippet_run` | `python_tools::python_snippet_run_try` | 任意代码执行 |

**两个非失败分支必须先拆出来**，否则迁移会把「正常跳过」翻成「工具故障」：

1. `pytest_run` 在**工作区没有 Python 工程标记**时返回「pytest: 跳过（…）」。旧实现以 `Err(String)`
   从这个点返回，但正文不含「错误 / 失败」，启发式判为**成功**（审计表按 `Legal` 钉住）。
   迁移后 `build_pytest_command` 返回 `PytestCommand::{Run, Skipped}`，`Skipped` 走 `Ok(msg)`；
   后台装配（`pytest_run_background_argv`，返回 `Result<_, String>` 供装配表用）仍把它当错误——
   没有命令可跑，保持旧行为。
2. `pytest` 的 **`5`（no tests collected）**：`run_and_format_try` 因此多一个 `allowed_exit` 参数
   （`PYTEST_OK_EXITS = &[0, 5]`）。**`1` 不在其中**——测试失败与 `cargo_test` 一致归
   `pytest_failed`；`2`/`3`/`4` 同理。这一点与 `gh pr checks` 的 1/8 是不同的问题：
   那里 1/8 是「CI 状态」，这里 1 是「断言失败」。

**错误码**：`invalid_args`（缺/空 `code`、`code` 超 256 KiB、`use_uv` 缺 `pyproject.toml`、
`test_path`/`keyword`/`markers` 非法）；`workspace_resolve_failed`（工作区根不可解析，与
`path_resolve_failed` 同码）；`python_snippet_temp_file_failed`（临时脚本 IO）；
**`python_snippet_temp_path_outside_workspace`**（越界，`PolicyDenied`，显式登记）；
`command_not_found` / `spawn_failed`；`timeout` / `cancelled`（走 `ToolError::session_stop`，
保留 `legacy_parsed` 的 stdout/stderr）；`pytest_failed` / `python_snippet_failed`
（`ToolError::subprocess_failed`，由 `cargo_subcommand_failed` 重命名而来——现在三族共用，
退出码仍写进 `legacy_parsed.exit_code`）。

**未抽取**：`python_tools::run_and_format_try` 与 `cargo_subcommand::run_and_format_try` 仍有
约 60 行重复（后者无 `allowed_exit`）。注释里已写明「第三次出现即抽公共模块」——现在是第二次，
抽出去要动已迁移的 cargo 路径，留给后续 slice。

**测试**：`python_tools::tests` 从 3 条扩到 8 条（`python_snippet_nonzero_exit_reports_failed`、
`python_snippet_missing_code_is_invalid_args`、`pytest_without_project_marker_is_success`、
`pytest_rejects_shellish_keyword`），超时两条改为断言 `e.code == "timeout"` 且
`e.legacy_parsed.stdout` 仍含部分输出。审计表删掉 `pytest_run` / `python_snippet_run` 两行。

### 4.14 `frontend_audit_ci.inc.rs`（2）

| 工具 | 形态 | 目标实现 | 备注 |
|---|---|---|---|
| `workflow_execute` | 手写 **stub** | 不执行，返回提示串 | 由 runtime 在 `run_agent_turn` 拦截，**迁移意义存疑**：同步面本就没有执行体，更接近 `AsyncOnly` |

### 4.15 `docs_health.inc.rs`（1）— 形态 `cwd_maxlen` — **已迁 2026-10-28**

> 旧标题写作 `format_lint.inc.rs / docs_health.inc.rs（各 1）`，但 `format_lint.inc.rs` 这个 spec 文件已不存在，按实际清单修正。

| 工具 | 目标实现 | 备注 |
|---|---|---|
| `docs_health_sweep` | `docs_health_sweep::docs_health_sweep_try` | 聚合器（预览 + 拼写 + 链接）；三种「失败」的拆法见 §4.19 |

### 4.16 只读聚合三兄弟 — **已迁 2026-10-28**

| 工具 | 形态 | 目标实现 | 失败码 |
|---|---|---|---|
| `diagnostic_summary` | 手写（多参数转发 `&[]`） | `diagnostics::diagnostic_summary_try` | `invalid_args` |
| `repo_overview_sweep` | 孪生宏 `define_runners_cwd_maxlen_try!` | `repo_overview::repo_overview_sweep_try` | `invalid_args` |
| `crate_contract_map` | 手写 | `contract_map::crate_contract_map_try` | `invalid_args` |

**为什么这一批要「先钉测试、再改类型」**：三个工具各自唯一的真失败点是 `parse_args_json`，其余全是**有意容忍**
（审计表原按 `Legal` 钉住）。容忍清单：

- `diagnostic_summary`：`sections` / `extra_env_vars` 类型不对 ⇒ 忽略（`sections` 退化为「输出全部小节」）；工作区根无法解析 ⇒ 正文写一行 `无法解析工作区根: …`，工具链 / env 小节照常输出。
- `repo_overview_sweep`：`doc_paths` / `source_roots` 等字段类型不对 ⇒ 退回默认值；文档 / 源码根不存在 ⇒ 正文标「不存在」。
- `crate_contract_map`：`head_lines_per_file` / `keyword_hits_per_file` 越界 ⇒ clamp（`5..=120` / `5..=200`）；非法 / 越界 / 不存在的 `extra_paths` ⇒ 跳过。

迁移时若把这些分支「顺手」改成 `Err`，模型会收到一次莫名其妙的失败，而它给的参数其实已被合理降级 ——
这是 §6 第 2 条（`Ok` 正文不得变）的镜像：**「容忍」也是成功语义的一部分**。因此每模块都留一对用例：
`*_tolerates_*`（非法字段 / 越界值 ⇒ `Ok`）与 `*_bad_args_is_invalid_args`（非法 JSON ⇒ `Err`），后者保证不是「全都成功」。

**没有新错误码**：三者都没有 IO / 网络失败分支（缺文件、缺路径一律写进正文），所以只用到既有 `invalid_args`；
不产生 `_failed` 码，也就不需要往 `failure_category_for_error_code` 里加分支。

**棘轮与审计表**：`LEGACY_BASELINE` 11 → 8；审计表三条换成一段注释；`Expect::Legal` 失去最后一个 Legacy 用例，
变体保留并加 `#[allow(dead_code)]` + 理由，供下一个「容忍非法入参」的 Legacy 工具使用。

### 4.17 外部 API 族（`get_weather` / `web_search`）— **已迁 2026-10-28**

| 工具 | 形态 | 目标实现 | 迁移后可出现的码 |
|---|---|---|---|
| `get_weather` | 手写（转发 `ctx.weather_timeout_secs`） | `weather::run_try` | `invalid_args` / `weather_client_failed` / `timeout` / `weather_api_error` / `weather_city_not_found` / `weather_no_current_data` |
| `web_search` | 手写（要整个 `ctx`：provider / key / proxy / 截断长度） | `web_search::run_try` | `invalid_args` / `web_search_not_configured` / `web_search_browser_unavailable` / `web_search_client_failed` / `timeout` / `web_search_api_error` |

**这两个工具本来就有错误码**，只是码藏在正文首行：`错误[weather_api_error]：…` 由
`extract_error_code_marker`（`src/cm_tools/tool_result/mod.rs`）反解。所以本批的收益不是「第一次有码」，而是：

1. 不再依赖**首行格式**——任何前置行（警告、`提示：`、多段拼装）都会让标记失效；
2. `retryable` / `failure_category` 由 `ToolError` 决定，而不是「正文里出现 timeout 字样」；
3. 正文文案**逐字保留**（含 `错误[码]：` 标记），`message` 侧零变化，前端与金样不受影响。

**顺带修掉 `get_weather` 的两个静默成功**：

| 旧分支 | 旧正文 | 旧判定 | 现在 |
|---|---|---|---|
| 地理编码无匹配 | `未找到与「X」匹配的地点，请换一个城市或地区名重试。` | 不以「错误」开头，也不含「失败 / 超时」⇒ **`ok: true`** | `weather_city_not_found` |
| 上游无 `current` | `未获取到当前天气数据` | 同上 ⇒ **`ok: true`** | `weather_no_current_data` |

即：模型查一个不存在的城市名，**旧实现会收到「成功」**。审计表当时没盖住这两条，是因为
`{"city":""}` 命中的是更早的参数校验分支（那条有 `错误：` 前缀）。

**新增码的归类**：`web_search_not_configured`（brave/tavily 缺 Key）归 `Workspace`——换配置可修、
改参数没用，与 `codebase_semantic_unconfigured` 同类；两个 `*_client_failed`、`*_browser_unavailable`、
`weather_no_current_data` 归 `External`；`weather_city_not_found` 归 `InvalidInput`。全部在
`failure_category_for_error_code` 显式登记（不以 `_failed` 结尾的码不登记会落到 `Unknown`）。

**测试边界**：`weather::tests` 用 `city_not_found_error` / `no_current_weather_error` 两个纯构造函数钉住
那两条静默成功；`web_search::tests` 用 `test_ctx`（provider=Brave、key 为空）钉住
`web_search_not_configured` 与 `invalid_args` 的区分。真实 HTTP 分支（超时 / 非 2xx / 解析失败）仍无单测——
需桩服务或真实网络，属已知测试缺口。

**棘轮**：8 → 6；审计表两条（`get_weather` / `web_search`）换成一段注释。

### 4.18 受控 HTTP 族（`http_fetch` / `http_request`）— **已迁 2026-10-28**

| 工具 | 形态 | 目标实现 |
|---|---|---|
| `http_fetch` | 手写（`run_direct`） | `sync_fetch::run_direct` → `runner_http_fetch_try` |
| `http_request` | 手写（`run_request_direct`） | `sync_fetch::run_request_direct` → `runner_http_request_try` |

**共用内核**：两者都落到 `sync_fetch::fetch_with_method` / `request_with_json_body`，所以核心函数
一次换类型，四个调用面（runner、并行只读批、审批后的异步路径、Docker 沙盒 runner）统一用
`unwrap_or_else(|e| e.message)` 展平 —— 正文仍是那段 `错误[码]：…`，逐字不变。

**「未匹配白名单」不是执行失败，而是策略拒绝**：同步路径（`run_tool`、
`workflow_execute` 节点、并行只读批）**没有审批通道**，URL 不匹配白名单时旧实现返回一句普通错误文本，
被启发式判成失败并塞进 `tool_failed`。现在给 `http_fetch_url_not_allowed` /
`http_request_url_not_allowed`，分类 `PolicyDenied`、`retryable=false`：重试同一 URL 不会变成允许，
必须换 URL 或改配置。两个工具各用各的码（审批键 GET/HEAD 与 `<METHOD>:` 本就分开）。
真正要审批的路径（Web SSE + `approval_session_id`、CLI 异步）**不经过**这两个函数，行为不变。

**新增码**：`http_client_failed`（`External`，客户端构建失败）、上面两个 `*_url_not_allowed`
（`PolicyDenied`）。`http_timeout` / `http_network_error` / `http_body_read_error` 复用原码，
正文首行 `错误[码]：` 标记保留。

**测试**：`http_fetch/tests.rs` 增三条 —— 非法 scheme ⇒ `invalid_args`、白名单外 ⇒
`http_fetch_url_not_allowed`（且 `!retryable`）、`http_request` 用自己的码。真实请求（超时 /
非 2xx / 解码）仍无单测，依赖既有实网与手工验证。

**棘轮**：6 → 4；审计表两条变注释，本族清零。剩余 Legacy 为
`present_clarification_questionnaire` / `docs_health_sweep` / `code_nav` / `codebase_semantic_search`。

### 4.19 B 档：`docs_health_sweep`（聚合器）— **已迁 2026-10-28**

| 工具 | 形态 | 目标实现 |
|---|---|---|
| `docs_health_sweep` | `f(args, cwd, max_len)` | `docs_health_sweep::docs_health_sweep_try` |

**核心问题：它有三种「失败」，旧实现把它们混成一个 `String`。** 正文含「失败」二字
（`整体: 存在失败项（见上）`）就会被 `parse_legacy_output` 判成工具故障，于是
「扫查跑完、发现三处拼写错误」被报成**工具失败**——调用方据此去重试一件已经成功的事。

迁移把它拆成三路：

| 情形 | 旧 | 新 |
|---|---|---|
| 入参坏（JSON 坏、`md_roots` 类型不对） | 靠 `参数解析错误:` 前缀 | `invalid_args` |
| 子阶段**执行故障**（`markdown_check_links_try` 返回 `Err`：工作区解析 / 扫描 IO） | 取 `e.message` 拼进正文，`markdown_links_failed` 判 `结论: 发现` 不成立 ⇒ **记 passed**，故障被吞 | 上抛为 `Err`（码原样透传，如 `markdown_check_links_workspace_failed`） |
| 子阶段**检查出问题**（拼写、错词、坏链接） | 正文写 `failed` + 整体 `存在失败项` ⇒ 判**失败** | 正文逐字不变，整体 `Ok` |

**新增码**：`docs_health_serialize_failed`（`Internal`）—— 阶段参数二次序列化，只可能
在 `serde_json` 不变量破坏时触发（入参已成功反序列化过一次）。

**`md_roots: ["../outside"]` 不返回 `Err`**：越界根被记成一条**本地问题**，即「发现」。
写测试时踩过一次（以为它会失败），所以 `markdown_phase_error_fails_the_sweep` 改用
「工作区根不存在」触发真故障，并在测试注释里写明这一点。

**测试**（4 条）：`sweep_readme_only_markdown`（干净通过）、`content_findings_stay_ok`
（坏链接 ⇒ `Ok` 且含 `markdown_check_links: failed` + `存在失败项`）、
`markdown_phase_error_fails_the_sweep`（缺工作区 ⇒ `markdown_check_links_workspace_failed`）、
`bad_args_is_invalid_args`。

**棘轮**：4 → 3；`define_runners_cwd_maxlen!` 失去最后一个实例，连带删除
（`unused_macros` 会报）。

### 4.20 控制面：`present_clarification_questionnaire` — **已迁 2026-10-28**

失败时旧实现返回 `退出码：1\n{e}\n`，只有 `parse_legacy_output` 的 exit-code 分支认得；
`{}`（必填字段全缺）全靠这层兜底。现返回 `Err(invalid_args)`，所有校验分支（问卷 id 非法、
`intro` 空白 / 超长、题目数不在 1～12、题目 `id` 非法或重复、`label` / `hint` 空白或超长、
`kind` 非法）同一码。

**成功正文逐字不变，首行尤其重要**：`cm_internal::clarification_sse_bridge` 用
`tool_output.trim_start().starts_with("退出码：0")` 判断「这是登记成功的问卷」，改动首行会让
Web SSE 控制面事件静默失效。由 `present_runner_keeps_exit_zero_header_and_reports_invalid_args`
钉住（成功首行 + `invalid_args` 的两种触发）。

**runner 形态**：args-only 的 `define_runners_args_only!` 随之改名为
`define_runners_args_only_try!`；`try_macro_typecheck` 挂上该实现，使每种宏形态都有真实
`_try` 实现参与类型检查。

**棘轮**：3 → 2。剩余 Legacy 仅 `code_nav`（B 档，action 选择器，需在分发层收敛未知 action、
并把四个子实现一并换类型）与 `codebase_semantic_search`（D 档，受 host 注入 + feature 限制）。

---

## 5. 建议切片顺序

按「实现层改动量 ÷ 风险」排，前两档可直接动手：

| 档 | 族 | 理由 |
|---|---|---|
| **A（低风险，建议先做）** | `metrics` 3 个、`self_config_info`、`long_term_*` 4 个、`skill_manage`、**只读聚合三兄弟**（`diagnostic_summary` / `repo_overview_sweep` / `crate_contract_map`，2026-10-28 已迁，见 §4.16）（**全部已迁，A 档已清空**） | 纯读或纯本地；错误分支已存在，只需从「返回串」改成 `Err`；无外部进程、无后台装配 |
| **B（中风险）** | ~~`archive` 3 个~~（**已迁 2026-10-28**）、~~`reminder` / `event`~~（**已迁 2026-10-28**）、~~`docs_health` 1 个聚合器~~（**已迁 2026-10-28**，见 §4.19）、`code_nav` 1 个 action 选择器 | action 选择器需在分发层做「未知 action」的错误收敛；聚合器须保留自己的失败判定（`结论: 发现` 表示「发现了链接问题」，不是工具失败） |
| **C（高风险，须先设计）** | ~~`gh_*` 5 个~~（**已迁 2026-10-28**）、~~`git`~~（**已迁 2026-10-28**）、~~`background_job*`~~（**已迁 2026-10-28**）、~~`http_fetch` / `http_request`~~（**已迁 2026-10-28**，见 §4.18）、~~`pytest_run` / `python_snippet_run`~~（**已迁 2026-10-28**）、~~`web_search` / `get_weather`~~（**已迁 2026-10-28**，见 §4.17） | 失败语义混在命令/HTTP 输出里；`http_*` 还有「未审批」这一中间态；`pytest_run` 与后台任务装配耦合 |
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
| 2026-10-28 | **B 档：`reminder` / `event` 迁为 Typed**（Legacy 25 → **23**，基线守卫同步）。两族共用 `schedule.rs` 的 `ensure_data_dir` / `read_json` / `write_json`，五个 helper 与九个入口一起换成 `Result<_, ToolError>`；runner 侧只是把两行从 `define_runners_cwd!` 挪到已有的 `define_runners_cwd_try!`，**无需新宏**。**迁移动机是四个静默成功分支**：dispatch 层合并时新增的「`add` 缺 `title`」「缺 `id`」两类必填校验，以及 `complete` / `update` / `delete` 的「`未找到提醒/日程：id=…`」，正文都不含「错误 / 失败 / 超时」，被 `parse_legacy_output` 判成 `ok: true` —— 模型传错参数或传错 id 却拿到「成功」。现分别为 `invalid_args` 与 `reminder_not_found` / `event_not_found`。新增错误码：`reminder_not_found` / `event_not_found`（`InvalidInput`，理由是 id 只可能来自调用方，模型可先 `list` 自查，与 `archive_not_found` 的「文件系统状态」口径刻意不同）、`schedule_io_failed` / `schedule_parse_failed`（`External`）、`schedule_serialize_failed`（`Internal`，走既有 `ends_with("_serialize_failed")` 分支）。`ToolError` 顺带加了通用构造器 `with_code`，`workspace` / `external_code` 改为委托它（三份相同实现收敛为一份）。**刻意保留**：`list` 在入参 JSON 坏掉时仍退化为「无过滤条件」（宽容语义不变），成功正文逐字未变，`builtin_write_effect_tools` 无需改动（两者九合一时已登记）。测试：既有 8 个流程/必填用例改为取 `Ok` / `e.code`，新增 `unknown_id_reports_not_found_instead_of_success` / `empty_list_is_success_not_failure` / `io_failure_reports_schedule_io_failed`（后者用「`.crabmate` 建成普通文件」触发，避免 root 下 chmod 造只读目录的 flaky）。 |
| 2026-10-28 | **C 档：`git` 迁为 Typed**（Legacy 14 → **13**，基线守卫同步）。10 个 action 全部换成 `Result<String, ToolError>`，`helpers::run_and_format` / `run_diff_mode` / `parse_args` / `extract_safe_path` / `require_safe_path` 一起换类型并加 `#[allow(clippy::result_large_err)]`；runner 侧把 `define_git_runners!` 换成孪生 `define_git_runners_try!`（旧的删掉，`unused_macros` 会报），并在 `try_macro_typecheck` 挂上 `git::dispatch` smoke。**关键复用**：在 `output_util` 里加 `run_command_output_try(..., failure_code)`，把渲染抽成私有 `render_process_output` 与前一个函数共用，所以 `Ok` 正文逐字不变、只多「非零退出 → `Err`」一层判定。新增错误码 **`git_not_a_repo`**（`Workspace`，显式登记：不以 `_failed` 结尾）与 `git_{action}_failed`（`External`，由 `external_code_owned` 拼接）。**没有豁免名单**：只读 git 子命令不存在「非零退出但语义正常」，代价是空仓库的 `log` / `show`（128）现在算失败——写入文档与测试（`log_in_empty_repository_is_a_failure` 与配对的 `log_after_first_commit_is_success`）。`ensure_git_repo` 保留 `String` 薄壳供 `github_cli::pr_body` 用，避免把 `gh_git_failed` 的码混进来。审计表删掉 `git` 条目。 |
| 2026-10-28 | **C 档：`pytest_run` / `python_snippet_run` 迁为 Typed**（Legacy 13 → **11**，基线守卫同步）。`pytest_run_try` / `python_snippet_run_try` 取代旧 `String` 入口，runner 改名为 `runner_pytest_run_try` / `runner_python_snippet_run_try`。**两个「不是失败」的分支先拆出来**：①`build_pytest_command` 现在返回 `PytestCommand::{Run, Skipped}`——工作区没有 Python 工程标记时的「跳过」以前以 `Err(String)` 返回但被启发式判成功（审计表 `Legal`），若塞进 `Err` 就成了迁移引入的假回归；后台装配 `pytest_run_background_argv` 仍把它当错误（没有命令可跑）。②pytest 的 **5（no tests collected）** 算成功：`run_and_format_try` 因此带 `allowed_exit`（`PYTEST_OK_EXITS = &[0, 5]`），而 **1（断言失败）不在其中**，与 `cargo_test` 的 `cargo_test_failed` 同口径——这点与 `gh pr checks` 的 1/8 不同：那里是 CI 状态，这里是真的测试失败。**错误码**：`invalid_args`（缺/空 `code`、`code` 超 256 KiB、`use_uv` 缺 `pyproject.toml`、`test_path`/`keyword`/`markers` 非法）、`workspace_resolve_failed`、`python_snippet_temp_file_failed`、**`python_snippet_temp_path_outside_workspace`**（`PolicyDenied`，显式登记）、`command_not_found` / `spawn_failed`、`timeout` / `cancelled`（`ToolError::session_stop`，保留 `legacy_parsed` 的 stdout/stderr，超时测试因此仍能断言部分输出）、`pytest_failed` / `python_snippet_failed`。顺带把 `ToolError::cargo_subcommand_failed` 重命名为 **`subprocess_failed`**（cargo / pytest / python_snippet 三族共用，退出码仍写进 `legacy_parsed.exit_code`）。**未抽取**：`python_tools::run_and_format_try` 与 `cargo_subcommand::run_and_format_try` 仍重复约 60 行，注释写明「第三次出现即抽公共模块」（现在只是第二次，抽出去要动已迁移的 cargo 路径）。测试 3 → 8 条；审计表删掉这两行。 |
| 2026-10-28 | **A 档收尾：只读聚合三兄弟迁为 Typed**（`diagnostic_summary` / `repo_overview_sweep` / `crate_contract_map`，Legacy 11 → **8**）。三者唯一的真失败点都是 `parse_args_json`，其余全是有意容忍（审计表原按 `Legal` 钉住）：`sections` / `extra_env_vars` 类型不对只忽略、工作区根不可解析只写一行、字段类型不对退回默认值、越界阈值 clamp、非法 `extra_paths` 跳过。因此**先在三处 `tests` 里把「容忍 ⇒ `Ok`」写成用例，再改返回类型**（每处配一条 `*_bad_args_is_invalid_args`，防「全都成功」）—— 这是 §6 第 2 条的镜像：`Ok` 正文不能变，**「容忍」本身也是成功语义**。错误码只用既有 `invalid_args`，无新增分类分支。runner 侧两个手写（多参数转发）+ 一个并入 `define_runners_cwd_maxlen_try!`；审计表三条变注释，`Expect::Legal` 失去最后一个 Legacy 用例，保留变体并加 `#[allow(dead_code)]` + 理由。顺带修正 §2.1 两处过期行（`basic_network` 实际全是 Legacy、`format_lint.inc.rs` 文件已不存在）。 |
| 2026-10-28 | **C 档：外部 API 族迁 Typed**（`get_weather` / `web_search`，Legacy 8 → **6**，见 §4.17）。这两个工具的码原本藏在正文首行 `错误[码]：` 里由 `extract_error_code_marker` 反解，迁移后不再依赖首行格式、`retryable` 由 `ToolError` 决定；正文逐字保留。**顺带修掉 `get_weather` 两个静默成功**：查不到地名（`未找到与「X」匹配的地点…`）与上游无 `current`（`未获取到当前天气数据`）旧实现都被启发式判成 `ok: true`。新增 6 个码并登记分类（`weather_city_not_found` → `InvalidInput`，`web_search_not_configured` → `Workspace`，其余 4 个 → `External`）。测试用两个纯构造函数 + `test_ctx` 钉住码的分工；真实 HTTP 分支仍无单测（已知缺口）。 |
| 2026-10-28 | **C 档：受控 HTTP 族迁 Typed**（`http_fetch` / `http_request`，Legacy 6 → **4**，见 §4.18）。核心是共用内核 `fetch_with_method` / `request_with_json_body` 一次换类型，四个调用面（runner、并行只读批、审批后异步路径、Docker 沙盒 runner）统一展平 `Err`，正文逐字不变。**关键判断**：同步路径没有审批通道，「URL 未匹配白名单」是**策略拒绝**而非执行失败 → 新码 `http_fetch_url_not_allowed` / `http_request_url_not_allowed`（`PolicyDenied`、`retryable=false`），两个工具各用各的码；另有 `http_client_failed`（`External`）。Web SSE 审批与 CLI 异步路径不经过这两个函数，行为不变。审计表两条变注释，本族清零；剩余 4 个 Legacy 见 §4.18 末。 |
| 2026-10-28 | **B 档：`docs_health_sweep` 迁 Typed**（Legacy 4 → **3**，见 §4.19）。核心是把它混在 `String` 里的**三种「失败」拆开**：入参坏 → `invalid_args`；子阶段执行故障（`markdown_check_links_try` 的 `Err`，工作区解析 / 扫描 IO）从「拼进正文并记 passed」改为上抛；子阶段**检查出问题**（拼写 / 坏链接）保持正文 `### 步骤汇总` + `整体: 存在失败项` 不变，但整体 `Ok`——此前正文里的「失败」二字会让 `parse_legacy_output` 把一次跑完的扫查判成工具故障。新增码 `docs_health_serialize_failed`（`Internal`，阶段参数二次序列化）。测试 1 → 4：`content_findings_stay_ok`（坏链接 ⇒ `Ok` 且含 `存在失败项`）与 `markdown_phase_error_fails_the_sweep`（缺工作区 ⇒ `markdown_check_links_workspace_failed`）成对。**踩坑记录**：`md_roots: ["../outside"]` 是「发现一条本地问题」，不返回 `Err`，故障注入必须换「工作区根不存在」。`define_runners_cwd_maxlen!` 失去最后实例，连带删除。 |
| 2026-10-28 | **控制面：`present_clarification_questionnaire` 迁 Typed**（Legacy 3 → **2**，见 §4.20）。失败从 `退出码：1\n{e}\n`（只有 `parse_legacy_output` 的 exit-code 分支认得）改为显式 `invalid_args`；成功正文逐字不变，首行 `退出码：0` 是 `clarification_sse_bridge` 判定「登记成功」的依据，由 `present_runner_keeps_exit_zero_header_and_reports_invalid_args` 钉住。args-only 形态的宏随之改名为 `define_runners_args_only_try!` 并挂进 `try_macro_typecheck`。剩余 Legacy **2**：`code_nav`（B 档，下一批）与 `codebase_semantic_search`（D 档）。 |
