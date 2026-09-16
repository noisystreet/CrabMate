//! Clap 派生 CLI、`parse_args`、`help` 子命令 argv 归一化；进程日志初始化见 crate 根 **`observability::init_tracing_subscriber`**。

pub mod definitions;

mod help_argv;
pub(crate) mod parse;

pub use definitions::{
    E2eCliArgs, ExtraCliCommand, ParsedCliArgs, PluginInitCli, PluginListCli, PluginValidateCli,
    SaveSessionCli, SaveSessionFormat, SaveSessionProjection, SseReplayCli, ToolReplayCli,
    WebBearerCli, WorkflowFileCli, root_clap_command_for_man_page,
};
pub use help_argv::normalize_help_argv;
pub use parse::{parse_args, parse_args_from_argv};
