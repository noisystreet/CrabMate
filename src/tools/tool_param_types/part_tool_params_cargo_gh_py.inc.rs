// Cargo/Rust、GitHub CLI、前端/Python 工具参数。

// ── Cargo / Rust ─────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoCommonCliArgs {
    pub release: Option<bool>,
    pub all_targets: Option<bool>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub features: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoTestArgs {
    pub release: Option<bool>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub features: Option<String>,
    pub test_filter: Option<String>,
    pub nocapture: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoRunArgs {
    pub release: Option<bool>,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub features: Option<String>,
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustTestOneArgs {
    pub test_name: String,
    pub package: Option<String>,
    pub bin: Option<String>,
    pub features: Option<String>,
    pub nocapture: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoMetadataArgs {
    pub no_deps: Option<bool>,
    #[schemars(range(min = 1))]
    pub format_version: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoTreeArgs {
    pub package: Option<String>,
    pub invert: Option<String>,
    #[schemars(range(min = 0))]
    pub depth: Option<u32>,
    pub edges: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoCleanArgs {
    pub package: Option<String>,
    pub release: Option<bool>,
    pub doc: Option<bool>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoDocArgs {
    pub package: Option<String>,
    pub no_deps: Option<bool>,
    pub open: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoNextestArgs {
    pub package: Option<String>,
    pub profile: Option<String>,
    pub test_filter: Option<String>,
    pub nocapture: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoOutdatedArgs {
    pub workspace: Option<bool>,
    #[schemars(range(min = 0))]
    pub depth: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoMacheteArgs {
    pub with_metadata: Option<bool>,
    pub path: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoUdepsArgs {
    pub nightly: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoPublishDryRunArgs {
    pub package: Option<String>,
    pub allow_dirty: Option<bool>,
    pub no_verify: Option<bool>,
    pub features: Option<String>,
    pub all_features: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustRustcArgs {
    pub args: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct RustCompilerJsonArgs {
    pub all_targets: Option<bool>,
    pub package: Option<String>,
    pub features: Option<String>,
    pub all_features: Option<bool>,
    #[schemars(range(min = 1, max = 500))]
    pub max_diagnostics: Option<u32>,
    pub message_format: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustAnalyzerPositionArgs {
    pub path: String,
    #[schemars(range(min = 0))]
    pub line: u32,
    #[schemars(range(min = 0))]
    pub character: Option<u32>,
    pub server_path: Option<String>,
    #[schemars(range(min = 0, max = 5000))]
    pub wait_after_open_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustAnalyzerReferencesArgs {
    pub path: String,
    #[schemars(range(min = 0))]
    pub line: u32,
    #[schemars(range(min = 0))]
    pub character: Option<u32>,
    pub include_declaration: Option<bool>,
    pub server_path: Option<String>,
    #[schemars(range(min = 0))]
    pub wait_after_open_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustAnalyzerDocumentSymbolArgs {
    pub path: String,
    #[schemars(range(min = 1, max = 5000))]
    pub max_symbols: Option<u32>,
    pub server_path: Option<String>,
    #[schemars(range(min = 0, max = 5000))]
    pub wait_after_open_ms: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoFixArgs {
    pub confirm: Option<bool>,
    pub broken_code: Option<bool>,
    pub all_targets: Option<bool>,
    pub package: Option<String>,
    pub features: Option<String>,
    pub all_features: Option<bool>,
    pub edition: Option<String>,
    pub edition_idioms: Option<bool>,
    pub allow_dirty: Option<bool>,
    pub allow_staged: Option<bool>,
    pub allow_no_vcs: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoAuditArgs {
    pub deny_warnings: Option<bool>,
    pub json: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct CargoDenyArgs {
    pub checks: Option<String>,
    pub all_features: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustFileOutlineArgs {
    pub path: String,
    pub include_use: Option<bool>,
    #[schemars(range(min = 1, max = 500))]
    pub max_items: Option<u32>,
}

// ── GitHub CLI ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GhPrState {
    Open,
    Closed,
    Merged,
    All,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhPrListArgs {
    pub repo: Option<String>,
    pub state: Option<GhPrState>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhPrViewArgs {
    pub number: u32,
    pub repo: Option<String>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhPrChecksArgs {
    pub repo: Option<String>,
    pub number: Option<u32>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhPrCreateArgs {
    pub title: String,
    pub body: Option<String>,
    pub repo: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub draft: Option<bool>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GhIssueState {
    Open,
    Closed,
    All,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhIssueListArgs {
    pub repo: Option<String>,
    pub state: Option<GhIssueState>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhIssueViewArgs {
    pub number: u32,
    pub repo: Option<String>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhRunListArgs {
    pub repo: Option<String>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhPrDiffArgs {
    pub number: u32,
    pub repo: Option<String>,
    pub patch: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhRunViewArgs {
    pub run_id: String,
    pub repo: Option<String>,
    pub log: Option<bool>,
    pub job: Option<String>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GhReleaseListArgs {
    pub repo: Option<String>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhReleaseViewArgs {
    pub tag: String,
    pub repo: Option<String>,
    pub fields: Option<Vec<String>>,
    pub web: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GhSearchScope {
    Issues,
    Prs,
    Repos,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhSearchArgs {
    pub scope: GhSearchScope,
    pub query: String,
    pub repo: Option<String>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
#[allow(clippy::upper_case_acronyms)] // JSON Schema 与 `gh api` 使用全大写 HTTP 方法名
pub enum GhApiMethod {
    GET,
    HEAD,
    POST,
    PATCH,
    PUT,
    DELETE,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhApiArgs {
    pub path: String,
    pub method: Option<GhApiMethod>,
    pub body: Option<String>,
    pub extra_args: Option<Vec<String>>,
}

// ── Frontend / Python ─────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct FrontendLintArgs {
    pub subdir: Option<String>,
    pub script: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct RuffCheckArgs {
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct PytestRunArgs {
    pub test_path: Option<String>,
    pub keyword: Option<String>,
    pub markers: Option<String>,
    pub quiet: Option<bool>,
    #[schemars(range(min = 1))]
    pub maxfail: Option<u32>,
    pub nocapture: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct MypyCheckArgs {
    pub paths: Option<Vec<String>>,
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PythonInstallBackend {
    Uv,
    Pip,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PythonInstallEditableArgs {
    pub backend: PythonInstallBackend,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct UvSyncArgs {
    pub frozen: Option<bool>,
    pub no_dev: Option<bool>,
    pub all_packages: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UvRunArgs {
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PythonSnippetRunArgs {
    pub code: String,
    pub use_uv: Option<bool>,
    #[schemars(range(min = 1, max = 600))]
    pub timeout_secs: Option<u32>,
}
