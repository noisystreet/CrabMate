//! `/user-data/workspaces/current/sessions` 会话列表行的公开瘦投影。
//!
//! 服务端对 `sessions` 数组做**透传存储**（行由 Client 全量 `ChatSession` 落盘，含
//! `messages` 等未列出字段）；本模块仅公开列表/续聊所需的**子集投影**（issue #939 B.3），
//! 线上 JSON 形状不变，服务端不做归一化。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `GET /user-data/workspaces/current/sessions` 中 `sessions` 数组元素的瘦投影。
///
/// **不**使用 `deny_unknown_fields`：存储行是 Client 全量 `ChatSession`，本类型只投影
/// 列表/续聊消费的字段子集；除 `id` 外全部容缺席省，旧数据 / 新增字段均不受影响。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionListRow {
    /// 会话本地 Web `id`（Client 生成；存储行主键）。
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// 最近更新时间戳（旧数据缺省为 0）。
    #[serde(default)]
    pub updated_at: i64,
    /// 置顶（侧栏排序优先于收藏）。
    #[serde(default)]
    pub pinned: bool,
    /// 收藏（侧栏排序次于置顶）。
    #[serde(default)]
    pub starred: bool,
    /// 与服务端 `conversation_id` 对齐；无则纯本地会话（不可拿 `id` 冒充续聊）。
    #[serde(default)]
    pub server_conversation_id: Option<String>,
    /// 最近一次已知的 `conversation_saved.revision` 或服务端消息 revision。
    #[serde(default)]
    pub server_revision: Option<u64>,
    /// 本会话绑定的 Web 工作区根；旧数据缺省为不绑定。
    #[serde(default)]
    pub workspace_root: Option<String>,
}

impl SessionListRow {
    /// 从 `GET` 响应的 `sessions` `Value` 解析瘦行列表。
    ///
    /// 非数组返回空；缺 `id` / 形状不符的行跳过（与 Client 既有 `filter_map` 语义一致）。
    #[must_use]
    pub fn parse_rows(sessions: &Value) -> Vec<Self> {
        let Some(arr) = sessions.as_array() else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|v| serde_json::from_value::<Self>(v.clone()).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_full_row_and_minimal_row() {
        let row: SessionListRow = serde_json::from_value(json!({
            "id": "s1",
            "title": "hi",
            "updated_at": 1_700_000_000,
            "pinned": true,
            "starred": false,
            "server_conversation_id": "c1",
            "server_revision": 7,
            "workspace_root": "/w"
        }))
        .expect("full row parses");
        assert_eq!(row.id, "s1");
        assert_eq!(row.server_revision, Some(7));
        assert_eq!(row.workspace_root.as_deref(), Some("/w"));

        let row: SessionListRow =
            serde_json::from_value(json!({ "id": "s2" })).expect("minimal row parses");
        assert_eq!(row.title, "");
        assert_eq!(row.updated_at, 0);
        assert!(!row.pinned && !row.starred);
        assert_eq!(row.server_conversation_id, None);
    }

    #[test]
    fn tolerates_unknown_storage_fields() {
        // 存储行是 Client 全量 ChatSession：messages / history_* 等字段必须容忍。
        let row: SessionListRow = serde_json::from_value(json!({
            "id": "s3",
            "title": "t",
            "messages": [{"role": "user", "content": "hi"}],
            "layout_schema_version": 2,
            "history_total": 42,
            "draft": "d"
        }))
        .expect("row with storage-only fields parses");
        assert_eq!(row.id, "s3");
    }

    #[test]
    fn parse_rows_lenient_semantics() {
        let rows = SessionListRow::parse_rows(&json!([
            {"id": "s1", "server_conversation_id": "c1"},
            {"no_id": true},
            {"id": "  "}
        ]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "s1");

        assert!(SessionListRow::parse_rows(&json!({"id": "x"})).is_empty());
    }
}
