// `exec_package` 与 `file_core` 手写 JSON Schema 迁移。

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSessionAction {
    Exec,
    SendSignal,
    Resize,
    List,
    Close,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TerminalSessionArgs {
    /// exec / send_signal / resize / list / close
    pub action: TerminalSessionAction,
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub input: Option<String>,
    pub signal: Option<i32>,
    #[schemars(range(min = 1))]
    pub cols: Option<u32>,
    #[schemars(range(min = 1))]
    pub rows: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunCommandArgs {
    /// 纯命令名（详见工具说明；禁止嵌入参数）
    pub command: String,
    pub args: Option<Vec<String>>,
}

// ── file_core ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModifyFileMode {
    Full,
    ReplaceLines,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModifyFileArgs {
    pub path: String,
    pub mode: Option<ModifyFileMode>,
    pub content: Option<String>,
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileFromToOverwriteArgs {
    pub from: String,
    pub to: String,
    pub overwrite: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ReadFileArgs {
    pub path: String,
    #[schemars(range(min = 1, max = 8000))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1, max = 8000))]
    pub end_line: Option<u32>,
    #[schemars(range(min = 1, max = 8000))]
    pub max_lines: Option<u32>,
    pub count_total_lines: Option<bool>,
    pub encoding: Option<String>,
    #[schemars(range(min = 1, max = 4000))]
    pub anchor_line: Option<u32>,
    #[schemars(range(min = 1, max = 4000))]
    pub context_lines: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobFilesArgs {
    pub pattern: String,
    pub path: Option<String>,
    #[schemars(range(min = 0, max = 100))]
    pub max_depth: Option<u32>,
    #[schemars(range(min = 1, max = 5000))]
    pub max_results: Option<u32>,
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListTreeArgs {
    pub path: Option<String>,
    #[schemars(range(min = 0, max = 60))]
    pub max_depth: Option<u32>,
    #[schemars(range(min = 1, max = 10000))]
    pub max_entries: Option<u32>,
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileExistsKind {
    File,
    Dir,
    Any,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileExistsArgs {
    pub path: String,
    pub kind: Option<FileExistsKind>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadBinaryMetaArgs {
    pub path: String,
    #[schemars(range(min = 0, max = 262144))]
    pub prefix_hash_bytes: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum HashFileAlgorithm {
    #[serde(rename = "sha256", alias = "sha-256")]
    Sha256,
    #[serde(rename = "sha512", alias = "sha-512")]
    Sha512,
    #[serde(rename = "blake3")]
    Blake3,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HashFileArgs {
    pub path: String,
    pub algorithm: Option<HashFileAlgorithm>,
    /// 仅哈希前若干字节；整文件时省略
    #[schemars(range(min = 1, max = 4294967295u64))]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtractInFileMode {
    Lines,
    RustFnBlock,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ExtractInFileArgs {
    pub path: String,
    pub pattern: String,
    #[schemars(range(min = 1))]
    pub start_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub end_line: Option<u32>,
    #[schemars(range(min = 1))]
    pub max_matches: Option<u32>,
    pub case_insensitive: Option<bool>,
    #[schemars(range(min = 1))]
    pub max_snippet_chars: Option<u32>,
    pub mode: Option<ExtractInFileMode>,
    #[schemars(range(min = 1))]
    pub max_block_chars: Option<u32>,
    #[schemars(range(min = 1))]
    pub max_block_lines: Option<u32>,
    pub encoding: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct ApplyPatchArgs {
    pub patch: String,
    #[schemars(range(min = 0))]
    pub strip: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CodebaseSemanticSearchArgs {
    pub query: Option<String>,
    pub rebuild_index: Option<bool>,
    pub incremental: Option<bool>,
    pub path: Option<String>,
    #[schemars(range(min = 1, max = 64))]
    pub top_k: Option<u32>,
    #[schemars(range(min = 0, max = 2_000_000))]
    pub query_max_chunks: Option<u32>,
    pub file_glob: Option<String>,
    pub extensions: Option<Vec<String>>,
    pub retrieve_mode: Option<String>,
    pub hybrid_alpha: Option<f64>,
    #[schemars(range(min = 1, max = 10_000))]
    pub fts_top_n: Option<u32>,
    #[schemars(range(min = 1, max = 10_000))]
    pub hybrid_semantic_pool: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct SearchInFilesEnhancedArgs {
    pub pattern: String,
    pub path: Option<String>,
    #[schemars(range(min = 1))]
    pub max_results: Option<u32>,
    pub case_insensitive: Option<bool>,
    pub ignore_hidden: Option<bool>,
    #[schemars(range(min = 0, max = 10))]
    pub context_before: Option<u32>,
    #[schemars(range(min = 0, max = 10))]
    pub context_after: Option<u32>,
    pub file_glob: Option<String>,
    pub exclude_glob: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReadDirSortBy {
    Name,
    Size,
    Mtime,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct ReadDirEnhancedArgs {
    pub path: Option<String>,
    #[schemars(range(min = 1))]
    pub max_entries: Option<u32>,
    pub include_hidden: Option<bool>,
    pub include_size: Option<bool>,
    pub include_mtime: Option<bool>,
    pub sort_by: Option<ReadDirSortBy>,
}
