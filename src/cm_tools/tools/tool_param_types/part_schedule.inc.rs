/// [`super::schedule::add_reminder`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddReminderArgs {
    pub title: String,
    pub due_at: Option<String>,
}

/// [`super::schedule::list_reminders`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListRemindersArgs {
    #[serde(default)]
    pub include_done: bool,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
}

/// [`super::schedule::update_reminder`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateReminderArgs {
    pub id: String,
    pub title: Option<String>,
    pub due_at: Option<String>,
    pub done: Option<bool>,
}

/// 仅含 `id` 的日程/提醒工具入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdOnlyArgs {
    pub id: String,
}

/// [`super::schedule::add_event`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddEventArgs {
    pub title: String,
    pub start_at: String,
    pub end_at: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

/// [`super::schedule::list_events`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListEventsArgs {
    pub year: Option<i32>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 12))]
    pub month: Option<u32>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
}

/// [`super::schedule::update_event`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateEventArgs {
    pub id: String,
    pub title: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

// ── 日程工具（`action` 选择器，2026-10-24 九合一）───────────────
//
// 下面两个是**工具 schema**；上面 `AddReminderArgs` / `UpdateEventArgs` 等旧结构保留为
// 内部 DTO —— `schedule.rs` 的既有实现仍按它们解析，分派层负责把 union 拆回旧形状，
// 这样合并的是对外 schema，而不是重写已验证的读写流程。

/// [`super::schedule_dispatch::reminder`] 的 `action`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReminderAction {
    /// 列出提醒（**只读**）。
    List,
    /// 新增提醒（`title` 必填）。
    Add,
    /// 更新提醒（按 `id`，`title` / `due_at` / `done` 任填）。
    Update,
    /// 删除提醒（按 `id`）。
    Delete,
    /// 标记完成（按 `id`）。
    Complete,
}

/// [`super::schedule_dispatch::reminder`] 入参：`action` 选择器 + 字段并集。
///
/// `list` 只读；`add` / `update` / `delete` / `complete` 写 `.crabmate/reminders.json`。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReminderArgs {
    /// 要执行的提醒操作。
    pub action: ReminderAction,
    /// `add` 必填（非空）；`update` 用于改标题。
    #[serde(default)]
    pub title: Option<String>,
    /// `add` / `update`：到期时间，支持 RFC3339 或 `YYYY-MM-DD HH:MM` / `YYYY-MM-DD`；空串表示清空。
    #[serde(default)]
    pub due_at: Option<String>,
    /// `list`：是否包含已完成（默认 false）。
    #[serde(default)]
    pub include_done: bool,
    /// `list`：仅看未来 N 天（含今天）。
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
    /// `update` / `delete` / `complete`：提醒 id。
    #[serde(default)]
    pub id: Option<String>,
    /// `update`：完成状态。
    #[serde(default)]
    pub done: Option<bool>,
}

/// [`super::schedule_dispatch::event`] 的 `action`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventAction {
    /// 列出日程（**只读**）。
    List,
    /// 新增日程（`title` / `start_at` 必填）。
    Add,
    /// 更新日程（按 `id`，其余任填）。
    Update,
    /// 删除日程（按 `id`）。
    Delete,
}

/// [`super::schedule_dispatch::event`] 入参：`action` 选择器 + 字段并集。
///
/// `list` 只读；`add` / `update` / `delete` 写 `.crabmate/events.json`。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventArgs {
    /// 要执行的日程操作。
    pub action: EventAction,
    /// `add` / `update`：标题（`add` 必填、非空）。
    #[serde(default)]
    pub title: Option<String>,
    /// `add` 必填；`update` 用于改开始时间。
    #[serde(default)]
    pub start_at: Option<String>,
    /// `add` / `update`：结束时间；空串表示清空。
    #[serde(default)]
    pub end_at: Option<String>,
    /// `add` / `update`：地点；空串表示清空。
    #[serde(default)]
    pub location: Option<String>,
    /// `add` / `update`：备注；空串表示清空。
    #[serde(default)]
    pub notes: Option<String>,
    /// `update` / `delete`：日程 id。
    #[serde(default)]
    pub id: Option<String>,
    /// `list`：按年份过滤。
    #[serde(default)]
    pub year: Option<i32>,
    /// `list`：按月份过滤（1–12）。
    #[serde(default)]
    #[schemars(range(min = 1, max = 12))]
    pub month: Option<u32>,
    /// `list`：仅看未来 N 天。
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub future_days: Option<u64>,
}

// ── 归档（`archive`）──────────────────────────────────────────
//
// 整段随 `archive-tools` feature 一起开关：注册表（`specs/archive.inc.rs`）、执行体
// （`tools/archive.rs`，`mod` 本身就带 gate）、写效应名单与 `cm_names` 常量全部按同一
// feature gate。默认构建里这几个类型没有任何构造点，不 gate 就会持续报 `dead_code`。

#[cfg(feature = "archive-tools")]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub enum ArchivePackFormat {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tar.xz")]
    TarXz,
}

/// [`super::archive::archive_pack`] 入参（`exclude` / `format` 与 schema 对齐，当前实现仍按输出扩展名推断格式）。
#[cfg(feature = "archive-tools")]
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchivePackArgs {
    pub output: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub format: Option<ArchivePackFormat>,
}

/// [`super::archive::archive_unpack`] 入参。
#[cfg(feature = "archive-tools")]
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveUnpackArgs {
    pub archive: String,
    #[serde(default = "default_dot_str")]
    pub output_dir: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub strip_components: Option<u32>,
}

#[cfg(feature = "archive-tools")]
fn default_dot_str() -> String {
    ".".to_string()
}

/// [`super::archive::archive_list`] 入参。
#[cfg(feature = "archive-tools")]
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveListArgs {
    pub archive: String,
    #[serde(default)]
    pub verbose: bool,
    /// 最多列出的条目数；超大归档默认 250，避免灌满上下文。
    #[serde(default)]
    #[schemars(range(min = 1, max = 5000))]
    pub max_entries: Option<u32>,
}

// ── 代码导航（`symbol` / `code_nav` / `call_graph_sketch`）──────

/// [`super::symbol::run`]（`find_symbol`）入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindSymbolArgs {
    pub symbol: String,
    pub path: Option<String>,
    pub kind: Option<String>,
    #[serde(default = "default_find_symbol_max_results")]
    #[schemars(range(min = 1, max = 200))]
    pub max_results: Option<u64>,
    #[serde(default = "default_context_lines")]
    #[schemars(range(min = 0))]
    pub context_lines: Option<u64>,
    #[serde(default = "default_true")]
    pub case_insensitive: bool,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_find_symbol_max_results() -> Option<u64> {
    Some(30)
}

fn default_context_lines() -> Option<u64> {
    Some(2)
}

/// [`super::code_nav::find_references`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindReferencesArgs {
    pub symbol: String,
    pub path: Option<String>,
    #[serde(default = "default_find_refs_max_results")]
    #[schemars(range(min = 1, max = 300))]
    pub max_results: Option<u64>,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default = "default_true")]
    pub exclude_definitions: bool,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_find_refs_max_results() -> Option<u64> {
    Some(80)
}

/// [`super::call_graph_sketch::run`] 入参（`symbol` 与 `symbols` 至少其一由 runner 校验）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CallGraphSketchArgs {
    #[serde(default)]
    pub symbols: Vec<String>,
    pub symbol: Option<String>,
    pub path: Option<String>,
    #[serde(default = "default_call_graph_max_edges")]
    #[schemars(range(min = 1, max = 3000))]
    pub max_edges: Option<u64>,
    #[serde(default = "default_call_graph_max_files")]
    #[schemars(range(min = 1, max = 50000))]
    pub max_files: Option<u64>,
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_call_graph_max_edges() -> Option<u64> {
    Some(400)
}

fn default_call_graph_max_files() -> Option<u64> {
    Some(12_000)
}

// ── `code_nav`：代码导航四合一（2026-10-25）──────────────────
//
// 四个只读导航动作共用一份 schema：`action` 选动作，其余字段按 action 被**选择性读取**
// （不相关字段被忽略而非报错，与 `git` / `background_job` 同口径）。per-action 必填
// 在 `code_nav_dispatch` 里校验，因为 union 结构体表达不了「`find_symbol` 必须有 `symbol`、
// `file_outline` 必须有 `path`」这类条件约束。

/// [`super::code_nav_dispatch::dispatch`]（`code_nav`）的动作选择器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeNavAction {
    /// 递归定位 Rust 符号的定义位置（原 `find_symbol`）。
    FindSymbol,
    /// 按词边界搜索标识符引用（原 `find_references`）。
    FindReferences,
    /// 单个 `.rs` 文件的顶层结构大纲（原 `rust_file_outline`）。
    FileOutline,
    /// 启发式变更影响面草图（原 `call_graph_sketch`）。
    CallGraphSketch,
}

impl CodeNavAction {
    /// 序列化后的名字（须与 `#[serde(rename_all = "snake_case")]` 一致）。
    ///
    /// 供分派层的错误文案与工具卡片使用：`match` 在这里被编译器强制穷尽，
    /// 避免手写字符串字面量与 enum 变体悄悄漂移。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FindSymbol => "find_symbol",
            Self::FindReferences => "find_references",
            Self::FileOutline => "file_outline",
            Self::CallGraphSketch => "call_graph_sketch",
        }
    }
}

/// `code_nav` 入参：`action` + 四个动作字段的并集（按 `action` 取用，多余字段被忽略）。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeNavArgs {
    pub action: CodeNavAction,
    /// `find_symbol` / `find_references` / `call_graph_sketch`：目标符号。
    pub symbol: Option<String>,
    /// `call_graph_sketch`：批量符号（与 `symbol` 至少给一个非空）。
    #[serde(default)]
    pub symbols: Vec<String>,
    /// `file_outline` 必填（单个 `.rs` 文件）；其余动作为搜索根目录（缺省工作区根）。
    pub path: Option<String>,
    /// `find_symbol`：定义种类过滤（`fn` / `struct` / `enum` / `trait` 等）。
    pub kind: Option<String>,
    /// `find_symbol`（内部上限 200）/ `find_references`（内部上限 300）。
    /// schema 取两者较宽者，按 action 在实现内部各自 clamp。
    #[schemars(range(min = 1, max = 300))]
    pub max_results: Option<u64>,
    /// `find_symbol`：每处命中前后的上下文行数（默认 2）。
    #[schemars(range(min = 0))]
    pub context_lines: Option<u64>,
    /// `find_symbol`：大小写不敏感（默认 true）。
    pub case_insensitive: Option<bool>,
    /// `find_references`：大小写敏感（默认 false）。
    pub case_sensitive: Option<bool>,
    /// `find_references`：跳过疑似定义行（默认 true）。
    pub exclude_definitions: Option<bool>,
    /// `find_symbol` / `find_references` / `call_graph_sketch`：是否扫描隐藏文件 / 目录
    /// （默认 false；`file_outline` 只读单个文件，不读该字段）。
    pub include_hidden: Option<bool>,
    /// `file_outline`：是否包含 `use` 行（默认 false）。
    pub include_use: Option<bool>,
    /// `file_outline`：最多列出的条目数（默认 200，上限 500）。
    #[schemars(range(min = 1, max = 500))]
    pub max_items: Option<u32>,
    /// `call_graph_sketch`：最多边数（默认 400，上限 3000）。
    #[schemars(range(min = 1, max = 3000))]
    pub max_edges: Option<u64>,
    /// `call_graph_sketch`：最多扫描文件数（默认 12000，上限 50000）。
    #[schemars(range(min = 1, max = 50000))]
    pub max_files: Option<u64>,
}
