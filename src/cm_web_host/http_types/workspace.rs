//! `/workspace*` JSON 体。
//! 各请求体使用 `deny_unknown_fields` 拒绝拼写错误的额外键。

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct WorkspacePickResponse {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceResponse {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSetBody {
    pub path: Option<String>,
    /// 项目池模式：按项目名切换（与 `path` 二选一，优先 `project`）。
    pub project: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceProjectsListResponse {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceProjectPostBody {
    pub name: String,
    /// 为 true 且目录不存在时创建；默认 false（仅切换到已存在项目）。
    #[serde(default)]
    pub create: bool,
}

#[derive(Serialize)]
pub struct WorkspaceProjectPostResponse {
    pub ok: bool,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /workspace/clone/stream` 请求体。
#[derive(Deserialize)]
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

#[derive(Deserialize)]
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

#[derive(Serialize)]
pub struct WorkspaceSearchResponse {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/profile`：只读生成的项目画像 Markdown（与首轮注入同源逻辑）。
#[derive(Serialize)]
pub struct WorkspaceProfileResponse {
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `POST /workspace/file/move`：工作区内移动/重命名**文件**（非目录）。
#[derive(Deserialize)]
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

#[derive(Deserialize)]
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
#[derive(Deserialize)]
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

#[derive(Deserialize)]
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

#[derive(Serialize)]
pub struct WorkspaceFileWriteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Default)]
pub struct WorkspaceFileDeleteResponse {
    /// 单路径删除或整批校验失败时的错误信息；批量部分失败时为空（看 `failed`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 批量删除：成功删除的相对路径（单路径删除时为空数组）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<String>,
    /// 批量删除：失败项（相对路径 + 原因）；非空时 `error` 为空。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<WorkspaceFileDeleteFailure>,
}

#[derive(Serialize)]
pub struct WorkspaceFileDeleteFailure {
    pub path: String,
    pub error: String,
}

#[derive(Deserialize)]
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

#[derive(Serialize)]
pub struct WorkspaceDirCreateResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/dir/archive`：打包目录为 zip。空/`None` 表示工作区根。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDirArchiveQuery {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize)]
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

#[derive(Serialize)]
pub struct WorkspaceDirDeleteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceFileReadResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /workspace/changelog`：本会话工作区变更集 Markdown（与 **`session_workspace_changelist`** 注入正文同源）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChangelogQuery {
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceChangelogResponse {
    pub revision: u64,
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
