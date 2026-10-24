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




