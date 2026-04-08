//! CrabMate **`POST /chat/stream`** 控制面 JSON 的**协议版本**常量。
//!
//! 与 `docs/SSE_PROTOCOL.md` 中的 **`v`** / `sse_capabilities.supported_sse_v` 一致；服务端 `src/sse/protocol.rs` 与 **Leptos** 均依赖本 crate，避免双端常量漂移。

/// 当前控制面版本：信封顶层 **`v`**，以及首帧 **`sse_capabilities.supported_sse_v`**。
pub const SSE_PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::SSE_PROTOCOL_VERSION;
    use std::path::PathBuf;

    /// 文档中的「当前版本」须与本常量一致（bump 版本时同步改 `docs/SSE_PROTOCOL.md` / `docs/en/SSE_PROTOCOL.md`）。
    #[test]
    fn sse_protocol_md_lists_current_version() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let zh = root.join("../../docs/SSE_PROTOCOL.md");
        let en = root.join("../../docs/en/SSE_PROTOCOL.md");
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
