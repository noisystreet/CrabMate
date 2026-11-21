//! `layout_meta_json` 列的唯一可用性判定：回填、覆盖率统计与读路径共用这一份口径。
//!
//! 这里曾有三份互不相同的「空」定义：读路径用 Rust `str::trim()`，`layout_coverage`
//! 用 SQL `trim(x, char(9) || char(10) || char(13))`，而回填候选用裸 `trim(x)`
//! （SQLite 单参 `trim` 只去空格）。同一行因此可以被同时判成「legacy」和「已有 layout」，
//! 「库内 legacy 行 = 0」（#1338 S3 删读路径的判据）既成立也不成立。
//!
//! 另外「不可用」不止「空」一类：`layout_meta_json` 非空但 JSON 损坏、或
//! `layout_schema_version < 2` 的行，旧判定既不算 legacy 也不算候选，却同样撑不起
//! 确定性投影。纯删读路径之后它们会直接读不出来，所以判定必须把三类一起覆盖。
//!
//! 判定**不用** SQL 的 JSON 函数：直接 `serde_json` 反序列化，与读路径同源，
//! 也不依赖 SQLite 是否带 JSON1 扩展。

use crate::cm_api_contract::chat::{CONVERSATION_LAYOUT_SCHEMA_VERSION_V2, ConversationLayoutMeta};

/// 一行 `layout_meta_json` 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutColumnState {
    /// `NULL` 或只含空白（Rust `str::trim` 口径）——从未写过布局。
    Missing,
    /// 非空但不是合法 JSON（或缺少 `layout_schema_version` 等必填字段）。
    Corrupt,
    /// 合法 JSON，但 `layout_schema_version < 2`（v1 行 / 手写数据）。
    NotV2 {
        /// 落盘的 `layout_schema_version`。
        version: u32,
    },
    /// `layout_schema_version >= 2` 且可解析——确定性投影可用。
    Usable(ConversationLayoutMeta),
}

impl LayoutColumnState {
    /// 是否可当确定性投影键用。
    #[must_use]
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Usable(_))
    }

    /// 是否需要回填（= 不可用，三类都算）。
    #[must_use]
    pub fn needs_backfill(&self) -> bool {
        !self.is_usable()
    }
}

/// 判定一行；列为 `NULL` 时传空串即可（`NULL` 与空白同义）。
#[must_use]
pub fn classify_layout_column(raw: &str) -> LayoutColumnState {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LayoutColumnState::Missing;
    }
    let Ok(meta) = serde_json::from_str::<ConversationLayoutMeta>(trimmed) else {
        return LayoutColumnState::Corrupt;
    };
    if meta.layout_schema_version >= CONVERSATION_LAYOUT_SCHEMA_VERSION_V2 {
        LayoutColumnState::Usable(meta)
    } else {
        LayoutColumnState::NotV2 {
            version: meta.layout_schema_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayoutColumnState, classify_layout_column};

    fn v2_json() -> String {
        r#"{"layout_schema_version":2,"projection_hash":"deadbeef","segments":[]}"#.into()
    }

    /// `NULL` / 空白一律算「没写过」——包括 SQLite 单参 `trim` 去不掉的 `\t\n\r`。
    #[test]
    fn null_and_whitespace_are_missing() {
        for raw in ["", "  ", "\n", "\t", "\r\n", " \t\n\r "] {
            assert_eq!(
                classify_layout_column(raw),
                LayoutColumnState::Missing,
                "{raw:?}"
            );
        }
    }

    /// 坏 JSON 是 `Corrupt` 而不是 `Missing`：两者都会挡住删读路径，但处置方式不同。
    #[test]
    fn broken_json_is_corrupt_not_missing() {
        assert_eq!(
            classify_layout_column("{not json"),
            LayoutColumnState::Corrupt
        );
        assert_eq!(classify_layout_column("[]"), LayoutColumnState::Corrupt);
    }

    #[test]
    fn v1_is_not_v2() {
        assert_eq!(
            classify_layout_column(r#"{"layout_schema_version":1}"#),
            LayoutColumnState::NotV2 { version: 1 }
        );
    }

    #[test]
    fn v2_payload_is_usable() {
        let state = classify_layout_column(&v2_json());
        assert!(state.is_usable());
        assert!(
            matches!(&state, LayoutColumnState::Usable(meta) if meta.layout_schema_version == 2),
            "{state:?}"
        );
    }

    /// 三类不可用都必须被判成「需要回填」：旧实现漏掉的正是 v1 与坏 JSON 两类。
    #[test]
    fn every_unusable_state_needs_backfill() {
        for raw in ["", "\n", "{bad", r#"{"layout_schema_version":1}"#] {
            let state = classify_layout_column(raw);
            assert!(state.needs_backfill(), "{raw:?} -> {state:?}");
            assert!(!state.is_usable(), "{raw:?} -> {state:?}");
        }
        assert!(!classify_layout_column(&v2_json()).needs_backfill());
    }
}
