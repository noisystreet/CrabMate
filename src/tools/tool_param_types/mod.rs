//! 内置工具入参的 **serde + schemars** 真源：与 [`super::tool_json_schema`] 生成的 `parameters` 及
//! 各 `runner_*` 反序列化形状一致；分片见 `part_*.inc.rs`（由 `include!` 拼入本模块，以保持 serde `default` 辅助函数作用域一致）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

include!("part_basic.inc.rs");
include!("part_ecosystem.inc.rs");
include!("part_schedule.inc.rs");
include!("part_structured.inc.rs");
