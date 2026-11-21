//! B3：布局元数据差分指纹（只记行数 / kind 序 / `projection_hash`，不记全文）。
//!
//! E4/S3 起 `layout` 必填，原 `layout_prefers_projection`（`layout_schema_version >= 2` 判定）已删——
//! 读路径不再可能给出 v1 / 缺失的元数据，留一个恒真的守卫只会让后来者以为还有第二条路。

use crate::cm_api_contract::chat::ConversationLayoutMeta;

/// 脱敏差分快照：可与客户端 hydration 指纹对照（不含正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydrationLayoutFingerprint {
    pub segment_count: usize,
    pub kind_order: String,
    pub projection_hash: Option<String>,
}

/// 由会话 `layout` 生成差分指纹。
#[must_use]
pub fn fingerprint_layout(meta: &ConversationLayoutMeta) -> HydrationLayoutFingerprint {
    let kind_order = meta
        .segments
        .iter()
        .map(|s| s.segment_kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    HydrationLayoutFingerprint {
        segment_count: meta.segments.len(),
        kind_order,
        projection_hash: meta.projection_hash.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::fingerprint_layout;
    use crate::cm_api_contract::chat::{ConversationLayoutMeta, ConversationLayoutSegment};

    fn seg(kind: &str, id: &str, seq: u32) -> ConversationLayoutSegment {
        ConversationLayoutSegment {
            turn_id: Some("u0".into()),
            segment_id: id.into(),
            segment_kind: kind.into(),
            before_tool_call_id: None,
            sequence: seq,
        }
    }

    #[test]
    fn fingerprint_records_count_kind_order_and_hash_not_body() {
        let meta = ConversationLayoutMeta {
            layout_schema_version: 2,
            projection_hash: Some("deadbeef".into()),
            segments: vec![
                seg("assistant_commentary", "seg-before-tc1", 0),
                seg("tool", "tc1", 1),
            ],
        };
        let fp = fingerprint_layout(&meta);
        assert_eq!(fp.segment_count, 2);
        assert_eq!(fp.kind_order, "assistant_commentary,tool");
        assert_eq!(fp.projection_hash.as_deref(), Some("deadbeef"));
    }
}
