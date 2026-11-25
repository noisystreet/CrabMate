//! `/workspace*` JSON 契约（请求体 + 响应体）。
//!
//! 自 `cm_web_host/http_types/workspace.rs` 整体迁入（issue #939 B.1，protocol 面
//! 公开）；原路径保留 `pub use` 转发，`cm_web_host` / `web` 引用处零改动。
//! 全部类型 `Serialize` + `Deserialize` 双向（Client 构造请求体、解析响应体共用）；
//! 各请求体使用 `deny_unknown_fields` 拒绝拼写错误的额外键。

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WorkspacePickResponse {
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceResponse {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSetBody {
    pub path: Option<String>,
    /// 项目池模式：按项目名切换（与 `path` 二选一，优先 `project`）。
    pub project: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceProjectsListResponse {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceProjectPostBody {
    pub name: String,
    /// 为 true 且目录不存在时创建；默认 false（仅切换到已存在项目）。
    #[serde(default)]
    pub create: bool,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceProjectPostResponse {
    pub ok: bool,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /workspace/clone/stream` 请求体。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCloneStreamBody {
    pub url: String,
    pub name: String,
    /// 可选浅克隆深度（`>=1` 时传 `git clone --depth`）。
    #[serde(default)]
    pub depth: Option<u32>,
    /// 可选分支（非空时 `--branch` + `--single-branch`）。
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSearchBody {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_insensitive: Option<bool>,
    #[serde(default)]
    pub ignore_hidden: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceSearchResponse {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/profile`：只读生成的项目画像 Markdown（供 Web 侧栏按需拉取；不进首轮注入）。
#[derive(Serialize, Deserialize)]
pub struct WorkspaceProfileResponse {
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /workspace/file/move`：工作区内移动/重命名**文件**（非目录）。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileMoveBody {
    pub from: String,
    pub to: String,
    /// 目标已存在为文件时须为 true 才覆盖（默认 false）。
    #[serde(default)]
    pub overwrite: bool,
    /// 可选；与 `GET /workspace/changelog` 相同作用域，供写入会话变更集。
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileQuery {
    /// 目标相对路径（读/下载/单路径删除用；批量删除时可省略）。
    #[serde(default)]
    pub path: String,
    /// 批量删除：逗号分隔的相对路径列表（与 `path` 二选一，优先）；任一非法则整批拒绝、不产生部分删除。含逗号的文件名请用单路径 `path` 删除。
    #[serde(default)]
    pub paths: String,
    /// 可选：`utf-8`（默认）、`utf-8-sig`、`gb18030`、`gbk`、`big5`、`utf-16le`、`utf-16be`、`auto`（与 `read_file` 一致）。
    #[serde(default)]
    pub encoding: Option<String>,
}

/// `PUT /workspace/file/raw` 查询参数（正文为原始字节，非 JSON）。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileRawPutQuery {
    pub path: String,
    /// 仅创建：若文件已存在则 **409** `WORKSPACE_FILE_EXISTS`
    #[serde(default)]
    pub create_only: bool,
    /// 仅修改：若文件不存在则 **404** `WORKSPACE_FILE_MISSING`
    #[serde(default)]
    pub update_only: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileWriteBody {
    pub path: String,
    pub content: String,
    /// 仅创建：若文件已存在则报错
    #[serde(default)]
    pub create_only: bool,
    /// 仅修改：若文件不存在则报错
    #[serde(default)]
    pub update_only: bool,
    /// 为 true 时在 `path` 创建目录（忽略 `content`；与 `POST /workspace/dir` 等价）。
    #[serde(default)]
    pub create_directory: bool,
    /// `create_directory` 为 true 时：为 true 则递归创建父目录（`mkdir -p`）。
    #[serde(default)]
    pub parents: bool,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceFileWriteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct WorkspaceFileDeleteResponse {
    /// 单路径删除或整批校验失败时的错误信息；批量部分失败时为空（看 `failed`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 批量删除：成功删除的相对路径（单路径删除时为空数组）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<String>,
    /// 批量删除：失败项（相对路径 + 原因）；非空时 `error` 为空。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<WorkspaceFileDeleteFailure>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceFileDeleteFailure {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDirCreateBody {
    pub path: String,
    /// 为 true 时等价 `create_dir_all`（中间缺失的父目录一并创建）。
    #[serde(default)]
    pub parents: bool,
    /// 为 true 时删除目录（须 `confirm=true`；非空目录须 `recursive=true`；与 `DELETE /workspace/dir` 等价）。
    #[serde(default)]
    pub delete: bool,
    /// 删除时须为 true（与 `DELETE` 查询参数一致）。
    #[serde(default)]
    pub confirm: bool,
    /// 删除非空目录时须为 true。
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceDirCreateResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/dir/archive`：打包目录为 zip。空/`None` 表示工作区根。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDirArchiveQuery {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDirDeleteQuery {
    pub path: String,
    /// 必须为 true 才会执行删除（与工具 `delete_dir` 一致）。
    #[serde(default)]
    pub confirm: bool,
    /// 为 true 时递归删除非空目录。
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceDirDeleteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceFileReadResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/changelog`：本会话工作区变更集 Markdown（与 **`session_workspace_changelist`** 注入正文同源）。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChangelogQuery {
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceChangelogResponse {
    pub revision: u64,
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/git` 查询参数：`refresh=true` 只跳过缓存读（仍回写缓存）。
///
/// `files=true` 才返回文件清单：侧栏高频轮询用不到列表，默认不在响应体里搬它。
/// 两条参数互不影响（`refresh` 管时效，`files` 管体积）。
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGitQuery {
    #[serde(default)]
    pub refresh: Option<bool>,
    #[serde(default)]
    pub files: Option<bool>,
}

/// `GET /workspace/git` 的可用性状态。
///
/// 四值互不相同 —— 非 git 工作区 / git 不可用 / 读取失败必须能被 UI 分别展示，
/// 不能退化成同一个「空值 + error」形态（否则客户端分不清该写哪句文案）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceGitState {
    /// 探测成功：`head` / `counts` 有效。
    Ok,
    /// 工作区根不是 git 工作树。
    NotARepo,
    /// 找不到 / 起不来 `git`（未安装或不在 `PATH`）。
    GitUnavailable,
    /// 其余失败，具体看 `reason_code`。
    Failed,
}

/// `head` 的判别标签。
///
/// detached 时**没有** `name` 字段：用「字段缺失」而不是空串来区分，避免客户端把
/// 空分支名当成合法值渲染。`oid` 两种情形都存在（首提交仓库为 `null`），
/// 让客户端只需处理「有值 / null」，不必再区分「字段缺失」。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceGitHead {
    Branch { name: String, oid: Option<String> },
    Detached { oid: Option<String> },
}

/// 脏摘要：**分类计数**而非文件数去重（同一文件可同时 staged 与 unstaged）。
///
/// 口径：只统计工作区根子树内的记录（仓库根可能在工作区根之上，见 `git_projection` 的裁剪规则）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceGitCounts {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
    /// 计入的记录行总数（rename 的 `2` 行只算一行）。
    pub total: u32,
}

/// 清单条目的节点类型。
///
/// git 会把整个未跟踪目录折叠成**一条**带尾斜杠的记录（`? newdir/`、`? ./`），
/// 与普通文件记录混在一张列表里；不给类型判别，客户端只能靠尾斜杠猜，
/// 而 `path` 已按契约去掉尾斜杠（见 [`WorkspaceGitFileEntry::path`]）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceGitFileKind {
    File,
    /// git 折叠的未跟踪目录（**不是**目录下每个文件各一条）。
    Dir,
}

/// 未跟踪口径（本条投影的取数模式）。
///
/// 当前服务端恒为 [`WorkspaceGitUntrackedMode::Folded`]（= `--untracked-files=normal`，
/// 与 `counts.untracked` 同口径）。回显它是为了让客户端明确「这轮数字是按哪种口径数的」，
/// 将来若新增 `all`（目录展开成文件，`counts` 语义会变）也不会被误当成同一种。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceGitUntrackedMode {
    /// 未跟踪目录折叠成一条 `kind=dir` 记录。
    #[default]
    Folded,
}

/// 单条未提交改动 = **一条** `--porcelain=v2` 记录。
///
/// 四个布尔与 [`WorkspaceGitCounts`] 的四个桶**一一对应**：同一文件可同时
/// staged 与 unstaged（`XY = MM`），此时两条布尔同时为 `true` 而仍只占一条记录。
/// 未截断时「各桶求和 == `items.len()` == `counts.total`」是一条被单测锁住的不变式。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceGitFileEntry {
    /// 工作区根**相对**路径，**从不**带引号 / 转义（服务端取数时已钉死 `core.quotePath=false` + `-z`）。
    ///
    /// 归一规则（进契约，勿随意改）：`newdir/` → `newdir`（`kind=dir`）、`./` → `.`（`kind=dir`）。
    pub path: String,
    pub kind: WorkspaceGitFileKind,
    /// index（暂存区）相对 HEAD 有改动：`XY` 的 `X != '.'`。
    pub staged: bool,
    /// 工作树相对 index 有改动：`Y != '.'`。
    pub unstaged: bool,
    /// `?` 记录。
    pub untracked: bool,
    /// `u` 记录（未解决的合并冲突）。
    pub conflicted: bool,
}

/// `GET /workspace/git?files=true` 的文件清单。
///
/// **`truncated` 必须与 `limit` 一起读**：为真时 `items` 只是前 `limit` 条，
/// `counts.total` 才是全量；此时不承诺「哪 N 条」跨次稳定。未截断时 `items` 按
/// `path` 字节序升序，完全确定。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceGitFileList {
    pub items: Vec<WorkspaceGitFileEntry>,
    /// 是否因 `limit` 截断（`counts.total > items.len()`）。
    pub truncated: bool,
    /// 本次生效的条数上限，回显给客户端写「仅显示前 N 条」，不必写死数字。
    pub limit: u32,
}

/// 响应值的来源，替代 `cached` + `stale` 两个布尔（后者存在 `stale=true` 而 `cached=false` 的非法组合）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceGitSource {
    /// 本次真探。
    Probe,
    /// TTL 内复用缓存。
    Cache,
    /// 重探失败，沿用旧值兜底。
    StaleCache,
}

/// `GET /workspace/git`：当前工作区的只读 git 状态投影。
///
/// 一律 HTTP 200，用 [`WorkspaceGitState`] 表达可用性；`reason_code` 用
/// `cm_api_contract::error_codes` 的 SCREAMING_SNAKE 常量。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkspaceGitStatusResponse {
    pub state: WorkspaceGitState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<WorkspaceGitHead>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    /// 「当前工作区根下」是否有未提交改动；**不**代表整个仓库干净（见裁剪规则）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<WorkspaceGitCounts>,
    /// 未提交改动的文件清单；**仅** `?files=true` 且 `state=ok` 时出现。
    ///
    /// 字段缺失即「本次没要列表 / 没有列表可给」（不可用态），客户端不要据此推断工作区干净。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<WorkspaceGitFileList>,
    /// 本轮取数用的未跟踪口径（当前恒为 `folded`）。与 `state` 无关，恒存在。
    pub untracked_mode: WorkspaceGitUntrackedMode,
    /// 上次**真实探测**完成的服务端 unix 秒；命中缓存时不推进。
    pub computed_at_unix: u64,
    pub source: WorkspaceGitSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// issue #939 B.1 验收：protocol 面可解析 set/dir/projects 响应样例、
    /// 构造请求体（出站 Serialize），入站请求体保持 `deny_unknown_fields` 严检。
    #[test]
    fn workspace_roundtrips_set_dir_projects_samples() {
        // set 请求体：`path` / `project` 二选一；Client 可出站序列化。
        let set: WorkspaceSetBody =
            serde_json::from_value(serde_json::json!({ "project": "crabmate" }))
                .expect("set body parses");
        assert_eq!(set.project.as_deref(), Some("crabmate"));
        assert!(set.path.is_none());
        let set_out = serde_json::to_value(&set).expect("set body serializes");
        assert_eq!(
            set_out,
            serde_json::json!({ "path": null, "project": "crabmate" })
        );
        // deny_unknown_fields：拼写错误的额外键拒绝。
        assert!(
            serde_json::from_value::<WorkspaceSetBody>(serde_json::json!({
                "path": "/tmp", "proj": "x"
            }))
            .is_err(),
            "unknown key must be rejected"
        );

        // dir 列表响应样例。
        let dir: WorkspaceResponse = serde_json::from_value(serde_json::json!({
            "path": "/tmp/ws",
            "entries": [{ "name": "src", "is_dir": true }],
        }))
        .expect("dir response parses");
        assert_eq!(dir.path, "/tmp/ws");
        assert_eq!(dir.entries.len(), 1);
        assert!(dir.error.is_none());

        // projects 列表响应样例（`projects` / `pool_path` 缺省为空）。
        let projects: WorkspaceProjectsListResponse =
            serde_json::from_value(serde_json::json!({ "enabled": true }))
                .expect("projects response parses");
        assert!(projects.enabled);
        assert!(projects.pool_path.is_none());
        assert!(projects.projects.is_empty());

        // 批量删除响应：`deleted` / `failed` 缺省为空（serde(default)）。
        let deleted: WorkspaceFileDeleteResponse =
            serde_json::from_value(serde_json::json!({})).expect("delete response parses");
        assert!(deleted.deleted.is_empty() && deleted.failed.is_empty());
    }
}
