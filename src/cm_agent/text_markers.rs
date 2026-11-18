//! 文案标记匹配的唯一实现（#1422 A 组）。
//!
//! 为什么要有这一层：`text.contains(marker)` 在英文里几乎必然误命中——`address` 含 `add`、
//! `Makefile` 含 `file`、`make sure` 含 `make`。这类误命中在两个方向都会污染行为：
//! 抑制类判定会吞掉真实工具调用，推进类判定会注入多余反馈。
//!
//! ## 口径
//!
//! - **纯 ASCII 标记按 token 边界**：前后不能是 `[A-Za-z0-9_]`，于是 `add` 不命中 `address`。
//! - **尾随空格按字面参与匹配**：空格不是词内字符，于是 `"make "` 天然要求「`make` 后面跟空白」——
//!   这是把 `make -j8` 与 `makefile` 区分开的唯一线索。
//! - **含非 ASCII 的标记（中文）保留子串匹配**：中文没有词边界，按字符切分只会漏判。
//!   中文的已知误命中靠用例钉住，不靠边界。
//! - **首 / 末字节非词内字符时豁免该侧边界**：`.tar` 的左边界由 `.` 顶替、`"make\n"` 的右边界由换行顶替——
//!   两侧对称，不再只特判尾随空格。
//!
//! ## 取舍（写进 #1422 验收）
//!
//! 加边界后英文标记命中变少，因此在「命中 → 抑制」的表上表现为**抑制变少、工具调用变多**
//! （token / 耗时上升）；在「命中 → 放行」的表上反过来表现为**抑制变多**。后者是反向风险，
//! 调用点必须显式把被收窄掉的语义补回表里（#1422 A2 的补偿词即由此而来）。

/// 标记字符是否属于「词内」：ASCII 字母数字或下划线。
///
/// 下划线写成 `b == 95` 而非 `b'_'` 字面量：本仓 lizard 会把 `'_'` 当生命周期起始，
/// 字符串扫描错位后会连带算错函数边界（与 `evidence.rs` 同款注释）。
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == 95
}

/// 标记首 / 末字节是否词内 —— 决定该侧是否还需要独立边界检查。
///
/// 词内字符开头（`add`）必须查左边界；非词内字符开头（`.tar`）自带分隔，豁免。
/// 这与「尾随空格顶替右边界」是同一条对称规则：边界字符本来就已经占住了那一侧。
fn marker_needs_left_boundary(marker: &str) -> bool {
    marker.as_bytes().first().copied().is_some_and(is_token_byte)
}

fn marker_needs_right_boundary(marker: &str) -> bool {
    marker.as_bytes().last().copied().is_some_and(is_token_byte)
}

/// 在 `text_lower` 里找带边界的 `marker`；两侧各自的边界按需启用。
fn find_bounded(text_lower: &str, marker: &str, need_left: bool, need_right: bool) -> bool {
    let bytes = text_lower.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text_lower.get(from..).and_then(|rest| rest.find(marker)) {
        let start = from.saturating_add(rel);
        let end = start.saturating_add(marker.len());
        let left_ok = !need_left || start == 0 || !is_token_byte(bytes[start - 1]);
        let right_ok = !need_right || end >= bytes.len() || !is_token_byte(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        // `marker` 全是 ASCII，`start + 1` 必然落在字符边界上，不会切碎多字节字符。
        from = start.saturating_add(1);
    }
    false
}

/// 单个标记是否命中。`text_lower` 必须已经 `to_lowercase()`。
#[must_use]
pub fn marker_matches(text_lower: &str, marker: &str) -> bool {
    if marker.is_empty() {
        return false;
    }
    if !marker.is_ascii() {
        // 中文等非 ASCII 标记没有词边界可依，保留子串语义。
        return text_lower.contains(marker);
    }
    // **不 trim**：`"make "` 的尾随空格就是「`make` 后面必须跟空白」这条线索本身
    // （`make -j8` 是构建，`makefile` 不是）。
    let need_left = marker_needs_left_boundary(marker);
    let need_right = marker_needs_right_boundary(marker);
    if !need_left && !need_right {
        // 整条标记都是分隔符（如 `" - "`）：两侧都由标记自身顶替，退化为子串。
        return text_lower.contains(marker);
    }
    find_bounded(text_lower, marker, need_left, need_right)
}

/// 任一标记命中（与 [`marker_matches`] 同口径）。
#[must_use]
pub fn text_matches_any(text_lower: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| marker_matches(text_lower, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_boundary_rejects_substring_hits() {
        // #1422 验收点名的误命中：`add` 藏在 `address` 里。
        assert!(!marker_matches("please address this", "add"));
        assert!(!marker_matches("grep makefile -n", "file"));
        assert!(marker_matches("add a test", "add"));
        assert!(marker_matches("please add this", "add"));
    }

    #[test]
    fn underscore_is_part_of_token() {
        assert!(!marker_matches("run_command", "command"));
        assert!(marker_matches("run command now", "command"));
    }

    #[test]
    fn trailing_space_marker_requires_only_left_boundary() {
        assert!(marker_matches("make -j8", "make "));
        assert!(!marker_matches("makefile", "make "));
    }

    /// 回归：空格之后的字符仍是词内字符（`make arch`），但空格已经顶替了右边界。
    /// 漏了这条会静默判掉「`make ` 命中 `make arch=Linux`」——A5 的兜底表就靠它。
    #[test]
    fn trailing_space_marker_ignores_next_token_boundary() {
        assert!(marker_matches("make arch=linux", "make "));
        assert!(marker_matches("run make now", "make "));
        assert!(marker_matches("命令：make -j8", "make -j8"));
    }

    #[test]
    fn cjk_markers_keep_substring_matching() {
        assert!(marker_matches("检查编译产物是否存在", "检查"));
        assert!(marker_matches("重新运行测试", "运行测试"));
    }

    #[test]
    fn multiword_ascii_markers_use_outer_boundaries() {
        assert!(marker_matches("please run again", "run again"));
        assert!(!marker_matches("rerun again", "run again"));
    }

    #[test]
    fn leading_separator_marker_needs_no_left_boundary() {
        // 扩展名类标记以 `.` 开头：左边界由 `.` 自己顶替，`xhpcg.tar.gz` 必须命中。
        assert!(marker_matches("xhpcg.tar.gz", ".tar"));
        assert!(!marker_matches("notar", ".tar"));
    }

    #[test]
    fn trailing_newline_marker_is_bounded_by_the_newline() {
        // 回归：`"make\n"` 曾经要求右边界，于是 `make\n<内容>` 永不命中（A5 词表里就有这一条）。
        assert!(marker_matches("make\nfoo", "make\n"));
        assert!(!marker_matches("maker\nfoo", "make\n"));
    }

    #[test]
    fn all_separator_marker_falls_back_to_substring() {
        assert!(marker_matches("a - b", " - "));
        assert!(!marker_matches("anything", " "));
    }

    #[test]
    fn empty_marker_never_matches() {
        assert!(!marker_matches("anything", ""));
        assert!(!marker_matches("anything", " "));
    }

    #[test]
    fn text_matches_any_is_any_of() {
        assert!(text_matches_any("please add", &["fix", "add"]));
        assert!(!text_matches_any("please address", &["fix", "add"]));
    }
}
