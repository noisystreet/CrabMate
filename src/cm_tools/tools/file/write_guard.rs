//! `modify_file` 的内容守卫：`guard` 子对象的**单一读取入口**。
//!
//! 键名与工具参数一致（`content` / `first_line` / `last_line` / `anchor_line`），
//! 顶层参数、`edits[i]`、以及 `apply_patch` 之外的局部编辑共用同一套语义。
//!
//! 兼容性：旧的顶层 `expect_content` / `expect_first_line` / `expect_last_line` /
//! `expect_line_content` 仍可被读取（新键优先），以便历史会话回放与工具参数纠错层
//! 把旧键上抬到 `guard` 之前的调用仍能工作；工具 schema 只暴露 `guard`。

use serde_json::Value;

/// 守卫键名：整段校验（`replace_lines` 的 `[start_line..=end_line]`）。
pub(super) const KEY_CONTENT: &str = "content";
/// 守卫键名：区间首行锚定（`replace_lines` 的 `start_line`）。
pub(super) const KEY_FIRST_LINE: &str = "first_line";
/// 守卫键名：区间末行锚定（`replace_lines` 的 `end_line`）。
pub(super) const KEY_LAST_LINE: &str = "last_line";
/// 守卫键名：锚点行校验（`insert_after_line` 的 `after_line`）。
pub(super) const KEY_ANCHOR_LINE: &str = "anchor_line";

/// 旧版平铺键 → 新键。仅用于兜底读取，**不再**写入 schema / description。
const LEGACY_KEYS: [(&str, &str); 4] = [
    (KEY_CONTENT, "expect_content"),
    (KEY_FIRST_LINE, "expect_first_line"),
    (KEY_LAST_LINE, "expect_last_line"),
    (KEY_ANCHOR_LINE, "expect_line_content"),
];

/// 一次调用里的守卫取值视图；全部为 `None` 表示未使用守卫。
pub(super) struct GuardView<'a> {
    /// `guard.content`：整段逐行校验。
    pub content: Option<&'a str>,
    /// `guard.first_line`：仅校验 `start_line`。
    pub first_line: Option<&'a str>,
    /// `guard.last_line`：仅校验 `end_line`。
    pub last_line: Option<&'a str>,
    /// `guard.anchor_line`：校验插入锚点行。
    pub anchor_line: Option<&'a str>,
}

impl GuardView<'_> {
    /// 是否设置了任意守卫。
    pub fn any(&self) -> bool {
        self.content.is_some()
            || self.first_line.is_some()
            || self.last_line.is_some()
            || self.anchor_line.is_some()
    }

    /// 是否只用了区间首/末行锚定（与 `content` 互斥的那一类）。
    pub fn edge_any(&self) -> bool {
        self.first_line.is_some() || self.last_line.is_some()
    }
}

/// 从参数对象（顶层或 `edits[i]`）读取守卫：`guard` 子对象优先，旧平铺键兜底。
pub(super) fn guard_view(v: &Value) -> GuardView<'_> {
    let guard = v.get("guard");
    let pick = |key: &str| -> Option<&str> {
        let legacy = LEGACY_KEYS
            .iter()
            .find_map(|(new, old)| (*new == key).then_some(*old));
        let from_guard = guard
            .and_then(|g| g.get(key))
            .and_then(|x| x.as_str());
        // 空串**不是**「未设置」：`first_line`/`last_line`/`anchor_line` 传空串表示
        // 「校验那一行确实是空行」，是合法用法——过滤掉它等于静默跳过守卫。
        let from_legacy = legacy
            .and_then(|old| v.get(old))
            .and_then(|x| x.as_str());
        from_guard.or(from_legacy)
    };
    GuardView {
        content: pick(KEY_CONTENT),
        first_line: pick(KEY_FIRST_LINE),
        last_line: pick(KEY_LAST_LINE),
        anchor_line: pick(KEY_ANCHOR_LINE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_guard_subobject() {
        let v = json!({"guard": {"content": "L1", "first_line": "A"}});
        let g = guard_view(&v);
        assert_eq!(g.content, Some("L1"));
        assert_eq!(g.first_line, Some("A"));
        assert!(g.last_line.is_none() && g.anchor_line.is_none());
        assert!(g.any() && g.edge_any());
    }

    #[test]
    fn guard_wins_over_legacy_flat_keys() {
        let v = json!({"guard": {"content": "new"}, "expect_content": "old"});
        assert_eq!(guard_view(&v).content, Some("new"));
    }

    #[test]
    fn legacy_flat_keys_still_readable() {
        let v = json!({"expect_line_content": "L2"});
        let g = guard_view(&v);
        assert_eq!(g.anchor_line, Some("L2"));
        assert!(g.any() && !g.edge_any());
    }

    #[test]
    fn empty_string_counts_as_set_guard() {
        // 空串 = 「校验该行为空行」，不是「未设置」；过滤掉会静默跳过守卫。
        let v = json!({"guard": {"content": ""}, "expect_first_line": ""});
        let g = guard_view(&v);
        assert_eq!(g.content, Some(""));
        assert_eq!(g.first_line, Some(""));
        assert!(g.any());
    }
}
