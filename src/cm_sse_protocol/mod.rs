//! CrabMate **`POST /chat/stream`** 控制面 JSON 的**协议版本**常量、SSE 帧层工具函数与运行时。
//!
//! - **`SSE_PROTOCOL_VERSION`**：与 `docs/SSE协议.md` 中的 **`v`** / `sse_capabilities.supported_sse_v` 一致。
//! - **`sse`**：控制面 JSON 协议与编码器；**`runtime` feature**（默认开）才含广播中枢、mpsc 桥与审批桥。
//! - WASM / 外仓分类器：`--no-default-features`（不要 `tokio`）。
//!
//! 与 Cargo semver、发版标签的关系：见 **`docs/design/client_contract_versioning.md`**。
//! 单包 crates.io：见 **`docs/design/crates_io_single_package.md`** S1。

mod ag_ui_classify;
mod control_classify;
pub mod sse;
mod sse_frame;
mod stream_end_reason;

pub use ag_ui_classify::{AgUiParseDispatch, classify_ag_ui_sse_data};
pub use control_classify::{classify_sse_control_outcome, key_present_non_null};
pub use sse_frame::{
    extract_stream_ended_reason, is_sse_done_sentinel, join_sse_data_lines, parse_sse_event_id,
};
pub use stream_end_reason::StreamEndReason;

/// 当前控制面版本：信封顶层 **`v`**，以及首帧 **`sse_capabilities.supported_sse_v`**。
pub const SSE_PROTOCOL_VERSION: u8 = 2;

/// 软能力：`sse_capabilities.terminal_order` 取值——`conversation_saved` 在成功 `RUN_FINISHED` 之前。
///
/// 旧客户端可忽略该字段；**不** bump [`SSE_PROTOCOL_VERSION`]。
pub const SSE_TERMINAL_ORDER_SAVED_BEFORE_FINISHED: &str = "saved_before_finished";

#[cfg(test)]
mod tests {
    use super::SSE_PROTOCOL_VERSION;
    use std::path::PathBuf;

    /// 文档中的「当前版本」须与本常量一致（bump 版本时同步改 `docs/SSE协议.md` / `docs/en/SSE_PROTOCOL.md`）。
    #[test]
    fn sse_protocol_md_lists_current_version() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let zh = root.join("docs/SSE协议.md");
        let en = root.join("docs/en/SSE_PROTOCOL.md");
        let zh_s =
            std::fs::read_to_string(&zh).unwrap_or_else(|e| panic!("read {}: {e}", zh.display()));
        let en_s =
            std::fs::read_to_string(&en).unwrap_or_else(|e| panic!("read {}: {e}", en.display()));
        let needle = format!("**`{SSE_PROTOCOL_VERSION}`**");
        assert!(
            zh_s.contains(&needle),
            "{} must contain current version marker {needle}",
            zh.display()
        );
        assert!(
            en_s.contains(&needle),
            "{} must contain current version marker {needle}",
            en.display()
        );
    }
}
