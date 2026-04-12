//! 界面文案与语言。当前为 **zh-Hans** / **en** 静态表；新文案请在此集中维护，便于后续接 ICU / 远程词条。
//!
//! 按界面域拆分子模块（`settings`、`messages` 等）；对外仍通过 [`crate::i18n`] 扁平导出，调用路径不变。

mod locale_storage;

mod api_errors;
mod approval;
mod assistant_md;
mod changelist;
mod chat_column;
mod clipboard_and_session;
mod export_md;
mod find;
mod messages;
mod session_modal;
mod settings;
mod sidebar;
mod status;
mod stream;
mod tool_cards;
mod workspace_toolbar;
mod workspace_tree;

pub use api_errors::*;
pub use approval::*;
pub use assistant_md::*;
pub use changelist::*;
pub use chat_column::*;
pub use clipboard_and_session::*;
pub use export_md::*;
pub use find::*;
pub use locale_storage::{Locale, load_locale_from_storage, store_locale_slug};
pub use messages::*;
pub use session_modal::*;
pub use settings::*;
pub use sidebar::*;
pub use status::*;
pub use stream::*;
pub use tool_cards::*;
pub use workspace_toolbar::*;
pub use workspace_tree::*;
