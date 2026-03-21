# CrabMate

CrabMate 是一个基于 **DeepSeek API** 从零实现的简易 Rust AI Agent，支持**工具调用**（Function Calling），能在工作区内执行命令、查看/编辑文件并给出自然语言回复。

## 功能概览

- **调用 DeepSeek 对话接口**，支持多模型切换（见下方配置）。
- **内置多种工具，由模型按需调用**：
  - `get_current_time`：获取当前日期时间。
  - `calc`：使用 Linux 的 `bc -l` 执行数学表达式（四则、乘方 ^、sqrt/sin/cos/tan/ln/exp、pi/e 等）。
  - `get_weather`：获取指定城市/地区当前天气（[Open-Meteo](https://open-meteo.com/) API，无需 Key）。
  - `run_command`：执行白名单内的只读/查询类 Linux 命令（`ls`、`pwd`、`whoami`、`date`、`cat`、`head`、`tail`、`wc`、`cmake`、`gcc`、`g++`、`make` 等），带超时与输出截断。
  - `run_executable`：在工作区目录下运行可执行文件（路径、参数均做安全校验）。
  - `create_file` / `modify_file`：创建或修改文件；`read_file` 支持分段与行上限；`modify_file` 支持按行区间替换（大文件友好）。
- **工作区浏览与文件编辑**（Web UI 右侧面板）：
  - 浏览当前工作目录的文件/子目录。
  - 在前端新建/编辑文件，保存后自动刷新工作区列表。
  - Agent 通过工具创建/修改文件后，前端会自动检测并刷新工作区。
- **命令执行与结果展示**：
  - Agent 下发 `run_command` / `run_executable` 时，前端会显示一条“系统消息”摘要（例如 `执行命令：g++ main.cpp -o main`）。
  - 命令执行完成后，命令输出（stdout/stderr、退出码）会以单独的系统气泡展示在聊天框中，便于直接查看 `ls`、编译日志等。
- **流式输出与状态栏**：
  - Chat 回复支持流式增量显示。
  - 状态栏区分“模型生成中…”和“工具运行中…”，命令完成后不会一直显示忙碌。
- **会话保存**：
  - 顶部菜单栏提供“保存会话”按钮，可将当前对话导出为 JSON 文件，便于归档或调试。

## 文档与维护

- **架构与二次开发**：见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)（模块职责、SSE 协议、TUI 目录结构等）。
- **待办清单**：[`docs/TODOLIST.md`](docs/TODOLIST.md) 仅列未完成项；**完成某项后应从该文件删除对应条目**（不要只打勾保留），约定详见 `DEVELOPMENT.md`。
- **新功能**：合并用户可见能力时，请同步更新本 README（功能/命令/配置）与/或 `DEVELOPMENT.md`（架构与协议），便于他人了解与支持。

## 部署与安全提示

- **默认监听 `0.0.0.0`**（`--serve`）：局域网内任意机器可访问 HTTP 接口；**无内置鉴权**，`API_KEY` 仅用于服务端调用模型，**不能**防止他人调用你的 Web API 或滥用配额。
- **工作区**：Web 端可将工作区设为服务器本机任意路径（在进程权限范围内），请勿在不可信网络暴露本服务。
- **建议**：仅本机使用时优先 `127.0.0.1` / 反向代理 + 鉴权；生产或公网需自行加固（TLS、mTLS、防火墙等）。

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
- `ci_pipeline_local`（本地 CI 关键检查）：
  ```json
  {"run_fmt":true,"run_clippy":true,"run_test":true,"run_frontend_lint":true,"fail_fast":true,"summary_only":false}
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
- `cargo_publish_dry_run`（`cargo publish --dry-run`，**不会**上传 registry）：
  ```json
  {"package":"my-crate","allow_dirty":false,"no_verify":false}
  ```
- `rust_compiler_json`（**rustc/Cargo JSON 诊断**：`cargo check --message-format=json`，解析 `compiler-message` 汇总错误/警告，无需 rust-analyzer）：
  ```json
  {"all_targets":true,"max_diagnostics":80,"message_format":"json"}
  ```
- `rust_analyzer_goto_definition` / `rust_analyzer_find_references`（本机需 **`rust-analyzer` 在 PATH**；**line/character 为 0-based**，与 LSP 一致）：
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"wait_after_open_ms":500}
  ```
  ```json
  {"path":"src/tools/mod.rs","line":1195,"character":4,"include_declaration":true}
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

另外，已支持的 Rust/前端开发辅助工具还包括：`cargo_check`、`cargo_test`、`cargo_clippy`、`cargo_metadata`、`cargo_publish_dry_run`、`rust_compiler_json`、`rust_analyzer_goto_definition`、`rust_analyzer_find_references`、`read_binary_meta`、`frontend_lint`、`find_references`、`rust_file_outline`、`format_check_file`、`quality_workspace`。
以及：`cargo_tree`、`cargo_clean`、`cargo_doc`。

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

## 环境

- Rust 1.70+
- 环境变量：`API_KEY`，值为 [DeepSeek 开放平台](https://platform.deepseek.com/) 的 API Key

## 配置与多模型切换

**默认配置**来自项目根目录的 `default_config.toml`（含 `api_base`、`model`）。可在当前工作目录用 `config.toml` 或 `.agent_demo.toml` 覆盖，再被环境变量覆盖（为了兼容早期命名，保留 `.agent_demo.toml` 作为别名）。

1. **环境变量**（优先级最高）  
   - `AGENT_API_BASE`：API 基础 URL  
   - `AGENT_MODEL`：模型 ID  
   - `AGENT_SYSTEM_PROMPT`：系统提示词（内联）  
   - `AGENT_SYSTEM_PROMPT_FILE`：系统提示词文件路径（与上二选一，文件优先）  
   - `AGENT_FINAL_PLAN_REQUIREMENT`：终答是否必须含结构化 `agent_reply_plan`，取值 `never` / `workflow_reflection` / `always`（与 `[agent] final_plan_requirement` 一致，默认 `workflow_reflection`）  
   - `AGENT_PLAN_REWRITE_MAX_ATTEMPTS`：规划不合格时最多重写轮次（默认 `2`，与 `[agent] plan_rewrite_max_attempts` 一致；用尽后 SSE 带 `code=plan_rewrite_exhausted`）  
   - **上下文窗口**（长会话防爆 token，见 `default_config.toml`）：`AGENT_TOOL_MESSAGE_MAX_CHARS`、`AGENT_CONTEXT_CHAR_BUDGET`、`AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM`、`AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS`（`0` 关闭 LLM 摘要）、`AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES`、`AGENT_CONTEXT_SUMMARY_MAX_TOKENS`、`AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS`  
   ```bash
   export AGENT_MODEL=deepseek-reasoner
   cargo run
   ```
2. **配置文件**：`config.toml` 或 `.agent_demo.toml`（可只写要覆盖的项）：
   ```toml
   [agent]
   api_base = "https://api.deepseek.com/v1"
   model = "deepseek-reasoner"
   # 系统提示词：内联或从文件加载
   # system_prompt = "你是专业的助手。"
   # system_prompt_file = "system_prompt.txt"
   ```
   可参考 `config.toml.example`。

**终答规划策略**（`[agent] final_plan_requirement`）：控制模型以**非 tool_calls**结束一轮时，是否必须嵌入可解析的 `agent_reply_plan` JSON（见 `docs/DEVELOPMENT.md`）。`workflow_reflection` 为默认：仅在工作流反思首轮注入「下一步须带规划」指令后启用校验；`never` 关闭该校验；`always` 对每次终答都校验（实验性，易多耗 API 轮次）。若近期存在 `workflow_validate_only` 结果，服务端还会按 `spec.layer_count` 要求规划步骤条数不少于层数。

**规划重写次数**（`[agent] plan_rewrite_max_attempts`）：不合格时追加「请重写」user 消息的上限；超过后结束本轮，流式场景下前端会收到 `error` + `code: plan_rewrite_exhausted`。

**系统提示词**：在 `default_config.toml` 中通过 `system_prompt`（多行字符串）或 `system_prompt_file`（文件路径）配置；若同时设置，以文件内容为准。未配置则启动报错。

**上下文窗口**（`[agent]`）：每次向模型发请求前会压缩 `messages`——`tool_message_max_chars` 截断工具输出；`max_message_history` 限制条数；`context_char_budget > 0` 时按近似字符删最旧消息；`context_summary_trigger_chars > 0` 且总长超阈值时再调一次无 tools 的 API 生成「较早对话摘要」（尾部保留 `context_summary_tail_messages` 条）。TUI/REPL 长会话下裁剪会缩短本地消息列表；Web 单请求内工具多轮仍受益。

**与模型网关的 HTTP 连接**：进程内**一个**共享 `reqwest::Client`（连接池、空闲连接保留、TCP keepalive、`User-Agent`），多次调用 `chat/completions` 时可复用 **TLS/HTTP Keep-Alive**；协议仍是 HTTP（JSON 或 SSE），不是 WebSocket「单条长连接」。细节见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) 中 `http_client`。

常用模型 ID：`deepseek-chat`（默认）、`deepseek-reasoner`（推理链更长，适合复杂推理）。

## 编译与运行（命令行选项）

基础运行方式：

```bash
export API_KEY="your-api-key"
cargo run
```

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
  响应中会提示「下一段可将 start_line 设为 N」。需要精确总行数时可设 `"count_total_lines": true`（大文件会多扫一遍）。也可用 `start_line` + `end_line` 精确区间（仍受 `max_lines` 上限截断）。
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
- `extract_in_file`（文件内按正则抽取匹配行）：
  ```json
  {"path":"src/main.rs","pattern":"workflow_execute","max_matches":20,"case_insensitive":true}
  ```
  若你只处理 Rust，可使用函数块模式（从匹配到的 `fn` 签名开始，抓取花括号 `{}` 配对的完整块）：
  ```json
  {"path":"src/main.rs","pattern":"pub\\s+fn\\s+run_agent_turn","mode":"rust_fn_block","max_matches":1}
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
- `format_check_file`（单文件格式检查，不写盘：`rustfmt --check` / `prettier --check`）：
  ```json
  {"path":"src/main.rs"}
  ```
- `quality_workspace`（组合质量检查；默认 `fmt --check` + `clippy`，可按开关加 `test` / 前端 lint / prettier check）：
  ```json
  {"run_cargo_fmt_check":true,"run_cargo_clippy":true,"run_cargo_test":false,"run_frontend_lint":false,"run_frontend_prettier_check":false,"fail_fast":true,"summary_only":false}
  ```

### 常用命令行选项

CrabMate 支持几种常见运行模式，对应 `src/lib.rs` 中 `run` 的 CLI 解析：

| 选项              | 作用 |
|-------------------|------|
| `-h, --help`      | 显示命令行帮助与示例。|
| `--config <path>` | 显式指定配置文件路径。指定后仅从该文件合并配置，不再查找当前目录下的 `config.toml` / `.agent_demo.toml`。|
| `--serve [port]`  | 以 Web 服务模式启动，默认端口 `8080`。可传入端口号，如 `--serve 3000`。|
| `--query <问题>`  | 单次提问模式：命令行参数中直接给出问题，输出回答后进程退出，适合脚本调用。|
| `--stdin`         | 管道模式：从标准输入读取问题（多行直到 EOF），输出回答后退出，适合 `echo ... | crabmate --stdin` 这种用法。|
| `--workspace <path>` | 启动时指定初始工作区路径（覆盖配置中的 `run_command_working_dir`，仅当前进程生效）。|
| `--output <mode>` | 仅对 `--query` / `--stdin` 生效；`plain` 为默认，`json` 会在末尾额外输出一行 JSON 结果。|
| `--no-tools`      | 禁用所有工具调用，仅作为普通 Chat 使用。|
| `--no-web`        | 仅提供后端 API，不挂载前端静态页面（适合部署为纯后端服务）。|
| `--cli-only`      | 等价于 `--no-web`，便于按习惯书写。|
| `--dry-run`       | 仅检查配置是否可加载、`API_KEY` 是否存在以及前端静态目录是否存在，然后退出，可用于 CI 自检。|
| `--no-stream`     | 对 API 使用 `stream: false`（非 SSE），并在 CLI 下等待完整回答后一次性 Markdown 打印；TUI 侧亦为整块正文刷新。|

对应示例：

```bash
# 使用默认配置交互运行
cargo run

# 使用指定配置文件（覆盖默认 config.toml / .agent_demo.toml 搜索）
cargo run -- --config /path/to/my.toml

# Web 服务模式（默认 8080）
cargo run -- --serve

# Web 服务模式（指定端口）
cargo run -- --serve 3000

# Web 服务模式并指定初始工作区
cargo run -- --serve 8080 --workspace /path/to/project

# 单次提问
cargo run -- --query "北京今天天气怎么样"

# 单次提问并以 JSON 结果形式返回（便于脚本消费）
cargo run -- --output json --query "北京今天天气怎么样"

# 从标准输入读入问题（多行直到 EOF）
echo "1+1等于几" | cargo run -- --stdin

# 禁用所有工具，仅使用模型本身
cargo run -- --no-tools --serve
```

前端在 **`frontend/`** 目录（Vite + React + TypeScript + Tailwind CSS），需先构建后启动后端：

```bash
cd frontend && npm install && npm run build && cd ..
cargo run -- --serve
```

后端从 `frontend/dist` 提供静态页面，API 与页面同源，无需 CORS。

- **GET /**：前端页面（聊天 + 工作区 + 状态栏），在浏览器打开即可对话。
- **POST /chat**：请求体 `{"message": "你的问题"}`，返回 `{"reply": "助手回复"}`（会走完整 Agent 与工具调用）。
- **GET /status**：返回当前模型、API 地址等后台状态。
- **GET /workspace**：返回当前工作目录路径及文件列表。
- **GET /health**：健康检查，返回 `{"status": "ok"}`。

**单次提问（脚本/管道）**：使用 `--query <问题>` 或 `--stdin` 时，程序只执行一次提问并输出回答后退出，便于在脚本或管道中调用：

```bash
# 参数传入问题
cargo run -- --query "北京今天天气怎么样"

# 从标准输入读入问题（多行直到 EOF）
echo "1+1等于几" | cargo run -- --stdin
```

运行后（交互模式）输入问题，例如：

- 「现在几点？」
- 「(123 + 456) * 2 等于多少？」
- 「北京今天天气怎么样？」
- 「今天几号？再帮我算 100 除以 5」

输入 `quit` / `exit` 或按 **Ctrl+D** 退出。

## 打包为 Debian `.deb` 包

本项目已内置 `cargo-deb` 的打包元数据，可在 Debian/Ubuntu 上打成 `.deb` 包后安装运行。

1. **安装 `cargo-deb` 子命令**（只需一次）：

   ```bash
   cargo install cargo-deb
   ```

2. **构建前端静态资源**（用于 Web 界面）：

   ```bash
   cd frontend
   npm install
   npm run build
   cd ..
   ```

3. **编译后端 Release 二进制**：

   ```bash
   cargo build --release
   ```

4. **生成 `.deb` 安装包**：

   ```bash
   cargo deb
   ```

   生成的安装包位于：

   ```bash
   ls target/debian/*.deb
   ```

5. **在系统中安装与卸载**：

   ```bash
   # 安装
   sudo dpkg -i target/debian/crabmate_0.1.0_amd64.deb

   # 如需卸载
   sudo apt remove crabmate
   ```

安装后可直接运行：

```bash
export API_KEY="your-api-key"
crabmate --serve 8080
```

## 项目结构

项目代码结构与各模块机制请移步开发文档：

- `docs/DEVELOPMENT.md`

## 还可完善的方向

可从以下方向继续增强（按需实现）：

| 方向 | 说明 |
|------|------|
| **会话持久化** | 将对话历史保存到文件，下次启动可加载或续聊 |
| **配置外部化** | 通过环境变量或配置文件设置 `max_tokens`、`temperature`、白名单命令等 |
| **更多工具** | 如：读文件（受限路径）、搜索文件内容、当前目录下的 grep 等 |
| **安全** | run_command 可加「允许的工作目录」限制；或通过环境变量扩展白名单 |
| **日志与调试** | 可选记录请求/响应或仅工具调用，便于排查问题 |
| **代码结构** | 拆成多模块（如 `api.rs`、`tools.rs`）并为主流程和工具写单元测试 |

## 参考

- [DeepSeek API - Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek 开放平台](https://platform.deepseek.com/)
