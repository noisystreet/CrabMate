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

// ── JVM（`jvm_tools`）──────────────────────────────────────────

/// [`super::jvm_tools::maven_compile`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct MavenCompileArgs {
    pub profile: Option<String>,
}

/// [`super::jvm_tools::maven_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct MavenTestArgs {
    pub profile: Option<String>,
    pub test: Option<String>,
}

/// [`super::jvm_tools::gradle_compile`] / [`super::jvm_tools::gradle_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GradleTasksArgs {
    #[serde(default)]
    pub tasks: Vec<String>,
}

// ── 归档（`archive`）──────────────────────────────────────────

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

fn default_dot_str() -> String {
    ".".to_string()
}

/// [`super::archive::archive_list`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveListArgs {
    pub archive: String,
    #[serde(default)]
    pub verbose: bool,
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
