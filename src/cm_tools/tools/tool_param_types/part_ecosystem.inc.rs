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

/// [`super::go_tools::go_fmt_check`] 入参（与 runner 一致：单键 `path`，默认 `.`）。
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct GoFmtCheckArgs {
    pub path: Option<String>,
}

// ── 容器（`container_tools`）──────────────────────────────────

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



