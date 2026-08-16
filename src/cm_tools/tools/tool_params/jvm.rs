//! JVM 工具 JSON Schema。

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{GradleTasksArgs, MavenCompileArgs, MavenTestArgs};

pub(in crate::cm_tools::tools) fn params_maven_compile() -> serde_json::Value {
    tool_parameters_schema_value::<MavenCompileArgs>()
}

pub(in crate::cm_tools::tools) fn params_maven_test() -> serde_json::Value {
    tool_parameters_schema_value::<MavenTestArgs>()
}

pub(in crate::cm_tools::tools) fn params_gradle_compile() -> serde_json::Value {
    tool_parameters_schema_value::<GradleTasksArgs>()
}

pub(in crate::cm_tools::tools) fn params_gradle_test() -> serde_json::Value {
    tool_parameters_schema_value::<GradleTasksArgs>()
}
