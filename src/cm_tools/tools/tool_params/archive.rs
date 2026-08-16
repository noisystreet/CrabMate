//! 归档工具 JSON 参数 schema

use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value;
use crate::cm_tools::tools::tool_param_types::{ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs};

pub(in crate::cm_tools::tools) fn params_archive_pack() -> serde_json::Value {
    tool_parameters_schema_value::<ArchivePackArgs>()
}

pub(in crate::cm_tools::tools) fn params_archive_unpack() -> serde_json::Value {
    tool_parameters_schema_value::<ArchiveUnpackArgs>()
}

pub(in crate::cm_tools::tools) fn params_archive_list() -> serde_json::Value {
    tool_parameters_schema_value::<ArchiveListArgs>()
}
