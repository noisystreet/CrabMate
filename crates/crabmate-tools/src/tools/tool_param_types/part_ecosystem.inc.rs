/// [`super::nodejs_tools::npm_install`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct NpmInstallArgs {
    pub subdir: Option<String>,
    #[serde(default)]
    pub ci: bool,
    #[serde(default)]
    pub production: bool,
}

/// [`super::nodejs_tools::npm_run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NpmRunArgs {
    pub script: String,
    pub subdir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// [`super::nodejs_tools::npx_run`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NpxRunArgs {
    pub package: String,
    pub subdir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// [`super::nodejs_tools::tsc_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TscCheckArgs {
    pub subdir: Option<String>,
    pub project: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

// ── Go（`go_tools`，不含 `golangci_lint`）──────────────────────

/// [`super::go_tools::go_build`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoBuildArgs {
    pub package: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub race: bool,
    #[serde(default)]
    pub verbose: bool,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_test`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoTestArgs {
    pub package: Option<String>,
    pub run: Option<String>,
    #[serde(default)]
    pub race: bool,
    #[serde(default = "default_true")]
    pub verbose: bool,
    #[serde(default)]
    pub short: bool,
    pub count: Option<u64>,
    pub timeout: Option<String>,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_vet`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoVetArgs {
    pub package: Option<String>,
    pub tags: Option<String>,
}

/// [`super::go_tools::go_mod_tidy`] 入参（runner 仅消费 `confirm`；`verbose` 与历史 schema 对齐）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoModTidyArgs {
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub confirm: bool,
}

/// [`super::go_tools::go_fmt_check`] 入参（与 runner 一致：单键 `path`，默认 `.`）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoFmtCheckArgs {
    pub path: Option<String>,
}

// ── 容器（`container_tools`）──────────────────────────────────

/// [`super::container_tools::docker_build`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DockerBuildArgs {
    pub context: Option<String>,
    pub tag: Option<String>,
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub no_cache: bool,
}

/// [`super::container_tools::docker_compose_ps`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct DockerComposePsArgs {
    pub project: Option<String>,
    #[serde(default)]
    pub compose_files: Vec<String>,
}

/// [`super::container_tools::podman_images`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct PodmanImagesArgs {
    pub reference: Option<String>,
}

// ── 单文件 path（`format`）────────────────────────────────────

/// [`super::format::run`] / [`super::format::run_check`] 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormatOnePathArgs {
    pub path: String,
}

// ── `lint::run` ───────────────────────────────────────────────

/// [`super::lint::run`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct RunLintsArgs {
    #[serde(default = "default_true")]
    pub run_cargo: bool,
    #[serde(default = "default_true")]
    pub run_cargo_check: bool,
    #[serde(default = "default_true")]
    pub run_frontend: bool,
    #[serde(default)]
    pub run_frontend_build: bool,
    #[serde(default = "default_true")]
    pub run_python_ruff: bool,
}

// ── `quality_tools::quality_workspace` ────────────────────────

/// [`super::quality_tools::quality_workspace`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct QualityWorkspaceArgs {
    #[serde(default = "default_true")]
    pub run_cargo_fmt_check: bool,
    #[serde(default)]
    pub run_cargo_check: bool,
    #[serde(default = "default_true")]
    pub run_cargo_clippy: bool,
    #[serde(default)]
    pub run_cargo_test: bool,
    #[serde(default)]
    pub run_frontend_lint: bool,
    #[serde(default)]
    pub run_frontend_build: bool,
    #[serde(default)]
    pub run_frontend_prettier_check: bool,
    #[serde(default)]
    pub run_ruff_check: bool,
    #[serde(default)]
    pub run_pytest: bool,
    #[serde(default)]
    pub run_mypy: bool,
    #[serde(default)]
    pub run_maven_compile: bool,
    #[serde(default)]
    pub run_maven_test: bool,
    #[serde(default)]
    pub run_gradle_compile: bool,
    #[serde(default)]
    pub run_gradle_test: bool,
    #[serde(default)]
    pub run_docker_compose_ps: bool,
    #[serde(default)]
    pub run_podman_images: bool,
    #[serde(default = "default_true")]
    pub fail_fast: bool,
    #[serde(default)]
    pub summary_only: bool,
}

// ── 源码分析（`source_analysis_tools`）────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckSeverity {
    Error,
    Warning,
    Info,
    Style,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckShellDialect {
    Sh,
    Bash,
    Dash,
    Ksh,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShellcheckOutputFormat {
    Tty,
    Gcc,
    #[serde(rename = "json1")]
    Json1,
    Checkstyle,
    Diff,
    Quiet,
}

/// [`super::source_analysis_tools::shellcheck_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ShellcheckCheckArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub severity: Option<ShellcheckSeverity>,
    pub shell: Option<ShellcheckShellDialect>,
    pub format: Option<ShellcheckOutputFormat>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CppcheckPlatform {
    #[serde(rename = "unix32")]
    Unix32,
    #[serde(rename = "unix64")]
    Unix64,
    #[serde(rename = "win32A")]
    Win32a,
    #[serde(rename = "win32W")]
    Win32w,
    #[serde(rename = "win64")]
    Win64,
    Native,
}

/// [`super::source_analysis_tools::cppcheck_analyze`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CppcheckAnalyzeArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub enable: Option<String>,
    pub std: Option<String>,
    pub platform: Option<CppcheckPlatform>,
}

/// [`super::source_analysis_tools::semgrep_scan`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct SemgrepScanArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub config: Option<String>,
    pub severity: Option<String>,
    pub lang: Option<String>,
    #[serde(default)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HadolintOutputFormat {
    Tty,
    Json,
    Checkstyle,
    Codeclimate,
    #[serde(rename = "gitlab_codeclimate")]
    GitlabCodeclimate,
    Gnu,
    Codacy,
    Sonarqube,
    Sarif,
}

/// [`super::source_analysis_tools::hadolint_check`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct HadolintCheckArgs {
    pub path: Option<String>,
    pub format: Option<HadolintOutputFormat>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub trusted_registries: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditSeverityArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditConfidenceArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BanditOutputFormat {
    Txt,
    Json,
    Csv,
    Xml,
    Html,
    Yaml,
    Screen,
    Custom,
}

/// [`super::source_analysis_tools::bandit_scan`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct BanditScanArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    pub severity: Option<BanditSeverityArg>,
    pub confidence: Option<BanditConfidenceArg>,
    pub skip: Option<String>,
    pub format: Option<BanditOutputFormat>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LizardSortKind {
    CyclomaticComplexity,
    Length,
    TokenCount,
    ParameterCount,
    Nloc,
}

/// [`super::source_analysis_tools::lizard_complexity`] 入参。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct LizardComplexityArgs {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    #[schemars(range(min = 1, max = 200))]
    pub threshold: Option<u32>,
    pub language: Option<String>,
    pub sort: Option<LizardSortKind>,
    #[serde(default)]
    pub warnings_only: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
}

