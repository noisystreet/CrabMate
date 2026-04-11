//! Clap 派生 CLI、`parse_args`、历史 argv 归一化；进程日志初始化见 crate 根 **`observability::init_tracing_subscriber`**。

pub mod definitions;

mod legacy_argv;
pub(crate) mod parse;

pub use definitions::{
    ChatCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionCli, SaveSessionFormat, ToolReplayCli,
    root_clap_command_for_man_page,
};
pub use legacy_argv::normalize_legacy_argv;
pub use parse::{parse_args, parse_args_from_argv};
