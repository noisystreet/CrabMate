//! B3：布局元数据差分指纹（只记行数 / kind 序 / `projection_hash`，不记全文）。

use crate::cm_api_contract::chat::{CONVERSATION_LAYOUT_SCHEMA_VERSION_V2, ConversationLayoutMeta};

/// 脱敏差分快照：可与客户端 hydration 指纹对照（不含正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydrationLayoutFingerprint {
    pub segment_count: usize,
    pub kind_order: String,
    pub projection_hash: Option<String>,
}

/// `layout_schema_version >= 2` 的元数据视为可用确定性投影键。
#[must_use]
pub fn layout_prefers_projection(meta: Option<&ConversationLayoutMeta>) -> bool {
    meta.is_some_and(|m| m.layout_schema_version >= CONVERSATION_LAYOUT_SCHEMA_VERSION_V2)
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
    use super::{fingerprint_layout, layout_prefers_projection};
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
    fn absent_or_v1_layout_is_legacy() {
        assert!(!layout_prefers_projection(None));
        let v1 = ConversationLayoutMeta {
            layout_schema_version: 1,
            projection_hash: None,
            segments: vec![],
        };
        assert!(!layout_prefers_projection(Some(&v1)));
    }

    #[test]
    fn v2_layout_prefers_projection_even_without_segments() {
        let v2 = ConversationLayoutMeta {
            layout_schema_version: 2,
            projection_hash: Some("abc".into()),
            segments: vec![],
        };
        assert!(layout_prefers_projection(Some(&v2)));
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
