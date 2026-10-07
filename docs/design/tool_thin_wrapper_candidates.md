# 工具薄 CLI 封装：剩余家族候选盘点与判定

**状态**：分析备忘（候选盘点，**未**承诺顺序与时间表）。**受众**：维护 `src/cm_tools/`、`tool_specs_registry` 与 `tool_dispatch` 的开发者。  
**语言**：中文。  
**关联**：

- 方案与约束（A/B/C 路线、只读语义、已落地清单）：**`docs/design/tool_calling_evolution.md`** §8.5
- 内置工具契约与信封：**`docs/工具说明.md`**
- 未完成待办：**`docs/待办清单.md`** → **`tools/` 与 `tool_registry.rs`** 章
- 安全面：**`docs/配置说明.md`**（`allowed_commands`、审批、沙盒）

---

## 1. 目的与判据

本文件是 **§8.5 的落地配套**：§8.5 已给出收敛的路线、约束与已落地清单（A 档 14 个 JVM/容器/前端、B 档 17 个写侧 Git 工具 + `git_diff` 三合一），并明确「其余家族（`gh_*` / `cargo_*` 等）待后续切片」。本文对**剩余内置工具**逐家族做一次「是否适合改为 `run_command` 薄封装」的盘点，供后续切片直接取用。

**判据**（三条须同时成立才「适合」）：

1. 实现 = **单一 CLI** + 固定/简单 `argv` 拼装 + 直接返回输出；
2. **无独有增值逻辑**——不含项目标记跳过、输出后处理 / JSON 结构化解析、多步聚合、写盘 `confirm` 门、进程内计算、结果缓存；
3. CLI 已在 **`config/tools.toml`** 的 `allowed_commands` 白名单内（否则须先评估放行）。

---

## 2. 收敛代价（先于判定必读）

`run_command` 与专用工具在**策略语义**上并不等价，收敛前须逐项权衡（细则见 §8.5.2）：

### 2.1 只读语义损失

`is_readonly_tool`（`src/cm_tools/registry_policy.rs`）以 `builtin_write_effect_tools()` 的**写副作用名单**判定；`run_command` **在该名单内**，即**非只读**。因此任何**只读**专用工具改写为 `run_command`，会连带失去：

- 并行只读批（`parallel_readonly_batch`）；
- 只读失败重试（`tool_retry_policy`）；
- `SessionMode::Ask` / `Plan` 的 `requires_readonly_tools()` 门控；
- 只读结果 TTL 缓存 / 同轮去重（`readonly_tool_ttl_cache_secs`）。

> **缓解事实**：多数语言栈前缀（`cargo_` / `gh_` / `go_` / `ruff_` / `pytest` / `mypy_` / `uv_` / `pre_commit` / `python_` / `typos_` / `codespell_` / `npm_` / `frontend_` / `maven_` / `gradle_` / `docker_` / `podman_`）已被 `builtin_parallel_sync_prefix_hit` **排除出并行只读批**，故其中「并行只读批」一项**本就不适用**；但**只读重试 / Plan·Ask 门控 / TTL 缓存**三项仍会丢失。

### 2.2 白名单与后台任务

- **白名单外 CLI**：`typos` / `codespell` / `ast-grep`（`sg`）/ `rust-analyzer` 等**不在** `allowed_commands`；删掉专用工具等于功能净丢失，除非先扩白名单（§8.5.3 边界，风险最高）。
- **后台任务装配**：`background_job_async_tools` 默认仅 `["run_command"]`；`cargo_test` / `pytest_run` 须**显式加入**才走后台，收敛前须确认不破坏该链路。

### 2.3 运行时语义差异

- **限流**：`run_command` 有 `MAX_COMMANDS_PER_SEC = 5` 频率限制；
- **墙钟**：按 `command_timeout_secs`（及按工具名覆盖）计；
- **截断**：`MAX_OUTPUT_LINES = 500`（专用工具常放宽到 800 / 512KB）；
- **审批**：工作区外绝对路径 / `..` 走 `tool_approval` 人工审批，比专用工具的「硬拒绝」更宽松；
- **变更集**：`run_command` 无法解析写路径，可能触发语义索引整表失效（见 `config/tools.toml` 注释）。

---

## 3. 判定矩阵（剩余家族）

> 图例：**§3.1 写侧薄封装** = 可收敛（不损失只读语义，但可能有写侧安全损失）；**§3.2 视情况** = 可收敛但有 §2 代价，须按频率 / 价值取舍；**§3.3 保留** = 不适合收敛。

### 3.1 写侧薄封装（收敛**不损失只读语义**）

这些工具**本就在 `builtin_write_effect_tools()` 名单内**，收敛**不额外损失 §2.1 的只读能力**。但「无只读损失」≠「零损失」：其中多数带 `confirm` 强制门或校验 / 路由逻辑，降级等于把这些**写侧安全能力**交还给 `run_command` 白名单与 `tool_approval`，须逐项确认可接受——风险画像与 §8.5 B 档（17 个写 Git 工具、**能力零损失**）不同。

**3.1.1 首推（试点，接近零损失）**

| 工具 | 底层 CLI | 收敛后失去 |
|------|----------|-----------|
| ~~`cargo_clean`~~（**已收敛 2026-10-07**） | `cargo clean` | 已删 ToolSpec + runner（`cargo_clean_try` / `CargoCleanArgs`），改用 `run_command`。失去 `dry_run` **默认 true** 的安全默认 + `package` / `release` / `doc` 参数 schema（降级后须显式自带 `--dry-run`）；**无只读语义损失**（二者均为写副作用工具）。 |

**3.1.2 含安全门 / 增值逻辑（须先定替代机制再收敛）**

| 工具 | 底层 CLI | 收敛后失去 |
|------|----------|-----------|
| `cargo_fix` | `cargo fix` | `confirm=true` **强制门**（`src/cm_tools/tools/cargo_tools.rs:597-602`）+ `Cargo.toml` 存在性检查 + 8 个布尔开关 schema |
| `go_mod_tidy` | `go mod tidy` | `confirm=true` **强制门** + `go.mod` 存在性跳过（`src/cm_tools/tools/go_tools.rs:167-172`） |
| `python_install_editable` | `uv pip install -e .` / `python3 -m pip install -e .` | `backend`（uv/pip）路由 + `pyproject.toml` / `setup.py` 存在性检查（`src/cm_tools/tools/python_tools.rs:309-342`） |

> 3.1.2 三项收敛前须先把 `confirm` 门映射到 **`tool_approval::SensitiveCapability`**（或等价审批），否则属**安全回归**。

### 3.2 视情况（只读薄封装，须权衡 §2.1 代价）

**均已注册为只读工具**，`run_command` 化会失去只读重试 / Plan·Ask 门控 / TTL 缓存：

| 家族 | 工具 | 说明 |
|------|------|------|
| cargo 元数据 / 旁路 | `cargo_metadata` / `cargo_tree` / `cargo_doc` / `cargo_nextest` / `cargo_outdated` / `cargo_machete` / `cargo_udeps` / `cargo_publish_dry_run` / `cargo_fmt_check` | 单条 `cargo <sub>`；低频者价值低，可整族收敛 |
| cargo 审计 | `cargo_audit` / `cargo_deny` | `cargo audit` / `cargo deny check` |
| cargo 主链路 | `cargo_check` / `cargo_clippy` / `cargo_test` / `cargo_run` / `rust_test_one` | 经**共享子进程会话** + **测试结果缓存**；`cargo_test` / `pytest_run` 关联后台任务，收敛须保留缓存与装配 |
| Rust 编译 | `rust_rustc` | `rustc` 已在白名单；含参数安全上限（64 个 / 8192 字节） |
| Go | `go_build` / `go_test` / `go_vet` / `go_fmt_check` / `golangci_lint` | 单条 `go` / `gofmt` / `golangci-lint` |
| Python | `ruff_check` / `mypy_check` / `pytest_run` / `uv_sync` / `uv_run` | 单条 `ruff` / `mypy` / `python3 -m pytest` / `uv`；`pytest_run` 关联后台任务 |
| Git 只读 | `git_status` / `git_diff` / `git_log` / `git_show` / `git_blame` / `git_file_history` / `git_branch_list` / `git_remote_status` / `git_remote_list` / `git_clean_check` | **§8.5 决策：保留**（审查工作流高频，只读语义价值高）；此处仅登记为「视情况」 |
| 质量 | `pre_commit_run` | `pre-commit run` |
| 格式（只读） | `format_check_file` | 扩展名 → 格式化器**路由**属增值；`format_file` 为写侧但同理 |
| 系统查询 | `package_query` / `port_check` / `process_list` | 只读，但含抽象 / 输出后处理，收敛会丢结构化输出 |
| GitHub CLI | `gh_pr_list` / `gh_pr_view` / `gh_pr_checks` / `gh_pr_diff` / `gh_issue_list` / `gh_issue_view` / `gh_run_list` / `gh_run_view` / `gh_run_failure_summary` / `gh_release_list` / `gh_release_view` / `gh_search`（读类）；`gh_pr_create` / `gh_pr_merge` / `gh_pr_review` / `gh_pr_comment` / `gh_pr_edit` / `gh_issue_create` / `gh_run_rerun` / `gh_release_create`（写类） | **注**：`github_cli/common.rs` 的 `run_gh_vec` **本身已构造 `{command:"gh",args:[…]}` 调 `command::run`**——读类已是 `run_command` 之上的一层；真正增值为参数校验、`--json` 结构化与 body 走临时文件 |

**白名单外 CLI（须先扩白名单）**：

| 工具 | 底层 CLI | 说明 |
|------|----------|------|
| `typos_check` / `codespell_check` | `typos` / `codespell` | 不在白名单；含路径安全与存在性校验 |
| `ast_grep_run` | `ast-grep` / `sg` | 不在白名单；`ast_grep_rewrite` 另含写盘 `confirm` 门 |

### 3.3 保留（无 CLI 对应 / 结构化 / 编排 / 状态 / 网络 / 高增值解析）

- **文件与结构化**：全部 `file_core` / `file_extra`（`create_file` / `modify_file` / `copy_file` / `move_file` / `delete_files` / `delete_dir` / `append_file` / `create_dir` / `search_replace` / `chmod_file` / `symlink_info` / `read_file` / `read_dir` / `glob_files` / `list_tree` / `file_exists` / `read_binary_meta` / `hash_file` / `extract_in_file` / `search_in_files` / `codebase_semantic_search`）、`structured_*`、`markdown_check_links`。
- **代码导航 / 度量**：`find_symbol` / `find_references` / `rust_file_outline` / `call_graph_sketch`、`code_stats` / `dependency_graph` / `coverage_report`。
- **归档**：`archive_pack` / `archive_unpack` / `archive_list`（用 Rust crate，非 CLI）。
- **状态 / 记忆 / 配置**：`schedule_*`（reminders / events）、`long_term_*`、`self_config_info`、`skill_manage`。
- **诊断 / 文档渲染**：`diagnostic_summary` / `error_output_playbook` / `present_clarification_questionnaire` / `changelog_draft` / `license_notice` / `repo_overview_sweep` / `crate_contract_map` / `summarize_experience` / `gh_pr_body_draft`。
- **Rust 专属**：`rust_compiler_json`（编译 JSON 解析）、`rust_backtrace_analyze`、`rust_analyzer_*`（LSP stdio，非白名单 CLI，不可替代）。
- **纯计算 / 文本**：`get_current_time` / `calc` / `convert_units` / `text_transform` / `json_format` / `regex_test` / `env_var_check` / `todo_scan` / `text_diff` / `table_text`。
- **网络 / 执行 / 会话**：`get_weather` / `web_search` / `http_fetch` / `http_request` / `terminal_session` / `background_job_*` / `workflow_execute` / `run_command`。
- **聚合编排**：`ci_pipeline_local` / `release_ready_check` / `quality_workspace` / `run_lints` / `docs_health_sweep` / `playbook_run_commands`（后者是 `error_output_playbook` 启发式的**执行半体**，含多命令编排与启发式归类，非单条 CLI 薄封装）。
- **校验密集 / 安全面敏感**：`gh_api`——含 `validate_api_path`（禁 `/` 开头、禁 `..`、字符白名单）+ 方法白名单（GET/HEAD/POST/PATCH/PUT/DELETE）+ body 须合法 JSON + **body 经 stdin 传入**；降级 `run_command` 无 stdin 通道、无 path 限制，属**扩大安全面**，不收敛（`src/cm_tools/tools/github_cli/api.rs:101-157`）。

---

## 4. 建议切片顺序

1. ~~**§3.1.1 试点**：`cargo_clean`~~（**已落地 2026-10-07**：删 ToolSpec + runner + `cargo_clean_try` + `CargoCleanArgs`，并同步 tests / `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`；`fmt` / `clippy -D warnings` / `cargo test` 全绿）。
2. **§3.1.2 写侧安全门族**（`cargo_fix` / `go_mod_tidy` / `python_install_editable`）：**须先落 `confirm` → `tool_approval` 替代**，否则不动。
3. **§3.2 cargo 元数据 / 审计族**（`cargo_metadata` / `tree` / `doc` / `nextest` / `outdated` / `machete` / `udeps` / `publish_dry_run` / `fmt_check` / `audit` / `deny`）：低频只读，接受 §2.1 代价。
4. **§3.2 Go / Python 族**：按频率评估；`cargo_test` / `pytest_run` 收敛前须核对后台任务装配。
5. **`gh_*` 家族**：读类已轻量，优先级低于 1–4；写类因类型化参数价值高，暂缓。
6. **白名单外 CLI（`typos` / `codespell` / `ast-grep`）**：仅在决定扩白名单后再评估（风险最高，暂不推进）。

保留项（§3.3，含 `gh_api` / `playbook_run_commands`）不动；Git 只读（§3.2）维持 §8.5 的**保留**决策。

---

## 5. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-10-07 | 初稿：对 §8.5 之后的**剩余内置工具**逐家族盘点，给出「适合 / 视情况 / 保留」判定矩阵、收敛代价（只读语义 / 白名单 / 后台任务 / 运行时差异）与建议切片顺序。 |
| 2026-10-07 | 修订：§3.1 收窄为「写侧薄封装（无只读损失，但非零损失）」，拆 **3.1.1 首推（`cargo_clean`）** / **3.1.2 含 `confirm` 门或校验（`cargo_fix` / `go_mod_tidy` / `python_install_editable`）**；`gh_api`（校验密集 + stdin body，降级扩大安全面）与 `playbook_run_commands`（启发式执行半体，非薄封装）移入 §3.3 保留；§4 顺序同步调整。 |
| 2026-10-07 | **§3.1.1 `cargo_clean` 试点已落地**：删除 ToolSpec + runner + `cargo_clean_try` + `CargoCleanArgs` + 写副作用 / `dev_tag` / tests 引用，改用 `run_command`；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
