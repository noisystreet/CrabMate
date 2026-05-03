//! 容器工具 JSON Schema。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{DockerBuildArgs, DockerComposePsArgs, PodmanImagesArgs};

pub(in crate::tools) fn params_docker_build() -> serde_json::Value {
    tool_parameters_schema_value::<DockerBuildArgs>()
}

pub(in crate::tools) fn params_docker_compose_ps() -> serde_json::Value {
    tool_parameters_schema_value::<DockerComposePsArgs>()
}

pub(in crate::tools) fn params_podman_images() -> serde_json::Value {
    tool_parameters_schema_value::<PodmanImagesArgs>()
}
