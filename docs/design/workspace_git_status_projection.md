# #1457 实施方案：`GET /workspace/git` 只读 git 状态投影（B 方案）

> **状态**：设计定稿，**未实现**（跟踪 #1457）。落地后请回来更新本节。
> 关联：#1457（本仓，路由/字段/错误码权威）、crabmate-client#324（消费方）。
> 关键决策（B 方案）：面板块走 `--porcelain=v2 --branch` **严格真源**；工具卡
> `structured_preview.rs` 的宽松「像不像」判据**一行不改**，两处写明有意分歧。

## 1. 范围与落点

**做**：一条只读路由，返回「是否 git 仓库 / 当前 head / upstream 与 ahead-behind / 脏摘要（分类计数）」，
带服务端时间戳与来源标记，供 Client 侧栏高频轮询。

**不做**（与 issue 非目标一致）：暂存、提交、切分支、fetch、PR、diff / hunk / 还原；
不改 `GET /workspace/changelog` 语义；不在本仓决定 UI 形态；不改 Client 侧 pin。

**新增文件两个**（按 CCN ≤ 10 / fn ≤ 200 行门禁，纯逻辑与 HTTP 薄壳必须分开）：

| 新文件 | 职责 |
| --- | --- |
| `src/web/workspace/git_projection.rs` | 纯逻辑：v2 解析、状态映射、探针与缓存读写、`#[cfg(test)]` 单测 |
| `src/web/workspace/handlers_git.rs` | HTTP 薄壳：取工作区根 → 调 projection → 组装响应（不含解析） |

**改动点**：

| 层 | 文件 | 动作 |
| --- | --- | --- |
| 路由表 | `src/web/routes/workspace/mod.rs` | 加 `.route("/workspace/git", get(workspace_git_handler))` |
| 声明表（OpenAPI 真源） | `src/web/routes/decl/workspace.rs` | 加一条 `RouteDecl`：`protected: true` + `ParamDecl{name:"refresh", Query, required:false, schema:Bool}`；并更新头注释的路径/操作计数 |
| 契约类型 | `src/cm_api_contract/workspace.rs` | 加 `WorkspaceGitStatusResponse` / `WorkspaceGitHead` / `WorkspaceGitCounts` |
| 错误码 | `src/cm_api_contract/error_codes.rs` | 复用 `WORKSPACE_NOT_SET`，新增 4 个 `GIT_PROBE_*` / `WORKSPACE_OUT_OF_ROOTS` |
| OpenAPI components | `src/cm_api_contract/openapi.rs` | `insert_adapted_schema::<WorkspaceGitStatusResponse>` |
| AppState | `src/web/app_state.rs` | 加缓存字段（构造点仅 `runtime_state.rs` / `test_serve.rs` 两处） |

`cm_tools` **零改动**（见 §5）。

不放 `/github/*`：该组 handler 经 `github_gh_context()` 把 `allowed_commands` 与工作目录打包进
`GhCtx`，语义绑死 gh 认证；git 状态与之无关，放进去只会继承「未认证 → 全 default」这一正是本 issue 要修的缺陷。

## 2. JSON 契约

一律 **HTTP 200**，用 `state` 表达可用性（与家族「业务失败仍 200」的习惯一致，客户端只需一套分支）。
**不**沿用 `error: Option<String>` —— 那正是 issue 第 3 条否掉的「error + 全 default，UI 分不清」。

```json
{
  "state": "ok",
  "reason_code": null,
  "message": null,
  "head": { "kind": "branch", "name": "main", "oid": "1a2b3c4d5e6f" },
  "upstream": "origin/main",
  "ahead": 2,
  "behind": 0,
  "dirty": true,
  "counts": { "staged": 1, "unstaged": 2, "untracked": 3, "conflicted": 0, "total": 5 },
  "computed_at_unix": 1735689600,
  "source": "probe"
}
```

### 2.1 `state` 与 `reason_code`（两套命名空间，别混）

`state`（枚举标签，**snake_case**，沿用 `agent/agent_turn/policy/turn_route_decision.rs:29` 的
`#[serde(tag = "outcome", rename_all = "snake_case")]` 风格）：

| `state` | 含义 |
| --- | --- |
| `ok` | 探测成功（`head` / 计数有效） |
| `not_a_repo` | 工作区根不是 git 工作树 |
| `git_unavailable` | 找不到 / 起不来 `git`（未安装、不在 PATH） |
| `failed` | 其余失败，具体看 `reason_code` |

`reason_code`（**SCREAMING_SNAKE**，集中在 `cm_api_contract/error_codes.rs`，与
`WORKSPACE_ALREADY_SET` / `CLONE_NO_POOL` / `JOB_EXPIRED` 同族；**不要**沿用工具面的小写码
`git_not_a_repo` / `command_not_found`，那是 `tool_error` 的命名空间）：

| `reason_code` | 何时 | 来源 |
| --- | --- | --- |
| `null` | `state` 为 `ok` / `not_a_repo` / `git_unavailable` | — |
| `WORKSPACE_NOT_SET` | 工作区根未设置 | **复用既有常量**（`error_codes.rs:18`） |
| `WORKSPACE_OUT_OF_ROOTS` | 根不在 `workspace_allowed_roots` | 新增 |
| `GIT_PROBE_TIMEOUT` | 超出 §4.2 超时 | 新增 |
| `GIT_PROBE_FAILED` | 子命令非零退出 | 新增 |
| `GIT_PROBE_UNPARSABLE` | v2 输出不符合 §3.3 | 新增 |

- `message`：用户可读短句，**不含**绝对路径、不含 stderr 原文（stderr 可能带路径）。
- 工作区未设置 / 越界归 `failed` + 对应 `reason_code`，不另开字段。

### 2.2 `head`：判别标签 + 字段存在性钉死

```json
{ "kind": "branch",   "name": "main", "oid": "1a2b3c4d" }
{ "kind": "detached",                    "oid": "1a2b3c4d" }
{ "kind": "branch",   "name": "main", "oid": null }
```

- `kind` 用 `#[serde(tag = "kind", rename_all = "snake_case")]`；本仓先例
  `agent/context_compaction/golden.rs:59`（同款 `tag = "kind"`）。
- `name`：detached 时**字段缺失**（`skip_serializing_if = "Option::is_none"`，与
  `cm_api_contract/workspace.rs` 全族惯例一致）。禁「空串冒充分支名」。
- `oid`：**字段始终存在**，首提交仓库（`# branch.oid (initial)`）为 `null`。
  这样 Client 只需处理「字段有值 / 字段为 null」，不必再区分「字段缺失」。

### 2.3 `counts` 口径定义

以 `--porcelain=v2` 记录行为准，**按分类计数而非文件数去重**（同一文件可同时 staged 与 unstaged），
且**只统计工作区根子树内的记录**（裁剪规则见 §3.2）：

- `staged` = `XY` 中 `X != '.'` 的记录数
- `unstaged` = `Y != '.'` 的记录数
- `untracked` = `?` 行数
- `conflicted` = `u` 行数
- `total` = **计入**的记录行总数（`1` / `2` / `u` / `?` 各算一行，rename 的 `2` 只算一行）
- `dirty` = `total > 0`（`# ` 头不计）

> **措辞约束**：`dirty` 的语义是「**当前工作区根下**无未提交改动」，**不等于**仓库干净。
> 工作区根是仓库子目录且改动都在根外时，`dirty=false` 而仓库实际是脏的（§3.2 裁剪所致）。
> 文档与 Client 文案都必须按这个口径写，不要写成「仓库干净」。

`upstream` / `ahead` / `behind` 无 upstream 时均为 `null`（不是 `0`）。

### 2.4 时效字段

- `computed_at_unix`：**上次真实探测**完成的服务端 unix 秒；命中缓存时**不推进**。
- `source`：三值单枚举，替代原方案的 `cached` + `stale` 两个布尔
  （两者存在 `stale=true 而 cached=false` 的非法组合，用类型消掉）：

| 值 | 含义 |
| --- | --- |
| `probe` | 本次真探（`computed_at_unix` 即本次） |
| `cache` | TTL 内复用（§4.3） |
| `stale_cache` | 重探失败、旧值兜底（§4.3） |

> **`source` 在未探测时无意义**：工作区根层面的失败（`WORKSPACE_NOT_SET` / `WORKSPACE_OUT_OF_ROOTS`）
> 根本没跑探测，此时 `source` 固定为 `probe`。客户端不要用 `source` 判断「刚探测过」，
> 一律只看 `state`（这也是不加第四个取值的理由：`state=failed` 已经够了）。

## 3. 解析规格（严格，宁失败不猜）

命令固定两条，无参数插值、无 `-C` 透传、不接受任何 rev / path 参数、不加 `--ignored`：

```text
git rev-parse --is-inside-work-tree                 # 仓库判据（true / false，机器值，不受 locale 影响）
git status --porcelain=v2 --branch                  # 唯一数据源（默认 --untracked-files=normal）
```

判据与工具侧 `git::ensure_git_repo_try` **完全同源**（同一命令、同一输出值）——这条已写进代码注释，
防止将来有人给工具侧加检查时静默漂移（见 §5）。

> **实现期更正（路径基准）**：原方案打算用 `git rev-parse --show-prefix` 做前缀裁剪，
> 实测（git 2.x）不需要 —— `git status` 在 cwd 下运行时已把 path 转成**相对 cwd**：
> 根内是 `x` / `./x`，根外是 `../x`，且**仍会列出根外改动**。故裁剪判据改为「丢弃 `../` 开头的记录」，
> `--show-prefix` 从命令中移除（见 §3.2），探测仍是 2 条命令。

### 3.1 记录类型与字段偏移

v2 记录按**空格**分词，而 path 可含空格 ⇒ 必须「固定字段数 + 余下整段」切分，
**不能**整行 `split_whitespace()` 取最后一段：

| 行形 | 固定字段数 | path 位置 | 处理 |
| --- | --- | --- | --- |
| `# branch.oid <oid>` | 3 | — | 记 `base_oid`；`(initial)` ⇒ `head.oid = null` |
| `# branch.head <name>` | 3 | — | `(detached)` ⇒ `detached{oid}`；否则 `branch{name, oid}` |
| `# branch.upstream <ref>` | 3 | — | `upstream = Some(ref)` |
| `# branch.ab +<a> -<b>` | 4 | — | `ahead = a`、`behind = b` |
| `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>` | 9 | 第 9 段起 | 普通变更：`X`→staged、`Y`→unstaged |
| `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>\t<orig>` | 10 | 第 10 段起、首个 `\t` 前 | rename/copy：同 `1` 计数，`total` +1 |
| `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>` | 11 | 第 11 段起 | unmerged：`conflicted` +1 |
| `? <path>` | 2 | 第 2 段起 | untracked +1 |
| 空行 | — | — | 跳过 |

**#1498 起加 `-z`**（原方案不加，那条理由已失效）：投影要出文件清单，而 porcelain v2 默认会用
C 风格引号 + 八进制转义非 ASCII 路径（`"\344\270\255…"`），只出计数时无害、出列表就是乱码。
`-z` 带来三件事，均已实测（git 2.47.3）：

- 记录改为 **NUL 分帧**，`2`（rename/copy）记录的两条路径也由 NUL 分隔（不再是 `\t` 后缀）。
- path **原样输出**（含换行 / 引号的文件名不再被转义），于是含 `\n` 的路径不必再判 UNPARSABLE。
- path 基准变成**仓库根相对**，`status.relativePaths` 随之失效 ⇒ 根内裁剪改由
  `git rev-parse --show-prefix` 剥前缀完成（见 §12.2）。

**不变**：它**不**隐含 `--untracked-files=all`（untracked 目录仍折叠），所以 `counts` 口径一字未改；
参数里仍显式写出 `--untracked-files=normal`，不赌版本行为。

submodule 变更走 `1` / `2` 行，天然覆盖，**不写专用分支**。

### 3.2 边界裁剪（工作区根 ≠ 仓库根时）

`git status` 在 cwd 下运行时**仍会列出工作区根之外的改动**（只是把 path 写成 `../x`），
原样返回就把根外路径泄露给客户端。本投影**裁剪到工作区根**：

- path 以 `../` 开头（或恰为 `..`）⇒ 根外，**不增任何计数**。
- 其余（`x` / `./x` / `./`）⇒ 根内，正常计入。注意 git 会把整个 untracked 目录**折叠成一条**
  `? ./`，所以 untracked 是**条目数**不是文件数。
- `head` / `upstream` / `ahead` / `behind` 是**仓库级**事实，不裁剪（也不含具体路径，无泄露）。

这与工具卡 `git status` 的整仓库输出**有意不同**：工具卡是模型在工作目录里跑命令的原始回显，
本投影是「当前工作区」视角的摘要。两处文档都要写明（§8）。

### 3.3 硬约束

- `# branch.head` 缺失 → `failed` / `GIT_PROBE_UNPARSABLE`（bare 仓库本就会被第一条命令判掉）。
- `# branch.ab` 缺失 = 无 upstream（正常，不是错误）。
- `XY` 长度不足 2 或字符不在 `MADRCU?!.` 内 → `GIT_PROBE_UNPARSABLE`。
- `1` / `2` / `u` 行固定字段数不足 → `GIT_PROBE_UNPARSABLE`。
  **#1498 起**：`-z` 下 path 可含 `\n`，不再据此失败；剥前缀后为空串是**合法**的「工作区根自身」
  记录，归一为 `.`（`kind=dir`）。
- **不以 human-readable 输出兜底**。locale 文案（`位于分支`、`尚未暂存以备提交的变更`）一律不认 ——
  这正是与工具卡判据的分界。

## 4. 执行、超时与缓存

### 4.1 执行

`spawn_blocking` 内顺序跑 §3 的两条命令：第一条非 0 / 首行非 `true` ⇒ 直接 `not_a_repo`，
跳过第二条。`current_dir` 用 handler 已 canonicalize 的工作区根，不用请求里的任何字符串；
前缀值**只用于裁剪**，不参与任何路径拼接。

### 4.2 超时

- 常量 `WORKSPACE_GIT_PROBE_TIMEOUT: Duration = 2s`，`tokio::time::timeout` 包住整段两条命令。
- **不**复用 `command_exec.command_timeout_secs`（默认 600s，那是模型工具的执行上限，语义不同），
  **不**新增 `CM_*` / 配置键（避免 `docs/配置说明.md` 与配置契约扩散）。
- 通用规则：**探测超时 ≤ 缓存 TTL**（见 §10.4）。否则单个慢探测的窗口长于缓存复用窗口，
  同一 TTL 周期内的并发请求会各自发起新探测（fork 放大），侧栏高频轮询下叠成进程风暴。

### 4.3 缓存

挂 `AppStateHttpCore`（与 `llm_models_health_cache` / `tiktoken_baseline_cache` 同一模式）：

```rust
pub(crate) workspace_git_cache: Arc<std::sync::Mutex<Option<CachedWorkspaceGitStatus>>>,

struct CachedWorkspaceGitStatus {
    workspace_root: String,          // canonical 字符串，做 key 校验
    computed_at: Instant,            // 单调钟，用于 TTL
    projection: WorkspaceGitStatusResponse,
}
```

- **一实例一工作区**（#1183）⇒ 单条目即可，不需要 HashMap；切换工作区后 `workspace_root` 不匹配自然失效。
- TTL：`WORKSPACE_GIT_CACHE_TTL = 2s`。命中条件：`workspace_root` 相同 且 `elapsed < TTL`。
- 构造点只有两处（`cm_cli/serve/runtime_state.rs`、`test_serve.rs`），加字段代价可忽略；
  `AppStateHttpCore` 是 `Clone`，字段用 `Arc<Mutex<…>>` 保持廉价克隆。
- 可选查询参数 `refresh=true`（默认 `false`）：**只跳过缓存读、仍回写缓存**（读穿透）。因此该 GET
  保持幂等（不改服务端状态，只可能返回更新的真值），仅供客户端「手动刷新」手势使用。
  命名约定：本仓 query 级缓存穿透统一叫 `refresh`；`/github/oauth/token/refresh` 的 `refresh` 是
  「换令牌」，语义无关，勿混用（见 §10.2）。
- **并发去重（最小 single-flight）**：探测前取 `Arc<tokio::sync::Mutex<()>>` 探针锁，拿到锁后
  **先二次检查缓存**（可能已被前一个请求填好）再探，避免 TTL 到期瞬间的并发请求各自 fork 两条命令。
  本仓**没有**通用 single-flight 设施（`per_turn_flight.rs` 是「单回合状态镜像」，不是去重闸），
  故用锁 + 双重检查就地实现，不抽象公共组件——单消费者时抽象没有收益（见 §10.5）。
- **代价（有意）**：探针锁跨越整个探测，所以一个慢探测会**串行阻塞**其余并发请求（最长 `PROBE_TIMEOUT`）。
  宁可排队也不要 fork 风暴；客户端侧栏是单路轮询，正常不会触发。
- **旧值兜底**：TTL 过期后重探失败（`GIT_PROBE_TIMEOUT` / `GIT_PROBE_FAILED` / `GIT_PROBE_UNPARSABLE`）
  且旧条目 `workspace_root` 仍匹配时，返回旧投影 + `source: "stale_cache"`（`computed_at_unix` 保持旧值），
  而不是把侧栏打成 `failed`。`not_a_repo` / `git_unavailable` 属「明确结论」，直接覆盖旧值，**不**兜底。

## 5. 复用与改动点

| 现状 | 用途 | 处理 |
| --- | --- | --- |
| `git::ensure_git_repo_try`（`pub(super)`） | 仓库判据 | **不改可见性**：本路由跑同一命令（`rev-parse --is-inside-work-tree`）、比同一输出值。判据真源是 **git 命令语义**，不是那段 Rust 代码；注释钉「与 `ensure_git_repo_try` 同源」防漂移 |
| `git::ensure_git_repo`（`pub`） | 同上（`String` 版） | 不用：它把 `ToolError` 压平成 `String`，错误码区分已丢失 |
| `WorkspacePathError` | 工作区根解析失败 | 复用 `user_message()`；错误分派映射到 `WORKSPACE_NOT_SET` / `WORKSPACE_OUT_OF_ROOTS` |
| `structured_preview.rs` porcelain 判据 | 工具卡预览 | **不改**；在文件注释 + `docs/命令行与路由.md` 两处写明有意分歧（AGENTS.md §4.8 写法） |
| `cm_api_contract::error_codes` | 错误码 | 复用 `WORKSPACE_NOT_SET`，新增 4 个 |

**`cm_tools` 零改动**：这是与上一版方案的实质差别——不再为复用而放宽 `ensure_git_repo_try` 的可见性，
PR 少一个 refactor commit、少一处跨模块耦合。代价是 web 层有一段「比较 `rev-parse` 首行是否为 `true`」的
三行逻辑与工具侧同形，用注释锚定同源。

## 6. 安全边界

- **无路径参数**：只查当前会话工作区根，`effective_workspace_base_canonical()` 已含
  `validate_effective_workspace_base`（`workspace_allowed_roots` 校验）。
- 只读：仅 `rev-parse` / `status`；`git status` 可能刷新 `.git/index` 的 stat 缓存，这是 git 固有行为
  （工具侧 `git_status` 同样），文档里说明一句，不视为写操作。
- **不受 `permission_level` 约束**（只读 UI 投影）。
- **不受 `allowed_commands` 约束**（裁定与判据见 §10.1）。家族先例：`/workspace/clone/stream`
  跑 `git clone` 且 URL 来自**客户端**，却**不检查** `allowed_commands`（`clone_stream.rs` 全文件
  无 `command_exec` 引用）。`allowed_commands` 的语义是 `run_command` 的**工具**命令白名单。
- **返回内容裁剪到工作区根子树**（§3.2）：不把工作区根之外的路径回给客户端。
- `message` / 日志只记 `state`、耗时、是否命中缓存，**不落** stderr 原文、不落绝对路径（AGENTS.md §4.5）。

## 7. 测试矩阵

### 7.1 解析单测（纯函数，无 IO，放 `git_projection.rs` 的 `#[cfg(test)]`）

| # | 用例 | 断言 |
| --- | --- | --- |
| 1 | detached | `head.kind == detached`，`oid` 有值，**无** `name` 键 |
| 2 | 无 upstream | `upstream` / `ahead` / `behind` 全 `null` |
| 3 | untracked-only | `dirty=true`，`untracked=1`，其余 0 |
| 4 | staged-only（`1 A. …`） | `staged=1`，`unstaged=0` |
| 5 | 同文件双态（`1 MM …`） | `staged=1` **且** `unstaged=1`，`total=1` |
| 6 | rename（`2 R. … <score> <path>\t<orig>`） | 计 1 条；`path` 含空格不被切错；`\t` 后丢弃 |
| 7 | conflicted（`u UU …`） | `conflicted=1` |
| 8 | 干净仓库 | `dirty=false`，`counts` 全 0 |
| 9 | ahead/behind（`# branch.ab +2 -3`） | `ahead=2`，`behind=3` |
| 10 | `# branch.head` 缺失 / 垃圾行 / 短 `XY` | `Err(GIT_PROBE_UNPARSABLE)`（不 panic、不猜） |
| 11 | 首提交仓库（`# branch.oid (initial)`） | `head.oid == null` 且 **键存在**，其余正常 |
| 12 | 尾部空行 / CRLF | 正常解析（不因 `\r` 判死） |
| 13 | 裁剪：记录 `../outside.txt` | 计数不增、`dirty` 不受影响 |
| 14 | 裁剪：记录 `inside.txt` / `./` | 计入（`./` 是 untracked 目录折叠形态） |
| 15 | path 含 `\n` | `Err(GIT_PROBE_UNPARSABLE)` |
| 16 | `1` / `2` / `u` 行固定字段数不足 | `Err(GIT_PROBE_UNPARSABLE)`，不 panic |
| 17 | path 含空格（`? my file.txt`） | 整段作 path，不被切成两段（与 #6 共用参数化用例） |

### 7.2 状态映射单测（纯函数）

探针结果 → `state` + `reason_code` 逐项断言（本版**不**经 `ToolError`，见 §5）：

- 命令 spawn 失败且 `io::ErrorKind::NotFound` ⇒ `git_unavailable`（`reason_code: null`）
- `rev-parse` 首行非 `true`（含非零退出） ⇒ `not_a_repo`
- 工作区根错误 ⇒ `failed` + `WORKSPACE_NOT_SET` / `WORKSPACE_OUT_OF_ROOTS`
- 超时 ⇒ `failed` + `GIT_PROBE_TIMEOUT`；非零退出 ⇒ `GIT_PROBE_FAILED`；解析失败 ⇒ `GIT_PROBE_UNPARSABLE`

**`git_unavailable` 只能在这一层测**：HTTP 层屏蔽 PATH 要改进程级全局环境，测试并行下不可靠。

### 7.3 缓存单测

命中不推进 `computed_at_unix` 且 `source=="cache"`；TTL 过期后重探（`source=="probe"`）；
`workspace_root` 变化不命中；`refresh=true` 绕过 TTL 但仍回写缓存；
重探超时 → `source=="stale_cache"` 且 `computed_at_unix` 保持旧值；`not_a_repo` **不**兜底（直接覆盖）；
TTL 到期瞬间并发两请求只实际探测一次（探针内计数验证 fork 次数）。
（TTL / 超时做成模块内 `pub(crate) const`，测试用 `#[cfg(test)]` 窄接口注入或直接构造过期条目。）

> **已知缺口**：并发去重（single-flight）没有自动化测试 —— 要验证「只探测一次」得给探针加
> `#[cfg(test)]` 计数注入点。当前靠「探针锁 + 双重检查」的结构保证，属 §10.5「明确不做」的范围。
> 缓存读取层（命中 / 过期 / 兜底 / 换工作区不命中）已有单测覆盖。

### 7.4 HTTP 冒烟（照 `handlers_profile_http_tests.rs` 骨架：`start_test_serve_with_config` + `tempfile`）

`POST /workspace` 设根 → `GET /workspace/git`：

1. `git init` 临时仓库 → `200`、`state=ok`、`head.kind=branch`、`dirty=false`、`source=probe`
2. 写入新文件 → `dirty=true` 且对应计数 +1
3. `git checkout --detach` → `head.kind=detached` 且响应中**无** `name` 键
4. 非 git 临时目录 → `state=not_a_repo`
5. 工作区未设置 → `state=failed` + `reason_code=WORKSPACE_NOT_SET`
6. 连续两次请求 → 第二次 `source=cache`
7. 工作区根设为仓库**子目录**、在根外改文件 → `counts` 不变（§3.2）；HTTP 层一条足矣，
   细粒度前缀剥离由 §7.1 #13/#14 覆盖
8. `?refresh=true` → `source=probe`

测试内起 git 前先 `assert!` git 可用；临时仓库须设 `GIT_AUTHOR_*` / `GIT_COMMITTER_*`
或统一用 `git -c user.name=… -c user.email=…`，避免依赖本机配置。

### 7.5 门禁

- `cargo test -p crabmate --lib workspace` + `cargo test -p crabmate --lib openapi`
- `CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec`（生成快照）
- 声明表 ↔ Router 一致性测试（既有机制自动覆盖）
- `pre-commit run --all-files`

## 8. 文档与契约同步清单

| 文件 | 内容 |
| --- | --- |
| `docs/命令行与路由.md` | workspace 表加一行（`/workspace/profile` 之后）；写明四态 + `reason_code`、计数口径与 `dirty` 措辞约束、`source`、`refresh`、只读，以及**两处**与工具卡的有意分歧（① 宽松判据 vs v2 严格真源 ② 整仓库回显 vs 裁剪到工作区根） |
| `docs/en/CLI.md` | **同一张表的英文版**（该文件 L249 起维护逐行对照的 workspace 路由表）；漏掉就是新一轮中英漂移 |
| `docs/openapi.json` | BLESS 生成，勿手改 |
| `README.md` | AGENTS.md §4.7 要求路由变更同步；README 无逐路由表，在侧栏/工作区能力处补一句即可 |
| `docs/开发文档.md` | 若「主要模块」表涉及 `web/workspace` 职责描述则补一句（新增 `git_projection` 模块） |
| `CHANGELOG.md` | `[Unreleased] → Added`：新增只读路由（纯新增，非破坏） |
| `src/cm_tools/tools/structured_preview.rs` | 文件级注释加一段「面板走 v2 严格真源，本文件宽松判据是有意分歧」 |
| `src/web/workspace/git_projection.rs` | 注释钉「仓库判据与 `git::ensure_git_repo_try` 同源（同命令同输出值）」 |

**`client-contract-v0.4.0`（当前最新 `v0.3.0`；纯新增属 minor）不在功能 PR 内打**：
按 AGENTS.md §4.2，打 tag 属**逐次人工批准**动作。PR 只保证路由与文档就绪，合入后由维护者在发版窗口打 tag，
Client 仓随后升级 pin（不属本仓范围）。

## 9. PR 切分

分支 `feat/1457-workspace-git-status`（PR 正文 `Closes #1457`），两 commit：

1. `feat(web): 新增 GET /workspace/git 只读投影 / add the read-only workspace git projection`
   —— `git_projection.rs`（解析 + 状态映射 + 缓存）+ `handlers_git.rs`（薄壳）+ 契约类型 +
   `error_codes` 新增常量 + AppState 缓存字段（两处构造点）+ 路由与声明表 + OpenAPI BLESS + 测试
2. `docs: 路由表（中英）/ CHANGELOG / 有意分歧说明 / document the route in both languages and the divergences`

（上一版方案的「提权 `ensure_git_repo_try`」commit 已取消：本版 `cm_tools` 零改动，见 §5。）

## 10. 裁定（decisions）

四条开放问题按「全局不变量优先于单点便利」定案。每条给出**通用判据**，后续同类端点不必重新讨论。

### 10.1 `allowed_commands` 不约束本路由

**裁定**：不约束。

**判据**：`allowed_commands` 是 **`run_command` 工具**的命令白名单，属**模型执行面**；
服务端自有的 UI 投影不走它。依据三条：

- `config/tools.toml` 把 `git` / `gh` 列在 `[agent]` 的 run_command 小节，白名单是那张表的语义；
- 家族先例：`/workspace/clone/stream` 跑 `git clone`（URL 来自**客户端**）却**不检查**白名单；
- 反例的代价是**策略越权扩散**：运维为防止模型执行某命令而收紧白名单，会连带静默关掉 UI 显示。
  「为了更安全，可观测性反而消失」是最容易埋雷的一类耦合。

**代价与通用边界**：本路由 fork 的命令参数**全由服务端常量决定**，无任何客户端 / 模型输入进 argv。
将来若某个 UI 投影要把用户输入拼进 argv（`clone` 的 URL 就是），必须自带与该输入形态匹配的校验
（`clone` 的做法：池内 + 目录名校验 + 超时 + 并发上限），**不能**靠借用白名单兜底。

### 10.2 缓存 2s + `refresh=true`

**裁定**：TTL 2s；提供 `refresh` 查询参数（读穿透，仍回写）。

**判据**：只有当缓存是「同一真源的加速副本」（随时可重算）时，才允许提供绕过缓存的读旗标；
若缓存承载**不可重算**的状态（一次性令牌、已消费事件、幂等键）则**不得**提供，否则客户端能靠
`refresh` 反复触发有副作用的路径。本投影随时可从 `git` 重算，属前者。

`computed_at_unix` + `source` 已让客户端自行判断新鲜度，`refresh` 只是省掉「等一个 TTL」；
因此它不改变缓存写入语义，GET 仍幂等。

### 10.3 裁剪到工作区根子树

**裁定**：只统计工作区根内的记录（丢弃 `../` 开头的根外记录）；仓库级事实字段（`head` / `upstream` / `ahead` / `behind`）不裁剪。

**判据**：任何 `/workspace/*` 端点的**返回内容都不得包含工作区根之外的路径信息**——与
`resolve_for_read` / `resolve_for_write` / `workspace_fs` 的边界同源（AGENTS.md §4.6）。
`git status` 是唯一会「天然越界」的数据源（仓库根可能在工作区根之上），必须显式裁剪，
否则侧栏会在无意间把未授权给本实例的目录内容回给客户端。

**顺带收益**：把「工作区根 ≠ 仓库根」这一此前未定义的情形定义清楚了；副作用是 `dirty` 的措辞
必须限定为「当前工作区根下」（§2.3）。

### 10.4 超时保持 2s

**裁定**：2s，不放到 5s。

**判据**：**探测超时 ≤ 缓存 TTL**。否则单个慢探测的窗口长于缓存复用窗口，同一 TTL 周期内的
并发请求会各自发起新探测（fork 放大），侧栏高频轮询下叠成进程风暴。而放宽 TTL 又直接违背
issue「脏摘要随缓存窗口变化」的期望。超时、TTL、轮询频率构成一个约束闭环，2s/2s 是其中最紧的自洽解。
首探慢仓库的体验由 §4.3「旧值兜底」覆盖，不需要靠加长超时解决。

### 10.5 附：明确不做（防后续 PR 顺手扩大范围）

- 不抽象 single-flight 公共组件（单消费者，锁 + 双重检查就地实现，见 §4.3）。
- 不改 `git_status` 工具默认的 `porcelain=false`（那会连带改变工具对外输出形状，属另一件事）。
- 不加 `--ignored`。
- ~~不加文件列表字段~~（原裁定「列表留给将来独立端点」）**已由 #1498 推翻**：改为在
  `GET /workspace/git` 上加 `?files=true`（不新开独立端点，共享同一份缓存），上限固定 500 +
  `truncated` 标志，见 §12。
- 不放宽 `git::ensure_git_repo_try` 的可见性（本版已不需要，见 §5）。

## 11. 对照 issue 验收

| issue 验收项 | 本方案落点 |
| --- | --- |
| 真实分支名 | `head.kind=branch` + `name` |
| detached 可与分支区分 | `head.kind=detached`，无 `name` 键 |
| 脏状态随缓存窗口变化 / 恢复干净 | `counts` + `dirty` + TTL 2s |
| 非 git / git 不可用 / 超时 三态互异 | `state` 四值 + `reason_code`（大写常量） |
| 单测覆盖 detached / 无 upstream / untracked-only / staged-only | §7.1 #1–#5 |
| 缓存失效与时间戳可测 | §7.3 |
| 工作区边界（根外路径不返回） | §3.2 裁剪 + §10.3 判据 |
| 文档同步 | §8（中英双份路由表） |
| 契约 tag | `client-contract-v0.4.0`，合入后由维护者打 |
| `SSE_PROTOCOL_VERSION` 不受影响 | 非流式，不触碰 SSE |

## 12. #1498 增补：文件清单（`?files=true`）

> 本节记录 #1498 相对 §1–§11 的**增补与推翻**；上文未特别标注处，以本节为准。

### 12.1 取数与「形状钉死」

命令固定三条。第一条**不带** `-c`：它要与 `git::ensure_git_repo_try` 保持「同命令同输出值」。

```text
git rev-parse --is-inside-work-tree              # 仓库判据（与工具侧同源）
git rev-parse --show-prefix                      # 工作区根在仓库内的前缀
git -c core.quotePath=false -c status.renames=true -c core.fileMode=true \
    status --porcelain=v2 --branch -z --untracked-files=normal
```

「形状钉死」原则：能改变解析结果的宿主 / 仓库配置，要么显式 `-c` 固定，要么不依赖。
实测（git 2.47.3）的四个漂移源：

| 配置 | 不钉死时的后果 |
| --- | --- |
| `core.quotePath=true`（默认） | 非 ASCII 路径被转成 `"\344\270\255…"`，列表变乱码 |
| `status.renames=false` | `git mv` 拆成 `1 AM` + `1 D.` 两条，`counts.total` 翻倍 |
| `core.fileMode=false` | `chmod +x` 不计改动，`dirty` / `counts` 随宿主变 |
| `status.relativePaths=false` | 旧 `../` 裁剪失效 → **根外路径被当根内返回**（#1498 一并修的缺陷） |

前三者用 `-c` 钉死；最后一个由 §12.2 的前缀裁剪从根上消掉：`-z` 下该配置本身也不再生效。

### 12.2 路径基准与前缀裁剪

加 `-z` 后 porcelain v2 的 path **恒为仓库根相对**：`status.relativePaths` 被忽略，
实测把它设回 `true` 也不会退回 cwd 相对。同时 git **仍会列出工作区根之外的改动**。所以：

- `prefix = git rev-parse --show-prefix`：仓库根自身为空串，否则形如 `ws/inner/`。
- 记录 path **以 prefix 开头** ⇒ 根内，剥掉前缀得到「工作区根相对」路径；否则 ⇒ 根外，丢弃。
- **尾斜杠必须保留**在 prefix 里，否则 `ws/inner2/x` 会被误判成 `ws/inner` 的子路径。
- 剥完为空串 ⇒ 记录就是工作区根自身（整根被折叠），归一为 `.` + `kind=dir`。

这条判据不依赖任何可被宿主改掉的 git 配置，比原先的 `../` 前缀判据稳。

### 12.3 JSON 契约增补

```json
{
  "state": "ok",
  "files": { "items": [], "truncated": false, "limit": 500 },
  "untracked_mode": "folded"
}
```

- `files`：**仅** `?files=true` 且 `state=ok` 时出现；字段缺失 = 本次没要列表 / 没有列表可给。
- 条目 `{ path, kind, staged, unstaged, untracked, conflicted }`：一条 = 一条 porcelain 记录，
  四个布尔与 `counts` 四桶**一一对应**（同一文件 `MM` 时 staged 与 unstaged 同时为真、仍只占一条）。
- `truncated`：为真时 `items` 只是前 `limit` 条，`counts.total` 才是全量，且不承诺哪 N 条跨次稳定。
  内存有界：解析时最多收集 `MAX_LISTED_FILES = 500` 条，计数照常累加。
  未截断时 `items` 按 `path` 字节序升序，完全确定。
- `untracked_mode`：回显取数口径，当前恒 `folded`。将来若加 `all`（`--untracked-files=all`，
  `counts` 从条目数变文件数）属**语义变更**，必须同改契约与文档。

`counts` / `dirty` / `head` 语义**不变**：untracked 目录仍折叠成一条，`counts.untracked` 仍是条目数。

### 12.4 缓存不分叉

`WorkspaceGitCache` 存的是**含全量列表**的响应，handler 在出站时按 `?files` 剥离。
若改成「按 `files` 存两份」，就会出现「先无参请求写缓存 → 再 `?files=true`」拿到没有列表的缓存
（`counts` 说 3、列表空）。HTTP 与缓存单测各有一条回归用例钉住这点。

### 12.5 测试矩阵增补

| 用例 | 位置 |
| --- | --- |
| `status_argv` 逐字钉死参数表 | `status_argv_pins_shape_config` |
| 不变量：未截断时 `items.len() == counts.total` 且四桶求和相等 | `file_list_matches_counts_invariant` |
| 501 条 → 500 条 + `truncated` | `truncation_sets_flag_and_caps_items` |
| 折叠目录 → `kind=dir`；根自身 → `.` | `collapsed_untracked_dir_becomes_dir_entry` / `root_self_record_becomes_dot` |
| 前缀边界：`sub/` 不匹配 `sub2/` | `prefix_boundary_is_a_directory` |
| rename 只一条且吃掉 orig 段 | `rename_record_counts_once_and_swallows_orig_segment` |
| 缓存不被无参请求污染 | `plain_request_must_not_poison_cache_for_files_request` |
| 宿主 `quotePath=true` 下路径保真 | `path_kept_verbatim_under_default_quotepath` |
| `relativePaths=false` 不泄露根外记录（bug 回归） | `relative_paths_false_does_not_leak_outside_records` |
| `renames=false` 下仍只一条 | `renames_pinned_true_keeps_one_record` |
| 先无参再 `?files=true` 仍拿到列表 | `files_served_after_plain_request_hit_cache` |

### 12.6 文档与契约同步

`docs/命令行与路由.md` / `docs/en/CLI.md`（路由表）· `docs/openapi.json`（快照）·
`CHANGELOG` 的 `[Unreleased]`。契约 tag：`client-contract-v0.5.0`（加字段 = minor，
排在 `v0.4.0` 之后，合入后由维护者打，不在功能 PR 内）。
