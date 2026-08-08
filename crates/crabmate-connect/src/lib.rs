//! 桌面 / 移动 Tauri 共用的「连接远程 `crabmate serve`」逻辑。
//!
//! - 规范化 URL、探测 `GET /health` + 受保护的 `GET /user-data/prefs`
//! - 非空 Bearer 经 URL hash `#cm_web_api_bearer=` 交给前端
//! - 成功连接时将非空 Bearer **覆盖写入**本机系统钥匙串（账户 `tauri_connect_web_api_bearer`）
//!
//! 连接页静态资源见本 crate 的 `assets/connect.html`（由各壳同步进 `dist/`）。

mod allowed_origin;
mod commands;
mod handoff;
mod keyring_bearer;
mod probe;

pub use allowed_origin::AllowedServeOrigin;
pub use commands::{
    SuggestedServerUrl, connect_remote, disconnect_remote, get_connect_bearer,
    get_suggested_server_url, seed_connect_home,
};
pub use handoff::{BEARER_HASH_KEY, build_handoff_url, normalize_base_url};
pub use keyring_bearer::{
    read_connect_bearer, write_connect_bearer_on_connect, write_connect_bearer_unchecked,
};
pub use probe::probe_server;
