//! JVM 工具 JSON Schema。

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{GradleTasksArgs, MavenCompileArgs, MavenTestArgs};

pub(in crate::tools) fn params_maven_compile() -> serde_json::Value {
    tool_parameters_schema_value::<MavenCompileArgs>()
}

pub(in crate::tools) fn params_maven_test() -> serde_json::Value {
    tool_parameters_schema_value::<MavenTestArgs>()
}

pub(in crate::tools) fn params_gradle_compile() -> serde_json::Value {
    tool_parameters_schema_value::<GradleTasksArgs>()
}

pub(in crate::tools) fn params_gradle_test() -> serde_json::Value {
    tool_parameters_schema_value::<GradleTasksArgs>()
}
