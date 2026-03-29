# CrabMate 内置工具参考

本文档列出各内置工具的能力说明，以及常见 Function Calling JSON 参数示例与发布检查排障指引。Web 工作区、流式与会话导出等行为说明仍在根目录 [`README.md`](../README.md)「功能概览」中；日常使用与入门亦见该文件。

**可选「解释卡」**（配置 `tool_call_explain_enabled`）：启用后，凡**非只读**内置工具（与 `tool_registry::is_readonly_tool` 一致，含 `run_command`、`run_executable`、写文件、`http_request`、git 写操作、`workflow_execute` 等）调用时，须在 JSON **顶层**增加字符串字段 **`crabmate_explain_why`**，用一句自然语言说明本步目的；服务端校验长度后**执行前会剥离**该键，避免与 `additionalProperties: false` 冲突。只读工具与 MCP 代理工具不要求；MCP 调用仍会剥离该键再转发。与命令/HTTP **审批**互补（审批管授权，解释卡管可理解性）。

**长输出进模型上下文**（`tool_result_envelope_v1` 默认开启）：写入历史的 `role: tool` 为 **`crabmate_tool`** JSON 信封（`summary`、`output` 等）。另含 **`tool_call_id`**、**`execution_mode`**（`serial` / `parallel_readonly_batch`）、**`parallel_batch_id`**（同批并行只读工具共享）；失败时含 **`retryable`**（与 `error_code` 配套的**启发式**，非保证）。每次请求模型前若超过 **`tool_message_max_chars`**，服务端对 **`output`** 做**首尾采样**并设置 **`output_truncated`**、**`output_original_chars`**、**`output_kept_head_chars`**、**`output_kept_tail_chars`**，避免单次 grep/构建日志撑满上下文；完整原文仍可通过 SSE/导出等在会话中查看（视 UI 设置）。SSE **`tool_result`** 事件含相同关联字段，与信封对齐。详见 **`docs/DEVELOPMENT.md`**、**`docs/SSE_PROTOCOL.md`** 与 **`docs/CONFIGURATION.md`**。

**终答 `agent_reply_plan` v1**（在工作流反思等路径下由服务端校验）：`steps[].id` 须唯一且符合稳定字符规则（见 **`docs/DEVELOPMENT.md`**）；可选 **`workflow_node_id`** 用于与最近一次 **`workflow_execute`** 工具结果里的 **`nodes[].id`** 对齐（子集校验），便于规划与 DAG 节点一一对应。

**可选 Docker 沙盒**（`sync_default_tool_sandbox_mode = docker`）：**SyncDefault** 与 **`run_command` / `run_executable` / `get_weather` / `web_search` / `http_fetch` / `http_request`** 在宿主完成审批/白名单后可在 **Docker 容器**内执行（经 **bollard** 调 Engine API）；**`workflow_execute`** 与 **MCP** 仍只在宿主。需配置镜像名且本机 **Docker 守护进程可访问**（通常与 `docker` CLI 共用 Unix 套接字）。详见 [`docs/CONFIGURATION.md`](CONFIGURATION.md)「SyncDefault 工具 Docker 沙盒」。

## 内置工具（模型可调用）

- **内置多种工具，由模型按需调用**：
  - `get_current_time`：获取当前日期时间。
  - `calc`：使用 Linux 的 `bc -l` 执行数学表达式（四则、乘方 ^、sqrt/sin/cos/tan/ln/exp、pi/e 等）。
  - `convert_units`：物理量与数据量**单位换算**（Rust [`uom`](https://crates.io/crates/uom) 库，不调用外部程序）。`category` 含 length / mass / temperature / data / time / area / pressure / speed（或中文别名），`value` + `from` + `to` 指定数值与单位；数据量区分十进制 KB/MB/GB 与二进制 KiB/MiB/GiB。
  - `get_weather`：获取指定城市/地区当前天气（[Open-Meteo](https://open-meteo.com/) API，无需 Key）。
  - `web_search`：**联网网页搜索**（[Brave Search API](https://brave.com/search/api/) 或 [Tavily](https://tavily.com/)），需在配置中填写 `web_search_api_key` 并设置 `web_search_provider`（`brave` / `tavily`）；未配置 Key 时工具会返回说明性错误。仓库内搜代码请仍优先用 `search_in_files`。
  - `http_fetch`：对给定 URL 发起 **GET**（默认）或 **HEAD**。GET 返回状态、Content-Type、**重定向链**与正文（有超时与体长上限）；**HEAD** 不下载 body，仅状态码、Content-Type、Content-Length 与重定向链。URL 匹配 `http_fetch_allowed_prefixes` 的**同源 + 路径前缀边界**规则时直接执行；不匹配时，Web（`/chat/stream` 携带 `approval_session_id`）或 **CLI（repl/chat）** 可人工审批 **拒绝 / 本次允许 / 永久允许**（GET/HEAD 共用同一归一化白名单键；CLI 见 `runtime::cli_approval`）。
  - `http_request`：对给定 URL 发起 **POST / PUT / PATCH / DELETE**（可选 `json_body`）。受 `http_fetch_allowed_prefixes` 约束（同源 + 路径前缀边界）；匹配则直接执行，**未匹配**时 Web（`/chat/stream` + `approval_session_id`）与 **CLI（repl/chat）** 可与 `http_fetch` 一样走 **拒绝 / 本次允许 / 永久允许**（永久键为 `http_request:<METHOD>:<URL>`，与 `http_fetch:` 键区分）。**`workflow_execute` 节点**内仍仅白名单前缀（同步路径无审批）。返回状态、Content-Type、重定向链与正文预览（默认建议先 dry-run，不在 body 中放真实密钥）。
  - `run_command`：执行白名单内的只读/查询类 Linux 命令（`ls`、`pwd`、`whoami`、`date`、`cat`、`file`、`head`、`tail`、`wc`、`cmake`、`ninja`、`gcc`、`g++`、`clang`、`clang++`、`c++filt`、`autoreconf`、`autoconf`、`automake`、`aclocal`、`make`，以及 **GNU Binutils 常用只读分析**：`objdump`、`nm`、`readelf`、`strings`、`size`；开发环境默认另含 `ar` 等），带超时与输出截断。**CMake**：已列入白名单，常用 `cmake -S . -B build`、`cmake --build build`；参数不得含 `..` 或以 `/` 开头，建议构建目录用相对路径（勿在 args 里写绝对路径的 `-D`）。未安装时 `/health` 中 `dep_cmake` 可能为 degraded。**c++filt**：可将链接器/栈追踪中的修饰名（mangled）反解为可读 C++ 名（Binutils/LLVM 通常提供）；未安装时 `dep_cxxfilt` 可能为 degraded。**Binutils**：`objdump`/`nm`/`readelf`/`strings`/`size`（及 `ar`）未安装时 `/health` 对应 `dep_objdump` / `dep_nm` / `dep_readelf` / `dep_strings_binutils` / `dep_size` / `dep_ar` 可能为 degraded。**Autotools**：默认白名单含 `autoreconf`/`autoconf`/`automake`/`aclocal`，便于维护仍使用 `configure.ac` / `Makefile.am` 的仓库；会处理项目内 m4/shell，仅应在**信任的工作区**使用，且 `run_command` 参数规则仍生效。
  - `run_executable`：在工作区目录下按**相对路径**运行可执行文件（如 `./main`、编译产物）；与 `run_command`（仅白名单系统命令）分工——**运行当前目录/工作区内的程序请用本工具**，不要用 `run_command`。
  - `package_query`：只读查询 Linux 包信息（apt/rpm 统一抽象）：是否安装、版本、来源。支持 `manager=auto|apt|rpm`（默认 `auto`，优先 `dpkg-query` 后尝试 `rpm`），不执行安装/卸载操作。
  - `delete_file`（需 `confirm`）/ `delete_dir`（需 `confirm`，可选 `recursive`）：删除文件或目录。
  - `append_file`：追加内容到已有文件末尾（`create_if_missing` 可选新建）。
  - `create_dir`：创建目录（默认 `parents=true`，类似 `mkdir -p`）。
  - `search_replace`：单文件搜索替换（字面量或正则，默认 `dry_run` 预览，写盘需 `confirm`）。
  - `create_file` / `modify_file`：创建或修改文件；`read_file` 支持分段与行上限及 **`encoding`**（`utf-8` 严格、`utf-8-sig`、`gb18030`/`gbk`/`big5`、`utf-16le`/`be`、`auto` 等；非法序列报错而非静默替换）；`modify_file` 支持按行区间替换（大文件友好）。**单轮** `run_agent_turn` 内，服务端可对相同文件+相同读取参数缓存 `read_file` 正文（比对磁盘 **mtime+size**；执行写类工具或工作区变更后缓存清空），键含 **encoding**，见配置 **`read_file_turn_cache_max_entries`**。Web `GET /workspace/file` 默认仅读取不超过 **1 MiB** 的文件，正文解码规则与 `read_file` 一致，可选查询参数 **`encoding`**；超出大小返回错误（避免大文件导致内存放大）。上述及 `hash_file`、`read_binary_meta`、`format_file` 等返回说明中的路径均为**相对工作区根**（POSIX 风格），不输出本机绝对路径。
  - `copy_file` / `move_file`：在工作区内复制或移动**文件**（相对路径、防目录穿越与 symlink 逃逸与 `create_file` 一致）；目标已存在时默认不覆盖，需 `overwrite: true`；`move_file` 跨盘时会自动复制后删源。
  - `read_dir` / `glob_files` / `list_tree`：列单层目录；按 glob（如 `**/*.rs`）递归匹配文件路径；递归列树（`max_depth` / `max_entries` 有上限，路径不出工作区）。
  - `markdown_check_links`：扫描 Markdown（默认 `README.md` 与 `docs/`），校验**相对路径**链接与 `#fragment` 锚点；支持 `output_format=text|json|sarif`。`http(s)://` 外链默认不联网，可选 `allowed_external_prefixes` 对匹配 URL 做 HEAD 探测（同 URL 去重缓存）。
  - `typos_check` / `codespell_check`：文档拼写检查（**只读**，需本机安装 [typos](https://github.com/crate-ci/typos) / [codespell](https://github.com/codespell-project/codespell)）；默认优先检查存在的 `README.md` 与 `docs/`，可用 `paths` 收窄；`typos_check` 支持 `config_path`（项目词典通常在 `.typos.toml`），`codespell_check` 支持 `dictionary_paths`（`-I` 词典文件）与 `ignore_words_list`（`-L`）。
  - `ast_grep_run`：用 [ast-grep](https://ast-grep.github.io/) 做**语法树级**搜索（需本机安装 `ast-grep`，如 `cargo install ast-grep`）；必填 `pattern` 与 `lang`，默认在存在的 `src` 下搜索，并内置排除 `target`、`node_modules`、`.git` 等；可用 `paths` / `globs` 进一步限制范围。
  - `ast_grep_rewrite`：用 `ast-grep run --rewrite` 做结构化改写。默认 `dry_run=true` 仅预览；当 `dry_run=false` 时必须 `confirm=true` 才会实际写盘（等价 `--update-all`）。
  - `structured_validate` / `structured_query` / `structured_diff` / `structured_patch`：校验、查询、结构化 diff，以及对 **JSON / YAML / TOML** 做定点 `set/remove`（`structured_patch` 默认 dry-run，写盘需 `confirm=true`）。CSV/TSV 仍用于校验/查询/diff，不支持结构化写回。
  - `table_text`：对工作区内 **CSV / TSV / 分号或管道分隔**等表格做**预览、列数校验、按列下标导出、按列筛选、数值聚合**（流式扫描，单文件 4MiB）；与 `structured_*` 的「整表载入为 JSON + 路径查询」分工不同，按需选用。
  - `text_transform`：纯内存字符串变换（Base64、URL 百分号编解码、短哈希、按行合并/按分隔符切分），不落盘，输入/输出有长度上限。
  - `text_diff`：两段 UTF-8 文本或工作区内两文件的**行级 unified diff**（与 Git 无关）；`structured_diff` 为键级结构化差异，二者互补。
  - `changelog_draft`：根据 **git log** 生成 **Markdown 变更说明草稿**（不写仓库）；可按提交日聚合、`flat` 平铺，或按 **相邻 tag** 分段（`tag_ranges`）。
  - `license_notice`：运行 **cargo metadata**，生成 **crate → license** 的 Markdown 表（未声明项有占位说明）；**非法律意见**，发版前需人工核对。
  - `hash_file`：对工作区内文件做只读 **SHA-256 / SHA-512 / BLAKE3**（流式读取）；可选仅哈希前 `max_bytes` 字节，便于大文件或抽样校验。
  - `diagnostic_summary`：只读排障摘要——Rust 工具链（`rustc`/`cargo`/`rustup`/`bc`）、工作区 `target/` 与 `Cargo.toml` / `frontend` 常见路径、关键环境变量**是否设置**（**永不输出变量值**；密钥类亦不输出长度）。可选 `extra_env_vars`（大写安全名）。
  - `error_output_playbook`：对**已脱敏**的 rustc/cargo/npm/pytest 等错误输出做启发式**归类**，并输出 **2～3 条**可经 **`run_command`** 执行的命令**建议**（**不执行**；仅含当前白名单内命令，如默认已含 `cargo`/`git`/`python3`/`npm` 等）。可选 `ecosystem`：`auto` / `rust` / `node` / `python` / `generic`；可选 `max_chars`。内置对 `API_KEY=` 等样式的轻度掩码；粘贴前仍须人工脱敏。
  - **Python / uv / pre-commit**（均在**工作区根**执行，需本机已安装对应 CLI；未安装时工具返回说明性错误）：`ruff_check`、`pytest_run`（`python3 -m pytest`）、`mypy_check`、`python_install_editable`（`uv` 或 `pip` 可编辑安装）、`uv_sync`、`uv_run`（`args` 为字符串数组，不经 shell）、`pre_commit_run`（需 `.pre-commit-config.yaml`）。**格式化**：`format_file` / `format_check_file` 按扩展名选用 **ruff format**（`.py`）、**clang-format**（`.c` / `.h` / `.cpp` / `.cc` / `.cxx` / `.hpp` / `.hh`）、`rustfmt` / `prettier` 等。**标签裁剪**：集成方可通过库 API `build_tools_with_options` 与 `dev_tag` 子域标签（如 `python`、`cpp`、`go`、`quality`）限制发给模型的工具列表，详见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。**模型调用后端**：库 API `run_agent_turn` 的 `RunAgentTurnParams` 可选字段 `llm_backend` 可传入自定义 `ChatCompletionsBackend`（默认 OpenAI 兼容 HTTP，与 `api_base` + `API_KEY` 行为一致）；上下文摘要等路径与主对话共用同一后端，详见 `DEVELOPMENT.md`。
  - **Node.js / npm**（须存在 `package.json`）：`npm_install`（支持 `npm ci`、`--production`）、`npm_run`（运行任意 npm script）、`npx_run`（执行 npx 包命令）、`tsc_check`（TypeScript 类型检查 `tsc --noEmit`）。
  - **Go 工具链**（须存在 `go.mod`，需本机已安装 Go）：`go_build`、`go_test`（支持 `-run` / `-race` / `-timeout` 等）、`go_vet`、`go_mod_tidy`（受控写入需 `confirm`）、`go_fmt_check`（`gofmt -l` 列出未格式化文件）、`golangci_lint`。
  - `chmod_file`（需 `confirm`，仅 Unix）：修改文件权限（八进制模式如 `755`）。
  - `symlink_info`（只读）：查看符号链接目标、是否悬空、是否指向工作区外。
  - **进程与端口管理**（只读）：`port_check`（检查端口占用，使用 ss/lsof）、`process_list`（按关键词过滤进程列表）。
  - **代码度量与分析**（只读）：`code_stats`（代码行数统计，优先 tokei/cloc，回退内置统计器）、`dependency_graph`（依赖关系图，支持 Cargo/Go/npm，输出 Mermaid/DOT/tree）、`coverage_report`（覆盖率报告解析，支持 LCOV/.info、Tarpaulin JSON、Cobertura XML）。
  - **源码分析工具**（只读，需本机安装对应 CLI；未安装时工具返回说明性错误）：
    - `shellcheck_check`：使用 [ShellCheck](https://www.shellcheck.net/) 对 Shell/Bash 脚本做静态分析。可指定 `paths`（默认递归扫描 `.sh`/`.bash` 等脚本）、`severity`（error/warning/info/style）、`shell`（sh/bash/dash/ksh）、`format`。
    - `cppcheck_analyze`：使用 [cppcheck](https://cppcheck.sourceforge.io/) 对 C/C++ 代码做静态分析。可指定 `paths`、`enable`（all/style/performance 等）、`std`（如 c++17）、`platform`。
    - `semgrep_scan`：使用 [Semgrep](https://semgrep.dev/) 做多语言 SAST 安全扫描。可指定 `config`（规则集，默认 auto）、`paths`、`severity`、`lang`、`json` 输出。
    - `hadolint_check`：使用 [Hadolint](https://github.com/hadolint/hadolint) 对 Dockerfile 做 lint。可指定 `path`（默认 Dockerfile）、`format`、`ignore`（规则列表）、`trusted_registries`。
    - `bandit_scan`：使用 [Bandit](https://bandit.readthedocs.io/) 对 Python 代码做安全分析。可指定 `paths`、`severity`、`confidence`、`skip`（跳过测试 ID）、`format`。
    - `lizard_complexity`：使用 [lizard](https://github.com/terryyin/lizard) 做多语言代码圈复杂度分析。可指定 `paths`、`threshold`（复杂度阈值）、`language`、`sort`、`warnings_only`、`exclude`。

## Rust 开发工具示例

以下示例展示了常用 Rust 开发工具的结构化参数（Function Calling 参数 JSON）：

- `cargo_run`（运行二进制）：
  ```json
  {"bin":"crabmate","args":["--help"]}
  ```
- `rust_test_one`（运行单个测试）：
  ```json
  {"test_name":"tools::tests::test_build_tools_names","nocapture":true}
  ```
- `cargo_audit`（依赖安全扫描）：
  ```json
  {"deny_warnings":true}
  ```
- `ci_pipeline_local`（本地 CI 关键检查；可选 Python：`run_ruff_check` / `run_pytest` / `run_mypy`）：
  ```json
  {"run_fmt":true,"run_clippy":true,"run_test":true,"run_frontend_lint":true,"run_ruff_check":true,"run_pytest":false,"run_mypy":false,"fail_fast":true,"summary_only":false}
  ```
- `release_ready_check`（发布前一键检查）：
  ```json
  {"run_ci":true,"run_audit":true,"run_deny":true,"require_clean_worktree":true,"fail_fast":true,"summary_only":true}
  ```
- `cargo_nextest`（更快测试执行）：
  ```json
  {"profile":"default","test_filter":"tools::","nocapture":false}
  ```
- `cargo_fmt_check`（代码格式检查）：
  ```json
  {}
  ```
- `cargo_outdated`（依赖过期检查）：
  ```json
  {"workspace":true,"depth":2}
  ```
- `cargo_machete`（**未使用依赖**启发式扫描，需 `cargo install cargo-machete`；与 `cargo_outdated` 的「可升级版本」互补）：
  ```json
  {"with_metadata":false}
  ```
- `cargo_udeps`（**未使用依赖**构建级检查，需 `cargo install cargo-udeps`；通常需 **`nightly: true`** 即 `cargo +nightly udeps`）：
  ```json
  {"nightly":true}
  ```
- `cargo_publish_dry_run`（`cargo publish --dry-run`，**不会**上传 registry）：
  ```json
  {"package":"my-crate","allow_dirty":false,"no_verify":false}
  ```
- `rust_compiler_json`（**rustc/Cargo JSON 诊断**：`cargo check --message-format=json`，解析 `compiler-message` 汇总错误/警告，无需 rust-analyzer）：
  ```json
  {"all_targets":true,"max_diagnostics":80,"message_format":"json"}
  ```
- `rust_analyzer_goto_definition` / `rust_analyzer_find_references` / `rust_analyzer_hover`（本机需 **`rust-analyzer` 在 PATH**；**line/character 为 0-based**，与 LSP 一致）：
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"wait_after_open_ms":500}
  ```
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"include_declaration":true}
  ```
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4}
  ```
- `rust_analyzer_document_symbol`（整文件符号大纲，可选 `max_symbols` 截断）：
  ```json
  {"path":"src/lib.rs","max_symbols":200}
  ```
- `cargo_fix`（应用编译器建议修复，受控写入）：
  ```json
  {"confirm":true,"broken_code":false}
  ```
- `cargo_deny`（许可证/安全策略检查）：
  ```json
  {"checks":"advisories licenses bans sources","all_features":true}
  ```
- `rust_backtrace_analyze`（分析 panic/backtrace）：
  ```json
  {"backtrace":"thread 'main' panicked at src/main.rs:10:5\nstack backtrace:\n   0: ...","crate_hint":"crabmate"}
  ```
- `frontend_lint`（前端 lint）：
  ```json
  {}
  ```
- `frontend_build`（前端 build）：
  ```json
  {"script":"build"}
  ```
- `frontend_test`（前端 test）：
  ```json
  {"script":"test"}
  ```
- `workflow_execute`（DAG 工作流执行：并行/串行、审批、SLA、失败补偿）：
  - **节点 `max_retries`**（0～5，默认 0）：对 **`timeout`**、**`workflow_tool_join_error`**、**`workflow_semaphore_closed`** 等**可重试**失败自动退避重跑（1s/2s/4s… 上限 8s）；**业务失败**（如测试失败、命令非零退出）**不重试**，避免重复写盘或重复副作用。
  - **静态校验**：每个节点的 **`tool_name`** 须为当前进程内置工具名；**`tool_args`** 须至少包含该工具 JSON Schema 中声明的 **`required`** 键（嵌套对象/数组内对象会递归检查；类型与 `additionalProperties` 等仍以运行时 `runner` 为准）。
  - **结果 JSON**（`workflow_execute_result` / `workflow_validate_result`）含 **`workflow_run_id`**（与日志 `workflow_run_id=` 对齐）、**`trace`**（`dag_start` / `node_attempt_*` / `node_retry_backoff` / `dag_end` 等事件，带时间戳与 `node_id`）、**`completion_order`**（成功节点完成顺序，供补偿逆序对照）；**`nodes[].attempt`** 为最终计入结果的尝试次数。
  ```json
  {"workflow":{
    "max_parallelism":2,
    "fail_fast":true,
    "compensate_on_failure":true,
    "nodes":[
      {"id":"clean","tool_name":"cargo_clean","tool_args":{"dry_run":true},"deps":[],"compensate_with":[]},
      {"id":"clippy","tool_name":"cargo_clippy","tool_args":{"all_targets":true},"deps":["clean"],"compensate_with":["clean"]},
      {"id":"test","tool_name":"cargo_test","tool_args":{},"deps":["clippy"],"compensate_with":[]},
      {"id":"deny","tool_name":"cargo_deny","tool_args":{"checks":"advisories licenses bans sources","all_features":true},"deps":["test"],"requires_approval":true,"compensate_with":["clean"]}
    ]
  }}
  ```
 - `workflow_execute`（Git 工作流示例：注入 `git_log` 的 commit hash 到 `git_show`）：
   ```json
   {"workflow":{
     "max_parallelism":2,
     "fail_fast":true,
     "compensate_on_failure":false,
     "nodes":[
       {"id":"log","tool_name":"git_log","tool_args":{"max_count":1,"oneline":true},"deps":[],"compensate_with":[]},
       {"id":"show","tool_name":"git_show","tool_args":{"rev":"{{log.stdout_first_token}}"},"deps":["log"],"compensate_with":[]}
     ]
   }}
   ```

 - `workflow_execute`（Git 工作流示例：diff 基准检查 + 补丁审批后应用）：
   ```json
   {"workflow":{
     "max_parallelism":1,
     "fail_fast":true,
     "compensate_on_failure":false,
     "nodes":[
       {"id":"diff","tool_name":"git_diff_base","tool_args":{"base":"main","context_lines":3},"deps":[],"compensate_with":[]},
       {"id":"patch_check","tool_name":"git_apply","tool_args":{"patch_path":"patches/fix.diff","check_only":true},"deps":["diff"],"compensate_with":[]},
       {"id":"patch_apply","tool_name":"git_apply","tool_args":{"patch_path":"patches/fix.diff","check_only":false},"deps":["patch_check"],"requires_approval":true,"compensate_with":[]}
     ]
   }}
   ```

   说明：`patch_path` 指向工作区内已有的补丁文件（例如 `patches/fix.diff`），该文件需你或上一步工具提前生成/提供。

  在后续节点参数里可使用上游节点输出占位符（仅会对 `string` 字段生效，JSON 对象/数组会递归处理）：
  - `{{node_id.output}}`：注入节点 `node_id` 的完整输出（默认会截断到最多 `output_inject_max_chars`，默认 `2000` 字符）
  - `{{node_id.status}}`：注入 `passed/failed`
  - `{{node_id.stdout_first_line}}`：注入输出的第一行（同样会截断）
  - `{{node_id.stdout_first_token}}`：注入输出的第一行第一个 token（常用于 `git log --oneline` 的 commit hash）

### 发布前检查推荐模板

- `release_ready_check`（快速版，适合本地频繁自检）：
  ```json
  {"run_ci":true,"run_audit":false,"run_deny":false,"require_clean_worktree":false,"fail_fast":true,"summary_only":true}
  ```

- `release_ready_check`（严格版，适合发版前）：
  ```json
  {"run_ci":true,"run_audit":true,"run_deny":true,"require_clean_worktree":true,"fail_fast":true,"summary_only":false}
  ```
- `cargo_tree`（查看依赖树）：
  ```json
  {"package":"crabmate","depth":2}
  ```
- `cargo_clean`（清理构建产物，默认仅预览）：
  ```json
  {"release":true,"dry_run":true}
  ```
- `cargo_doc`（生成文档）：
  ```json
  {"package":"crabmate","no_deps":true,"open":false}
  ```

另外，已支持的 Rust/前端开发辅助工具还包括：`cargo_check`、`cargo_test`、`cargo_clippy`、`cargo_metadata`、`cargo_machete`、`cargo_udeps`、`cargo_publish_dry_run`、`rust_compiler_json`、`rust_analyzer_goto_definition`、`rust_analyzer_find_references`、`rust_analyzer_hover`、`rust_analyzer_document_symbol`、`read_binary_meta`、`frontend_lint`、`find_references`、`rust_file_outline`、`format_check_file`、`quality_workspace`、`markdown_check_links`、`structured_validate`、`structured_query`、`structured_diff`、`structured_patch`、`table_text`、`text_diff`、`ast_grep_rewrite`、`diagnostic_summary`、`error_output_playbook`、`package_query`。
以及：`cargo_tree`、`cargo_clean`、`cargo_doc`。

**Python / uv / pre-commit**：`ruff_check`、`pytest_run`、`mypy_check`、`python_install_editable`、`uv_sync`、`uv_run`、`pre_commit_run`；聚合类还有 `run_lints`（可选 ruff）、`quality_workspace`（可选 ruff/pytest/mypy）。

**Node.js / npm**：`npm_install`（含 `npm ci`）、`npm_run`（任意 npm script）、`npx_run`（npx 执行包命令）、`tsc_check`（TypeScript 类型检查）。

**Go 工具链**：`go_build`、`go_test`、`go_vet`、`go_mod_tidy`、`go_fmt_check`、`golangci_lint`。

**进程与端口管理**：`port_check`（端口占用检查）、`process_list`（进程列表查询）。

**Git 写操作**：`git_checkout`（切换/创建分支）、`git_branch_create`/`git_branch_delete`、`git_push`、`git_merge`、`git_rebase`（含 abort/continue）、`git_stash`（push/pop/apply/list/drop/clear）、`git_tag`（list/create/delete）、`git_reset`（soft/mixed/hard）、`git_cherry_pick`、`git_revert`。

**代码度量与分析**：`code_stats`（代码行数统计）、`dependency_graph`（依赖关系图）、`coverage_report`（覆盖率报告解析）。

**源码分析**：`shellcheck_check`（Shell 脚本静态分析）、`cppcheck_analyze`（C/C++ 静态分析）、`semgrep_scan`（多语言 SAST 安全扫描）、`hadolint_check`（Dockerfile lint）、`bandit_scan`（Python 安全分析）、`lizard_complexity`（代码圈复杂度分析）。

### Python 与 pre-commit 工具示例

- `ruff_check`：
  ```json
  {}
  ```
- `pytest_run`：
  ```json
  {"test_path":"tests","quiet":true}
  ```
- `uv_sync`：
  ```json
  {"frozen":false,"no_dev":false}
  ```
- `uv_run`（`args` 逐项为子进程参数，不经 shell）：
  ```json
  {"args":["pytest","-q"]}
  ```
- `pre_commit_run`（默认检查**暂存**文件；可与 `all_files` / `files` 组合，见工具说明）：
  ```json
  {"all_files":true}
  ```

## Git 工具示例

以下示例展示新增 Git 工具的常见调用参数（Function Calling 参数 JSON）：

- `git_clean_check`（检查当前工作区是否干净）：
  ```json
  {}
  ```
- `git_diff_stat`（diff 统计）：
  ```json
  {"mode":"working"}
  ```
- `git_diff_names`（diff 变更文件名列表）：
  ```json
  {"mode":"working"}
  ```
- `git_fetch`（拉取远程更新）：
  ```json
  {"remote":"origin","branch":"main","prune":true}
  ```
- `git_remote_list`（查看远程仓库）：
  ```json
  {}
  ```
- `git_remote_set_url`（设置远程 URL，需确认）：
  ```json
  {"name":"origin","url":"git@github.com:your-org/your-repo.git","confirm":true}
  ```
- `git_apply`（先检查补丁可用性）：
  ```json
  {"patch_path":"patches/fix.diff","check_only":true}
  ```
- `git_clone`（克隆到工作区内目录，需确认）：
  ```json
  {"repo_url":"https://github.com/rust-lang/cargo.git","target_dir":"vendor/cargo","depth":1,"confirm":true}
  ```

## 常见失败处理指引

下面是使用 `release_ready_check`、`cargo_deny`、`cargo_audit` 时最常见的失败场景与处理建议。

- `cargo_deny` 失败（许可证/策略不满足）
  - 先单独执行并看完整输出：
    ```bash
    cargo deny check advisories licenses bans sources
    ```
  - 常见原因：
    - 新依赖命中了 `bans`（被禁止包或重复版本过多）
    - 许可证不在允许列表（`licenses`）
    - 来源不符合策略（`sources`）
  - 建议处理：
    - 优先升级或替换触发规则的依赖
    - 在项目 `deny.toml` 中按团队策略补充白名单/例外（需代码评审）
    - 对临时例外设置说明与到期计划，避免长期“豁免”

- `cargo_audit` 失败（存在已知漏洞）
  - 先单独执行并看完整输出：
    ```bash
    cargo audit
    ```
  - 常见原因：
    - 依赖树中存在 RustSec 漏洞公告
    - 锁文件过旧，仍引用已修复前版本
  - 建议处理：
    - 优先执行 `cargo update`（或定向 `cargo update -p <crate>`）后复测
    - 若上游尚未修复，评估降级功能、替代库或临时隔离风险
    - 确认修复后重新运行 `cargo audit` 与测试

- 工作区不干净（`require_clean_worktree=true`）
  - 现象：`release_ready_check` 中 `git_clean_check` 显示 failed。
  - 先确认改动：
    ```bash
    git status
    git diff
    ```
  - 建议处理：
    - 需要保留改动：先提交（或拆分提交）再执行发布检查
    - 暂不发布这些改动：可 stash 后再跑检查
    - 仅想本地快速自检：将 `require_clean_worktree` 设为 `false`

- 工具未安装导致失败
  - `cargo-deny` 未安装：
    ```bash
    cargo install cargo-deny
    ```
  - `cargo-audit` 未安装：
    ```bash
    cargo install cargo-audit
    ```

### 发布前建议顺序

建议按“先快后严”的顺序执行，能更高效定位问题：

1. 本地快速自检（快速版）  
   目标：先确认主流程基本可用，快速发现明显问题。
2. 修复后运行严格检查（严格版）  
   目标：补齐安全与策略校验，确保发版质量门禁。
3. 确认工作区干净并复跑关键测试  
   目标：避免把临时改动带入发布物。
4. 打 tag / 进入发布流程  
   目标：在可追溯状态下产出正式版本。


## 文件/目录辅助工具示例

- `read_binary_meta`（二进制/大文件：只返回大小、修改时间、**文件头 SHA256**，不把整文件读进上下文）：
  ```json
  {"path":"assets/app.bin","prefix_hash_bytes":8192}
  ```
  `prefix_hash_bytes` 为 `0` 时跳过哈希；最大 262144。
- `read_file`（**按行流式**读取，默认单次最多 500 行，避免大文件撑爆上下文）：
  ```json
  {"path":"src/main.rs","start_line":1,"max_lines":200}
  ```
  非 UTF-8 或需去 BOM 时可设 **`encoding`**，例如 `{"path":"legacy.txt","encoding":"gb18030"}`、`{"path":"utf8bom.txt","encoding":"utf-8-sig"}`、`{"path":"unknown.bin","encoding":"auto"}`。默认 **`utf-8` 严格**：非法 UTF-8 字节会返回明确错误，而**不会**用替换字符静默乱码。响应中会提示「下一段可将 start_line 设为 N」。需要精确总行数时可设 `"count_total_lines": true`（大文件会多扫一遍）。也可用 `start_line` + `end_line` 精确区间（仍受 `max_lines` 上限截断）。
- `modify_file`（大文件局部改：`mode=replace_lines` + 行号区间 + `content`，流式改写不落整文件到内存）：
  ```json
  {"path":"src/huge.rs","mode":"replace_lines","start_line":120,"end_line":135,"content":"// 新片段\n"}
  ```
- `read_dir`（列出目录内容）：
  ```json
  {"path":"src","max_entries":50,"include_hidden":false}
  ```
- `file_exists`（检查文件/目录是否存在）：
  ```json
  {"path":"src/main.rs","kind":"file"}
  ```
- `extract_in_file`（文件内按正则抽取匹配行；可选 **`encoding`**，与 `read_file` 相同）：
  ```json
  {"path":"src/main.rs","pattern":"workflow_execute","max_matches":20,"case_insensitive":true}
  ```
  若你只处理 Rust，可使用函数块模式（从匹配到的 `fn` 签名开始，抓取花括号 `{}` 配对的完整块）：
  ```json
  {"path":"src/main.rs","pattern":"pub\\s+fn\\s+run_agent_turn","mode":"rust_fn_block","max_matches":1}
  ```
- `markdown_check_links`（维护 `README.md` / `docs/`：相对链接与 `#fragment` 是否有效；外链仅在对 `allowed_external_prefixes` 命中时才发 HTTP）：
  ```json
  {"roots":["README.md","docs"],"max_files":300,"check_fragments":true,"output_format":"text","allowed_external_prefixes":["https://example.com/docs/"],"external_timeout_secs":10}
  ```
  省略 `roots` 时默认扫描 `README.md` 与 `docs` 目录。
- `structured_validate`（解析 `package.json` / `Cargo.toml` / `.yml` 等是否合法，可选顶层摘要）：
  ```json
  {"path":"frontend/package.json","format":"auto","summarize":true}
  ```
- `structured_query`（取嵌套字段，省上下文）：
  ```json
  {"path":"Cargo.toml","query":"/package/name","format":"toml"}
  ```
  点号路径示例：`{"path":"frontend/package.json","query":"devDependencies.vite"}`。
- `structured_diff`（两份 OpenAPI/配置的结构差异，非行级 diff）：
  ```json
  {"path_a":"specs/openapi.v1.json","path_b":"specs/openapi.v2.json","max_diff_lines":200}
  ```
  **CSV/TSV**：`format` 可用 `csv`/`tsv` 或依赖扩展名；`has_header`（默认 `true`）为 `false` 时表解析为「数组的数组」，便于无表头导出。查询示例：`{"path":"data/sample.csv","query":"/0/name"}`。
- `table_text`（表格行操作，与 `structured_query` 的整表 JSON 路径不同）：例如预览 `{"action":"preview","path":"data/foo.tsv","preview_rows":15}`；按列筛选 `{"action":"filter_rows","path":"data/foo.csv","column":2,"contains":"ERR","max_output_rows":100}`。
- `diagnostic_summary`（脱敏排障：工具链、`target/`、关键 env 是否设置，**不输出任何 env 取值**）：
  ```json
  {"include_toolchain":true,"include_workspace_paths":true,"include_env":true,"extra_env_vars":["CI"]}
  ```
- `apply_patch`（**统一 unified diff**，先 dry-run 再应用；强调 **小步、可回滚、带上下文**）：
  - **格式**：与 `git diff` 相同：`---` / `+++` 文件头、`@@ -旧起始,行数 +新起始,行数 @@`，变更行 `-`/`+`，**上下文行必须以单个空格开头**。
  - **带上下文**：每个 hunk 在修改行上下保留 **至少 2～3 行** 未改动内容，减少错位；避免零上下文单行 hunk。
  - **小步**：一次补丁尽量只做一个逻辑改动（单函数/单配置块），大改动拆多次调用，便于 dry-run 失败定位。
  - **可回滚**：`patch -R`、或 `git checkout -- <file>`、或反向 diff；小步更易安全撤销。
  - **路径与 strip（二选一）**：
    - **推荐**：`--- src/example.rs` / `+++ src/example.rs`（相对工作区根，**无** `a/`），**`strip` 用默认 `0`**。
    - **Git 导出**：`--- a/src/example.rs` / `+++ b/src/example.rs` 时须设 **`"strip": 1`**，否则 patch 会找错路径。
  - 路径须在工作区内，禁止 `..`。
  ```text
  --- src/example.rs
  +++ src/example.rs
  @@ -4,6 +4,7 @@ fn demo() {
       let a = 1;
       let b = 2;
  -    let c = a + b;
  +    let c = a + b + 1;
       println!("{}", c);
       // trailing context
  ```
  ```json
  {"patch":"--- src/example.rs\n+++ src/example.rs\n@@ -4,6 +4,7 @@ fn demo() {\n ...\n"}
  ```
- `find_symbol`（工作区递归定位 Rust 符号位置）：
  ```json
  {"symbol":"run_agent_turn","kind":"fn","path":"src","context_lines":2,"max_results":10}
  ```
- `find_references`（在 `.rs` 中按词边界搜标识符引用，默认跳过疑似定义行）：
  ```json
  {"symbol":"run_tool","path":"src/tools","max_results":50,"case_sensitive":false,"exclude_definitions":true}
  ```
- `rust_file_outline`（单文件结构导航：mod/fn/struct/impl 等行摘要）：
  ```json
  {"path":"src/tools/mod.rs","include_use":false,"max_items":200}
  ```
- `format_check_file`（单文件格式检查，不写盘：`rustfmt --check` / `clang-format --dry-run --Werror` / `prettier --check` / `ruff format --check`）：
  ```json
  {"path":"src/main.rs"}
  ```
- `quality_workspace`（组合质量检查；默认 `fmt --check` + `clippy`，可按开关加 `test` / 前端 lint / prettier check）：
  ```json
  {"run_cargo_fmt_check":true,"run_cargo_clippy":true,"run_cargo_test":false,"run_frontend_lint":false,"run_frontend_prettier_check":false,"fail_fast":true,"summary_only":false}
  ```
