//! `/workspace*` JSON 体；路由表见 [`crate::web::routes::workspace::router`]。
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileQuery {
    pub path: String,
    /// 可选：`utf-8`（默认）、`utf-8-sig`、`gb18030`、`gbk`、`big5`、`utf-16le`、`utf-16be`、`auto`（与 `read_file` 一致）。
    #[serde(default)]
    pub encoding: Option<String>,
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

#[derive(Serialize)]
pub struct WorkspaceFileDeleteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
