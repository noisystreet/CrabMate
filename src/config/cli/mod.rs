//! Clap 派生 CLI、`parse_args`、历史 argv 归一化与日志初始化。

pub mod definitions;

mod legacy_argv;
mod logging;
pub(crate) mod parse;

pub use definitions::{
    ChatCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionCli, SaveSessionFormat, ToolReplayCli,
    root_clap_command_for_man_page,
};
pub use legacy_argv::normalize_legacy_argv;
pub use logging::init_logging;
pub use parse::{parse_args, parse_args_from_argv};
