# 工具薄 CLI 封装：剩余家族候选盘点与判定

**状态**：分析备忘（候选盘点，**未**承诺顺序与时间表）。**受众**：维护 `src/cm_tools/`、`tool_specs_registry` 与 `tool_dispatch` 的开发者。  
**语言**：中文。  
**关联**：

- 方案与约束（A/B/C 路线、只读语义、已落地清单）：**`docs/design/tool_calling_evolution.md`** §8.5
- 内置工具契约与信封：**`docs/工具说明.md`**
- 未完成待办：**`docs/待办清单.md`** → **`tools/` 与 `tool_registry.rs`** 章
- 安全面：**`docs/配置说明.md`**（`allowed_commands`、审批、沙盒）
- **另一条正交的收敛方向**（`ToolRunner` Legacy → Typed，与本文件的「收敛为 `run_command`」不是一回事）：**`docs/design/tool_runner_typed_migration.md`**

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

> **缓解事实**：多数语言栈前缀（`cargo_` / `gh_` / `go_` / `ruff_` / `pytest` / `mypy_` / `uv_` / `pre_commit` / `python_` / `npm_` / `frontend_` / `docker_` / `podman_`）已被 `builtin_parallel_sync_prefix_hit` **排除出并行只读批**，故其中「并行只读批」一项**本就不适用**；但**只读重试 / Plan·Ask 门控 / TTL 缓存**三项仍会丢失。（`typos_` / `codespell_` 前缀已于 2026-10-18 随对应工具删除一并清理；`maven_` / `gradle_` 前缀随 `jvm_tools` 整体删除同步清理。）

### 2.2 白名单与后台任务

- **白名单外 CLI**：`rust-analyzer` / `pytest` 等**不在** `allowed_commands`；删掉专用工具等于功能净丢失，除非先扩白名单（§8.5.3 边界，风险最高）。**`typos` / `codespell` / `ast-grep` 已于 2026-10-18 扩入白名单并删除对应薄封装**（`sg` 仍未加入，见 §3.2）。
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

**3.1.2 含安全门 / 增值逻辑（已收敛 2026-10-07）**

| 工具 | 底层 CLI | 收敛后失去 |
|------|----------|-----------|
| ~~`cargo_fix`~~（**已收敛 2026-10-07**） | `cargo fix` | 已删 ToolSpec + runner（`cargo_fix_try` / `CargoFixArgs`）。失去 `confirm=true` **强制门** + `Cargo.toml` 存在性检查 + 8 个布尔开关 schema |
| ~~`go_mod_tidy`~~（**已收敛 2026-10-07**） | `go mod tidy` | 已删 ToolSpec + runner（`go_mod_tidy` / `GoModTidyArgs`）。失去 `confirm=true` **强制门** + `go.mod` 存在性跳过 |
| ~~`python_install_editable`~~（**已收敛 2026-10-07**） | `uv pip install -e .` / `python3 -m pip install -e .` | 已删 ToolSpec + runner（`python_install_editable` / `PythonInstallEditableArgs` + `PythonInstallBackend` + `summary_python_install_editable`）。失去 `backend`（uv/pip）路由 + `pyproject.toml` / `setup.py` 存在性检查 |

> **修订（2026-10-07）**：原计划「3.1.2 三项收敛前须先把 `confirm` 门映射到 **`tool_approval::SensitiveCapability`**，否则属安全回归」经复核后**不再适用**——`cargo` / `go` / `uv` / `python3` **本就在 `allowed_commands` 白名单**，`run_command` 仅在「命令名不在白名单」时触发审批，**无子命令级审批**；即 `confirm` 门在收敛前即可被 `run_command` 直接绕过，**并非强制安全边界**。故本次按「直接收敛、不新增机制」落地：收敛后审批面与既有白名单一致（三工具均为写副作用工具，`Ask`/`Plan` 门控与语义索引失效语义不变）。

### 3.2 视情况（只读薄封装，须权衡 §2.1 代价）

**均已注册为只读工具**，`run_command` 化会失去只读重试 / Plan·Ask 门控 / TTL 缓存：

| 家族 | 工具 | 说明 |
|------|------|------|
| cargo 元数据 / 旁路 | ~~`cargo_metadata`~~ / ~~`cargo_tree`~~ / ~~`cargo_doc`~~ / ~~`cargo_nextest`~~ / ~~`cargo_outdated`~~ / ~~`cargo_machete`~~ / ~~`cargo_udeps`~~ / ~~`cargo_publish_dry_run`~~ / ~~`cargo_fmt_check`~~ | 单条 `cargo <sub>`；低频者价值低，整族已收敛。**首片（Slice A）已收敛 2026-10-07**：`cargo_metadata` / `tree` / `doc` / `outdated` / `machete` / `udeps` / `publish_dry_run` 七个已删。**Slice B 已收敛 2026-10-07**：`cargo_nextest` / `cargo_fmt_check` 已删，并同步改写内置工作流模板 `rust_ci_fmt`（改为 `run_command` + `cargo fmt --check`）与 fixtures / examples。 |
| cargo 审计 | ~~`cargo_audit`~~ / ~~`cargo_deny`~~ | `cargo audit` / `cargo deny check`。**Slice B 已收敛 2026-10-07**：`cargo_audit` / `cargo_deny` 的 ToolSpec 已删。**后续更新**：其复用方 `ci_pipeline_local` / `release_ready_check` 已整族删除，`security_tools` 整个模块随之移除；现直接经 `run_command` 调用。 |
| cargo 主链路 | `cargo_check` / `cargo_clippy` / `cargo_test` / `cargo_run` / `rust_test_one` | 经**共享子进程会话** + **测试结果缓存**；`cargo_test` / `pytest_run` 关联后台任务，收敛须保留缓存与装配。**现状复核（2026-10-27）**：五个工具**均仍在**，未收敛为 `run_command`——按 §1 判据不成立（含结果缓存与后台装配增值逻辑）。但**底层实现早已共用**：`cargo_tools::run_cargo_subcommand_str_try` / `_value_try`，四个 cargo 函数体仅 3–18 行，差异只是 `sub` 字符串（`"check"` / `"clippy"` / `"run"` / `"test"`），`test` 额外包一层 `maybe_cache_cargo_test_try`。**重复因此集中在注册层**：4 个 `ToolSpec` + 4 个 runner + 3 个参数结构体（`CargoCommonCliArgs` 已由 `cargo_check` / `cargo_clippy` 共用；`CargoTestArgs` / `CargoRunArgs` 仍各自重复 `release` / `package` / `bin` / `features` 四字段，共 2 份）。**注意**：参数结构体只服务 JSON Schema 生成——运行时由 `cargo_subcommand.rs` 与 `test_result_cache.rs` 从 `serde_json::Value` 手动取值，不反序列化到结构体。另一种收敛形态是**合并为 `cargo` + `action` 选择器**（非 `run_command` 路线；同类先例 `git` / `gh_pr` / `code_nav`），阻碍项见 §3.2.1。 |
| Rust 编译 | ~~`rust_rustc`~~ | `rustc` 已在白名单；含参数安全上限（64 个 / 8192 字节）。**已收敛 2026-10-16**（改用 `run_command`；`rustc` 已在 `allowed_commands` 白名单；`rustc` 前缀本就不在并行只读批；`rust_rustc_try` 无聚合器复用，整函数删除）。 |
| Go | ~~`go_build`~~ / ~~`go_test`~~ / ~~`go_vet`~~ / ~~`go_fmt_check`~~ / ~~`golangci_lint`~~ | 单条 `go` / `gofmt` / `golangci-lint`。**整族已收敛 2026-10-12**（改用 `run_command`；`go` / `gofmt` / `golangci-lint` 均在 `allowed_commands` 白名单；`go_` 前缀本就不在并行只读批）。 |
| Python | ~~`ruff_check`~~ / ~~`mypy_check`~~ / `pytest_run` / ~~`uv_sync`~~ / ~~`uv_run`~~ | 单条 `ruff` / `mypy` / `python3 -m pytest` / `uv`；`pytest_run` 关联后台任务。**四个已收敛 2026-10-12**（`ruff_check` / `mypy_check` / `uv_sync` / `uv_run`，改用 `run_command`；`ruff` / `mypy` / `uv` 均在 `allowed_commands` 白名单；`ruff_` / `mypy_` / `uv_` 前缀本就不在并行只读批）。评估确认 `ruff_check` / `mypy_check` 函数体被 `ci_tools` / `quality_tools` / `run_lints` 复用，故**保留函数体**、仅删 ToolSpec / runner / 参数结构体；`uv_sync` / `uv_run` 无内部复用则整函数删除。`pytest_run` 因后台任务装配**保留**。**后续更新**：`ci_tools` / `quality_tools` 已整体删除，`mypy_check` 随之移除，仅 `ruff_check` 仍由 `run_lints` 复用。 |
| Git 只读 | `git_status` / `git_diff` / `git_log` / `git_show` / `git_blame` / `git_file_history` / `git_branch_list` / `git_remote_status` / `git_remote_list` / `git_clean_check` | **§8.5 决策：保留**（审查工作流高频，只读语义价值高）；此处仅登记为「视情况」。**后续更新**：这 10 个已合并为单个 **`git`** 工具的 **`action`** 选择器（见 `CHANGELOG.md`）——保留的是「只读能力」，不是「10 个工具名」 |
| 质量 | ~~`pre_commit_run`~~ | `pre-commit run`。**已收敛 2026-10-12**（改用 `run_command`；`pre-commit` 已在 `allowed_commands` 白名单；`pre_commit` 前缀本就不在并行只读批；`precommit_tools.rs` 整文件删除，无聚合器复用；原「无 `.pre-commit-config.yaml` 时跳过」保护消失）。 |
| 格式（只读） | ~~`format_check_file`~~ | 扩展名 → 格式化器**路由**属增值；`format_files` 为写侧但同理。**已收敛 2026-10-18**：改用 `run_command`（各格式化器 `--check`，如 `clang-format --dry-run --Werror`）；`format_files` 保留。**2026-10-20 追加收敛**：`format_files` 亦已移除——其 `rustfmt` 分支未传 `--edition`（rustfmt 直调默认 2015，遇 edition 2024 语法报 `let chains are only allowed in Rust 2024 or later`），改用 `run_command`（`.rs` 走 `cargo fmt`，其余格式化器同上），并补 `prettier` 入白名单 |
| 系统查询 | ~~`package_query`~~ / ~~`port_check`~~ / ~~`process_list`~~ | 只读，但含抽象 / 输出后处理，收敛会丢结构化输出。**已收敛 2026-10-16**（改用 `run_command`；`dpkg-query` / `rpm` / `ss` / `lsof` / `ps` 均在 `allowed_commands` 白名单；三者前缀本就不在并行只读批；实现无聚合器复用，`package_query.rs` / `process_tools.rs` 整文件删除）。 |
| GitHub CLI | ~~`gh_pr_list`~~ / ~~`gh_pr_view`~~ / `gh_pr_checks` / ~~`gh_pr_diff`~~ / ~~`gh_issue_list`~~ / ~~`gh_issue_view`~~ / ~~`gh_run_list`~~ / ~~`gh_run_view`~~ / `gh_run_failure_summary` / ~~`gh_release_list`~~ / ~~`gh_release_view`~~ / ~~`gh_search`~~（读类）；`gh_pr_create` / `gh_pr_merge` / `gh_pr_review` / `gh_pr_comment` / `gh_pr_edit` / `gh_issue_create` / `gh_run_rerun` / `gh_release_create`（写类） | **注**：`github_cli/common.rs` 的 `run_gh_vec` **本身已构造 `{command:"gh",args:[…]}` 调 `command::run`**——读类已是 `run_command` 之上的一层；真正增值为参数校验、`--json` 结构化与 body 走临时文件。**读类纯薄封装 10 个已收敛 2026-10-07**（`gh_pr_list` / `gh_pr_view` / `gh_pr_diff` / `gh_issue_list` / `gh_issue_view` / `gh_run_list` / `gh_run_view` / `gh_release_list` / `gh_release_view` / `gh_search`），改用 `run_command`；保留读类增值 2 个（`gh_pr_checks` / `gh_run_failure_summary`）、写类 8 个，以及 §3.3 的 `gh_pr_body_draft` / `gh_api`。**注（2026-10-20）**：这批工具已按「多动作合并」收敛为 5 个——`gh_pr`（`action` = `checks` / `create` / `merge` / `review` / `comment` / `edit` / `body_draft`）、`gh_run`（`action` = `rerun` / `failure_summary`）、`gh_issue_create`、`gh_release_create`、`gh_api`。合并保持子实现与校验器不变，代价是丢失工具粒度的只读身份（详见 `CHANGELOG.md`）。**现状口径（2026-10-27）**：本行前文的「保留读类增值 2 个 + 写类 8 个」是**合并前**的分类；合并后注册表实际只有 5 个 gh 工具（`gh_pr` / `gh_run` / `gh_issue_create` / `gh_release_create` / `gh_api`），请以此为准。 |

**原白名单外 CLI（已扩白名单并收敛，2026-10-18）**：

| 工具 | 底层 CLI | 说明 |
|------|----------|------|
| ~~`typos_check`~~ / ~~`codespell_check`~~ | `typos` / `codespell` | **已收敛 2026-10-18**：`typos` / `codespell` 已加入 `allowed_commands`；删 ToolSpec + runner + summary，改用 `run_command`。**函数体与参数结构体保留**供 `docs_health_sweep` 复用。失去路径安全 / 存在性校验与只读语义 |
| ~~`ast_grep_run`~~ / ~~`ast_grep_rewrite`~~ | `ast-grep`（不扩 `sg`，避免与系统 switch-group 工具冲突） | **已收敛 2026-10-18**：`ast-grep` 已加入 `allowed_commands`；删 ToolSpec + runner + summary + 参数结构体（`AstGrepRunArgs` / `AstGrepRewriteArgs`）+ 全部实现，改用 `run_command`。`ast_grep_rewrite` 的写盘 `confirm` 门消失（`ast-grep run --rewrite` 须自行加 `--update-all`） |

### 3.2.1 `cargo` 主链路合并为 `action` 选择器（未落地，2026-10-27 盘）

§3.2 的 cargo 主链路不走 `run_command` 路线，但可走**多动作合并**路线（`git` / `gh_pr` / `code_nav` 先例）：四个 `ToolSpec` 合成一个 `cargo`，`action` = `check` / `clippy` / `test` / `run`；`rust_test_one` 本质是 `action=test` + 必填 `test_name`，可一并并入。

**收益**：工具数 4→1（并入 `rust_test_one` 则 5→1）；schema 由三份并成一份，`release` / `package` / `bin` / `features` 只写一遍；4 个 runner 与 4 条 `Static` summary 收为一份动作分发。**底层无需改动**——§3.2 已说明实现共用 `run_cargo_subcommand_*`。

**已核实的阻碍项**（按严重度）：

| # | 耦合点 | 位置 | 说明 |
|---|--------|------|------|
| 1 | `background_job_async_tools` **按工具名匹配** | `tool_registry/execute/execute_background_tool_async.inc.rs`（`"cargo_test" => Some(…)`） | 合并后工具名只剩 `cargo`；用户既有配置 `background_job_async_tools = ["cargo_test"]` 会**静默失效**，属配置行为回归而非重构。需把配置语义扩为「工具 + 动作」，或保留 `cargo_test` 别名 |
| 2 | 按工具名覆盖的墙钟超时 | `registry_policy::parallel_tool_wall_timeout_secs(cfg, "cargo_test")` | 同上，按名查表 |
| 3 | 只读身份变粗 | `builtin_write_effect_tools()` **不含**任何 `cargo_*` | 四个工具当前**全部算只读**（`cargo_run` 亦然，属既有分类瑕疵）。合并后要么整族维持只读（`cargo_run` 混入更别扭），要么整个 `cargo` 进写名单——后者让 `check` / `clippy` / `test` 丢掉只读重试、`Plan`·`Ask` 门控与 TTL 缓存（与 `gh_*` 合并同型代价） |
| 4 | 并行只读批前缀失配 | `builtin_parallel_sync_prefix_hit` 的前缀表含 `"cargo_"`（带下划线） | 改名成 `cargo` 后**不再命中**，会重新进入并行批判定，须同步补 `"cargo"` |
| 5 | 工作流 / fixtures 大面积引用 | `cm_workflow/workflow_templates.rs`（`rust_ci_light` 三节点）、`cm_workflow/tests.rs`、`examples/workflows/*.yaml`、`fixtures/workflows/*.yaml` | 10+ 处 `tool: cargo_check` / `cargo_clippy` / `cargo_test` 需逐一改写，并同步 `.expected.json` |
| 6 | 按名 `match` 的语义分支 | `auto_summarize_experience.rs`、`outer_loop_build_idle.rs`、`dev_tag.rs`、`step_executor_policy.rs` | 构建/质量识别、经验提炼、计划执行器测试运行器名单各有一份工具名清单 |

**不受影响**：`error_code`（`cargo_check_failed` 等）由工具名自动派生，合并后自然成为 `cargo_failed`，无需额外改动；`test_result_cache` 的指纹 `cargo_test_args_fingerprint` 只需把 `action` 纳入指纹输入。

**建议路径（2026-10-27 复核后修正）**：原「三份参数结构体改为 `#[serde(flatten)] common: CargoCommonCliArgs`、零行为变化、测试无需改」的说法**不成立**，复核发现三点：

1. **实际重复只有 2 份**：`cargo_check` / `cargo_clippy` 已共用 `CargoCommonCliArgs`，重复的是 `CargoTestArgs` / `CargoRunArgs`。
2. **参数结构体只服务 schema**：运行时由 `cargo_subcommand.rs`（`v.get("all_targets")` 等）与 `test_result_cache.rs` 从 `serde_json::Value` 手动取值，不反序列化到结构体。所以 flatten 不涉及字段访问路径（原判断的成本高估了），但**唯一效果就是改 schema**（收益也高估了）。
3. **`all_targets` 是此前漏掉的新阻碍**：它只对 `check` / `clippy` 生效（`cargo_subcommand.rs`：`if o.all_targets && matches!(subcmd, "check" | "clippy")`），却被 `CargoCommonCliArgs` 与四个公共字段混在一处。直接 flatten 会让 `cargo_test` / `cargo_run` 的 schema 多出一个传了也无效的属性，属工具契约变差。

若仍要推进，正确形态是先拆一层：`CargoPackageCliArgs { release, package, bin, features }`，再由 `CargoCommonCliArgs`（= 它 + `all_targets`）、`CargoTestArgs`、`CargoRunArgs` 各自 flatten。但**净收益约 0~2 行**（新增结构体 6 行 + 2 处 flatten 注解，抵消原有 8 行），且需实测 schemars 对 flatten 的输出（是否引入 `allOf` / `$ref`）。

**当前结论：不做参数收敛。** 8 行注册层重复换取 schema 语义清晰，这个交易划算；是否承担上表 6 项（工具名合并），留待单独决策。

### 3.3 保留（无 CLI 对应 / 结构化 / 编排 / 状态 / 网络 / 高增值解析）

- **文件与结构化**：全部 `file_core` / `file_extra`（`create_file` / `modify_file` / `copy_file` / `move_file` / `delete_files` / `delete_dir` / `append_file` / `create_dir` / `search_replace` / `chmod_file` / `apply_patch` / `read_file` / `read_dir` / `glob_files` / `list_tree` / `read_binary_meta` / `hash_file` / `search_in_files` / `codebase_semantic_search`）、`structured_*`、`markdown_check_links`。`symlink_info` / `file_exists` / `extract_in_file` 已于 2026-10-18 移除（改用 `run_command readlink`·`realpath`·`stat` 与 `search_in_files`）。
- **代码导航 / 度量**：`code_nav`（`find_symbol` / `find_references` / `file_outline` / `call_graph_sketch` 四个只读工具已于 2026-10-25 合并为一个 `action` 选择器）、`code_stats` / `dependency_graph` / `coverage_report`。
- **归档**：`archive_pack` / `archive_unpack` / `archive_list`（用 Rust crate，非 CLI）。
- **状态 / 记忆 / 配置**：`reminder` / `event`（reminders / events，2026-10-24 由九个工具合并为两个 `action` 选择器）、`long_term_*`、`self_config_info`、`skill_manage`。
- **诊断 / 文档渲染**：`diagnostic_summary` / `present_clarification_questionnaire` / `repo_overview_sweep` / `crate_contract_map` / `summarize_experience` / `gh_pr_body_draft`（`changelog_draft` / `license_notice` 已于 2026-10-27 **整工具删除**，平替为 `run_command` / `gh_release_create` 的 `auto_notes` / `cargo deny check licenses`；`error_output_playbook` 同日整工具删除，平替为模型自行归类 + `run_command`）。
- **Rust 专属**：`rust_analyzer_*`（LSP stdio，非白名单 CLI，不可替代）。`rust_compiler_json` 已于 2026-10-27 **整工具删除**（其 JSON 输出里的 `rendered` 就是 cargo 自己的人类可读块，与只读的 `cargo_check` 重合）；`rust_backtrace_analyze` 同日**整工具删除**（纯计算分析器，模型可直读 panic 文本，或用 `run_command`（`c++filt` 在白名单）+ `code_nav` 定位帧）。
- **纯计算 / 文本**：`text_transform` / `json_format` / `regex_test` / `env_var_check` / `text_diff` / `table_text` / `date_calc`（`todo_scan` 已于 2026-10-18 移除，改用 `search_in_files`）。原 `get_current_time` / `calc` / `convert_units` 已于 2026-10-24 **整体移除**（`date` 已在白名单可代 `get_current_time`；`calc` / `convert_units` 由 `python_snippet_run` 或 `run_command` 承担 —— 代价是每次计算多一轮工具往返，且失去 `bc` 的任意精度与 `uom` 的类型安全）。**`date_calc` 保留**：它不止取当前时间，还提供 `offset`（`+30d` / `-2w` / `+1m`）与 `diff`（两日期间隔）两种模式，`date` 单条命令无法表达，且纯计算无 CLI 依赖。
- **网络 / 执行 / 会话**：`get_weather` / `web_search` / `http_fetch` / `http_request` / `terminal_session` / `background_job`（`status` / `list` / `output` 三个只读动作已合并为一个 `action` 选择器，2026-10-24）+ `background_job_cancel`（写副作用，保持独立） / `workflow_execute` / `run_command`。
- **聚合编排**：`run_lints` / `docs_health_sweep`；原 `ci_pipeline_local` / `release_ready_check` / `quality_workspace` 已于后续整体删除，改用 `run_command`；`playbook_run_commands`（原 `error_output_playbook` 启发式的执行半体）已于 2026-10-27 整工具删除。
- **校验密集 / 安全面敏感**：`gh_api`——含 `validate_api_path`（禁 `/` 开头、禁 `..`、字符白名单）+ 方法白名单（GET/HEAD/POST/PATCH/PUT/DELETE）+ body 须合法 JSON + **body 经 stdin 传入**；降级 `run_command` 无 stdin 通道、无 path 限制，属**扩大安全面**，不收敛（`src/cm_tools/tools/github_cli/api.rs:101-157`）。

---

## 4. 建议切片顺序

1. ~~**§3.1.1 试点**：`cargo_clean`~~（**已落地 2026-10-07**：删 ToolSpec + runner + `cargo_clean_try` + `CargoCleanArgs`，并同步 tests / `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`；`fmt` / `clippy -D warnings` / `cargo test` 全绿）。
2. ~~**§3.1.2 写侧安全门族**（`cargo_fix` / `go_mod_tidy` / `python_install_editable`）~~（**已落地 2026-10-07**：按「直接收敛、不新增机制」删 ToolSpec + runner + 实现 + 参数结构体（含 `PythonInstallBackend` / `summary_python_install_editable`），并同步 `registry_policy` / `codebase_semantic_invalidation` / `dev_tag` / tests / `docs/工具说明.md` / `docs/en/TOOLS.md` / `docs/待办清单.md` / `CHANGELOG.md`；`fmt` / `clippy -D warnings` / `cargo test` 全绿）。
3. ~~**§3.2 cargo 元数据 / 审计族**（`cargo_metadata` / `tree` / `doc` / `nextest` / `outdated` / `machete` / `udeps` / `publish_dry_run` / `fmt_check` / `audit` / `deny`）：低频只读，接受 §2.1 代价。~~ **Slice A 已落地 2026-10-07**（7 个元数据 / 旁路工具）；**Slice B 已落地 2026-10-07**（`cargo_fmt_check` / `cargo_nextest` / `cargo_audit` / `cargo_deny`，并改写内置工作流模板 `rust_ci_fmt` 与 fixtures / examples）。
4. ~~**§3.2 Go / Python 族**：按频率评估；`cargo_test` / `pytest_run` 收敛前须核对后台任务装配。~~ **Go 族已落地 2026-10-12**（`go_build` / `go_test` / `go_vet` / `go_fmt_check` / `golangci_lint`，接受 §2.1 代价；Go 族无后台任务装配耦合）。**Python 族已落地 2026-10-12**（`ruff_check` / `mypy_check` / `uv_sync` / `uv_run` 四个只读薄封装改用 `run_command`；评估确认 `ruff_check` / `mypy_check` 函数体被 `ci_tools` / `quality_tools` / `run_lints` 复用，故**保留函数体**、仅删 ToolSpec / runner / 参数结构体，`uv_sync` / `uv_run` 无复用则整函数删除；`pytest_run` 因后台任务装配**保留**；接受 §2.1 代价，`ruff_` / `mypy_` / `uv_` 前缀本就不在并行只读批）。**质量族 `pre_commit_run` 已落地 2026-10-12**（改用 `run_command`；`pre-commit` 已在 `allowed_commands` 白名单；`precommit_tools.rs` 整文件删除，无聚合器复用；原「无 `.pre-commit-config.yaml` 时跳过」保护消失；接受 §2.1 代价，`pre_commit` 前缀本就不在并行只读批）。
5. ~~**`gh_*` 家族**：读类已轻量，优先级低于 1–4；写类因类型化参数价值高，暂缓。~~ **读类纯薄封装 10 个已落地 2026-10-07**（`gh_pr_list` / `gh_pr_view` / `gh_pr_diff` / `gh_issue_list` / `gh_issue_view` / `gh_run_list` / `gh_run_view` / `gh_release_list` / `gh_release_view` / `gh_search`，接受 §2.1 代价；`gh_` 前缀本就不在并行只读批）。保留读类增值 2 个（`gh_pr_checks` / `gh_run_failure_summary`）、写类 8 个与 `gh_pr_body_draft` / `gh_api`；写类因类型化参数价值高，**暂缓**。
6. ~~**白名单外 CLI（`typos` / `codespell` / `ast-grep`）**：仅在决定扩白名单后再评估（风险最高，暂不推进）。~~ **已落地 2026-10-18**：先向 `allowed_commands` 扩入 `typos` / `codespell` / `ast-grep`（123 → 126），再删除 `typos_check` / `codespell_check` / `ast_grep_run` / `ast_grep_rewrite` 四个薄封装；typos/codespell 函数体保留供 `docs_health_sweep` 复用。

保留项（§3.3，含 `gh_api`）不动（`playbook_run_commands` 已于 2026-10-27 整工具删除）；Git 只读（§3.2）维持 §8.5 的**保留**决策。

---

## 5. 修订记录

| 日期 | 摘要 |
|------|------|
| 2026-10-07 | 初稿：对 §8.5 之后的**剩余内置工具**逐家族盘点，给出「适合 / 视情况 / 保留」判定矩阵、收敛代价（只读语义 / 白名单 / 后台任务 / 运行时差异）与建议切片顺序。 |
| 2026-10-07 | 修订：§3.1 收窄为「写侧薄封装（无只读损失，但非零损失）」，拆 **3.1.1 首推（`cargo_clean`）** / **3.1.2 含 `confirm` 门或校验（`cargo_fix` / `go_mod_tidy` / `python_install_editable`）**；`gh_api`（校验密集 + stdin body，降级扩大安全面）与 `playbook_run_commands`（启发式执行半体，非薄封装）移入 §3.3 保留；§4 顺序同步调整。 |
| 2026-10-07 | **§3.1.1 `cargo_clean` 试点已落地**：删除 ToolSpec + runner + `cargo_clean_try` + `CargoCleanArgs` + 写副作用 / `dev_tag` / tests 引用，改用 `run_command`；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-07 | **§3.1.2 写侧安全门族已落地**：按「直接收敛、不新增机制」删除 `cargo_fix` / `go_mod_tidy` / `python_install_editable` 的 ToolSpec + runner + 实现 + 参数结构体（含 `PythonInstallBackend` / `summary_python_install_editable`）及写副作用 / `dev_tag` / tests 引用，改用 `run_command`；复核确认 `confirm` 门并非强制安全边界；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md` / `tool_calling_evolution.md`。 |
| 2026-10-07 | **§3.2 cargo 元数据 / 旁路族 Slice A 已落地**：删除 `cargo_metadata` / `cargo_tree` / `cargo_doc` / `cargo_outdated` / `cargo_machete` / `cargo_udeps` / `cargo_publish_dry_run` 七个只读薄封装的 ToolSpec + runner + 实现（`cargo_metadata_try` 等）+ 参数结构体 + 写副作用 / `dev_tag` / tests 引用，改用 `run_command`；`cargo_metadata_command` 等内部共享 helper 保留；删除 `/health` 的 `cargo-machete` / `cargo-udeps` 探测（`dep_cargo_machete` / `dep_cargo_udeps`）；`cargo_nextest` / `cargo_fmt_check` / `cargo_audit` / `cargo_deny` 留待 Slice B（须改写内置工作流模板 `rust_ci_fmt`）；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-07 | **§3.2 cargo 质测 / 审计族 Slice B 已落地**：删除 `cargo_fmt_check` / `cargo_nextest` / `cargo_audit` / `cargo_deny` 四个薄封装的 ToolSpec + runner + 实现 + 参数结构体（`CargoNextestArgs` / `CargoAuditArgs` / `CargoDenyArgs`）+ 写副作用 / `dev_tag` / tests 引用，改用 `run_command`；内部复用函数体保留（`ci_tools::cargo_fmt_check_tool`、`cargo_tools::cargo_fmt_check`、`security_tools::cargo_audit` / `cargo_deny` 供 `ci_pipeline_local` / `release_ready_check` 聚合器调用）；改写内置工作流模板 `rust_ci_light` 的 `rust_ci_fmt` 节点为 `run_command`（`cargo fmt --check`），并同步 `cm_workflow/tests.rs`、`examples/workflows/choice_lint.yaml`、`fixtures/workflows/07_choice_node.yaml` 与 `.expected.json`；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `docs/工作流编写教程.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-07 | **§3.2 `gh_*` 读类纯薄封装已落地**：删除 `gh_pr_list` / `gh_pr_view` / `gh_pr_diff` / `gh_issue_list` / `gh_issue_view` / `gh_run_list` / `gh_run_view` / `gh_release_list` / `gh_release_view` / `gh_search` 十个只读薄封装的 ToolSpec + runner + 实现（`pr_issue.rs` / `run_release_search.rs` 整文件删除）+ 参数结构体（含 `GhPrState` / `GhIssueState` / `GhSearchScope`）+ `dev_tag` / `tool_summary_args` / tests 引用，并用 `run_command` 替代；`common.rs` 中仅供这些工具使用的 helper / 常量一并删除，迁移有效单测（`common.rs` 6 个 + `pr_workflow.rs` 1 个）；保留读类增值 2 个（`gh_pr_checks` / `gh_run_failure_summary`）、写类 8 个与 `gh_pr_body_draft` / `gh_api`；同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。接受 §2.1 代价（`gh_` 前缀本就不在并行只读批）。 |
| 2026-10-12 | **§3.2 Go 族已落地**：删除 `go_build` / `go_test` / `go_vet` / `go_fmt_check` / `golangci_lint` 五个只读薄封装（整文件删 `go_tools.rs`、`specs/go.inc.rs`、`specs/golangci.inc.rs`）的 ToolSpec + runner + 参数结构体（`GoBuildArgs` / `GoTestArgs` / `GoVetArgs` / `GoFmtCheckArgs` / `GolangciLintArgs`）+ `dev_tag` / `tool_json_schema` / 计划执行器测试运行器名单 / 构建进展识别引用，改用 `run_command`；复核确认 `./...` 能通过 `run_command` 参数安全校验（`arg_has_parent_dir_ref` 只拦 `..`），且 Go 族无后台任务装配耦合；接受 §2.1 代价（`go_` 前缀本就不在并行只读批）。同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-12 | **§3.2 Python 族已落地**：删除 `ruff_check` / `mypy_check` / `uv_sync` / `uv_run` 四个只读薄封装的 ToolSpec（`specs/frontend_python.inc.rs`）+ runner 绑定 + 参数结构体（`RuffCheckArgs` / `MypyCheckArgs` / `UvSyncArgs` / `UvRunArgs`）+ `uv_run` summary 绑定与 `UvRunSummaryArgs` + `dev_tag` / tests 引用，改用 `run_command`（`ruff` / `mypy` / `uv` 均在白名单）；评估确认 `ruff_check` / `mypy_check` 函数体被 `ci_tools.rs` / `quality_tools/mod.rs` / `lint.rs`（`run_lints`）复用，故**保留 `python_tools::ruff_check` / `mypy_check` 公开函数体**，仅删其 ToolSpec / runner / Args；`uv_sync` / `uv_run` 无内部复用则整函数删除（连同 `MAX_UV_RUN_*` 常量、`collect_uv_run_argv` / `is_safe_uv_run_arg` helper 与相关单测）；`run_ruff_check` 系聚合器**参数字段名**（非工具名），5 处引用保留；`pytest_run` 因后台任务装配**保留**；接受 §2.1 代价，`ruff_` / `mypy_` / `uv_` 前缀本就不在并行只读批。同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-18 | **A 档低风险 5 个已移除**：删除 `todo_scan` / `extract_in_file` / `file_exists` / `symlink_info` / `format_check_file` 的 ToolSpec + runner + summary 映射 + 参数结构体（`TodoScanArgs` / `ExtractInFileArgs` + `ExtractInFileMode` / `FileExistsArgs` + `FileExistsKind` / `SymlinkInfoArgs` / `FormatOnePathArgs`）+ 实现（`todo_scan.rs` / `file/extract.rs` / `file/symlink.rs` / `file/rust_brace_scan.rs` 整文件删除；`file/inspect.rs::file_exists` 与 `format.rs::run_check` 删除）+ `dev_tag` / 策略名单 / schema 测试 / 文档引用；替代方式：`run_command stat`、`run_command readlink`·`realpath`、`search_in_files`、各格式化器 `--check`。`format_files` 与 `run_formatter(..., check_only)` 保留（**`format_files` 已于 2026-10-20 移除，见上表**）。 |
| 2026-10-18 | **B 档白名单外 CLI 已落地（先扩白名单再删）**：先向 `config/tools.toml` 的 `allowed_commands` 扩入 `typos` / `codespell` / `ast-grep`（123 → 126，不扩 `sg`），再删除 `typos_check` / `codespell_check` / `ast_grep_run` / `ast_grep_rewrite` 四个薄封装的 ToolSpec（`specs/docs_health.inc.rs`（原 `quality_ast.inc.rs`，重命名后仅留 `docs_health_sweep`）+ runner + summary（含 `AstGrepRunSummaryArgs` / `AstGrepRewriteSummaryArgs`）+ 参数结构体（`AstGrepRunArgs` / `AstGrepRewriteArgs`）+ 全部 ast-grep 实现（`spell_astgrep_tools.rs` → `spell_tools.rs`）+ `dev_tag` / 写副作用名单 / 并行前缀 / 记忆失效填充 / schema 测试引用，改用 `run_command`。**`typos_check` / `codespell_check` 函数体与 `TyposCheckArgs` / `CodespellCheckArgs` 保留**供 `docs_health_sweep` 复用。失去路径安全 / 存在性校验、只读语义，以及 `ast_grep_rewrite` 的写盘 `confirm` 门。 |
| 2026-10-12 | **§3.2 质量族 `pre_commit_run` 已落地**：删除这一只读薄封装的 ToolSpec（`specs/quality_ast.inc.rs`）+ runner 绑定 + 参数结构体（`PreCommitRunArgs`）+ summary 绑定与 `PreCommitRunSummaryArgs` + 实现整文件（`precommit_tools.rs`，含 `has_precommit_config` / `parse_pre_commit_json` / `validate_pre_commit_hook_id` / `apply_pre_commit_cli_flags` / `is_safe_hook_id` / `parse_files_array` 及相关单测）+ `dev_tag` / tests 引用，改用 `run_command`（`pre-commit` 已在白名单）；复核确认实现文件**无任何聚合器复用**，故整文件删除；接受 §2.1 代价，`pre_commit` 前缀本就不在并行只读批；原「无 `.pre-commit-config.yaml` 时跳过」保护消失。同步 `docs/工具说明.md` / `docs/en/TOOLS.md` / `CHANGELOG.md` / `docs/待办清单.md`。 |
| 2026-10-27 | **现状复核（纯文档对齐，无代码改动）**：①§3.2「cargo 主链路」补明五个工具**均未收敛**、但**底层实现已共用** `run_cargo_subcommand_str_try` / `_value_try`（差异只是 `sub` 字符串），重复集中在注册层（4 个 `ToolSpec` + 4 个 runner + 3 份参数结构体各写一遍 `release` / `package` / `bin` / `features`）；新增 **§3.2.1** 固化「合并为 `cargo` + `action` 选择器」的 6 项已核实阻碍（`background_job_async_tools` 按名匹配致配置静默失效、按名超时覆盖、只读身份变粗、`cargo_` 前缀失配、工作流 / fixtures 10+ 处引用、按名 `match` 分支）与「先做零行为变化的参数结构体 `serde(flatten)` 收敛」的建议路径。②§3.2 GitHub CLI 行补「现状口径」：合并后注册表只有 5 个 gh 工具，前文「保留读类 2 个 + 写类 8 个」系**合并前**分类。③§3.3 补漏记的 `apply_patch`（在 `builtin_write_effect_tools()` 内，原清单未列）与 `date_calc`（`get_current_time` 移除后仍在注册表，文档从未记录其 `offset` / `diff` 两种模式）。 |
| 2026-10-27 | **上条①②的「3 份参数结构体」与「零行为变化的 `serde(flatten)` 收敛」两条结论复核后修正（纯文档，无代码改动）**：① `cargo_check` / `cargo_clippy` 已共用 `CargoCommonCliArgs`，实际重复只有 `CargoTestArgs` / `CargoRunArgs` 两份；② 参数结构体只服务 schema，运行时由 `cargo_subcommand.rs` / `test_result_cache.rs` 从 `serde_json::Value` 手动取值（不反序列化），故成本（字段访问路径）与收益（去重）均被高估；③ 新增阻碍：`all_targets` 只对 `check` / `clippy` 生效，flatten 进 test/run 会让 schema 多出一个传了也无效的属性。**结论改为不做参数收敛**，并补上正确形态（先拆 `CargoPackageCliArgs`）与净收益估算（0~2 行）。§3.2 同表措辞一并校正。 |
