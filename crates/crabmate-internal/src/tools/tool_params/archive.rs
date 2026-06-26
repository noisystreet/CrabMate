//! 归档工具 JSON 参数 schema

use crate::tools::tool_json_schema::tool_parameters_schema_value;
use crate::tools::tool_param_types::{ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs};

pub(in crate::tools) fn params_archive_pack() -> serde_json::Value {
    tool_parameters_schema_value::<ArchivePackArgs>()
}

pub(in crate::tools) fn params_archive_unpack() -> serde_json::Value {
    tool_parameters_schema_value::<ArchiveUnpackArgs>()
}

pub(in crate::tools) fn params_archive_list() -> serde_json::Value {
    tool_parameters_schema_value::<ArchiveListArgs>()
}
