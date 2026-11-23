//! `gh` 检查状态归一：`state`（展示串）/ `bucket`（稳定机器枚举）→ 五档。
//!
//! **为什么需要它**：`gh pr checks --json bucket,state` 里 `bucket` 是文档中的稳定取值
//! （`pass` / `fail` / `pending` / `skipping` / `cancel`），而 `state` 是**展示串**
//! （`SUCCESS` / `FAILURE` / `SKIPPED` / `NEUTRAL` / `CANCELLED`，跨 gh 版本会变）。
//!
//! 旧实现有两套重复逻辑，且都做了子串匹配：
//! - `contains("fail")` 命中 `unfailed`；`contains("ok")` 命中 `token` / `broken`；
//! - `SKIPPED` / `NEUTRAL` 落进 pending ⇒ PR 永久显示「待完成」。
//!
//! 这里改为：**`bucket` 优先**、**精确匹配**、五档穷举（#1478 B-α）。

/// 检查结果五档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

impl CheckBucket {
    /// `bucket` 优先（稳定枚举），解析不出再看 `state`（展示串），都解析不出按 pending。
    #[must_use]
    pub(super) fn of(state: &str, bucket: Option<&str>) -> Self {
        bucket
            .and_then(Self::from_token)
            .or_else(|| Self::from_token(state))
            .unwrap_or(Self::Pending)
    }

    /// 精确匹配（**不**用 `contains`）；大小写、`_` / `-` / 空格归一后比较。
    fn from_token(raw: &str) -> Option<Self> {
        let s = raw.trim().to_ascii_lowercase().replace(['_', '-', ' '], "");
        Some(match s.as_str() {
            "pass" | "passing" | "success" | "successful" => Self::Pass,
            "fail" | "failure" | "failed" | "error" => Self::Fail,
            "pending" | "queued" | "inprogress" | "waiting" | "expected" => Self::Pending,
            "skipping" | "skipped" | "neutral" => Self::Skipping,
            "cancel" | "cancelled" | "canceled" | "timedout" | "stale" => Self::Cancel,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CheckBucket;

    #[test]
    fn bucket_wins_over_state() {
        // `bucket` 是稳定枚举，`state` 只作 fallback。
        assert_eq!(
            CheckBucket::of("SUCCESS", Some("skipping")),
            CheckBucket::Skipping
        );
        assert_eq!(CheckBucket::of("SKIPPED", Some("pass")), CheckBucket::Pass);
    }

    #[test]
    fn skipping_and_neutral_are_not_pending() {
        // 旧实现把这两个都算成 pending ⇒ PR 永久显示「待完成」。
        assert_eq!(CheckBucket::of("SKIPPED", None), CheckBucket::Skipping);
        assert_eq!(CheckBucket::of("NEUTRAL", None), CheckBucket::Skipping);
    }

    #[test]
    fn substring_traps_are_not_misread() {
        // 旧 `contains("fail")` 命中 `unfailed`、`contains("ok")` 命中 `token` / `broken`。
        for trap in ["unfailed", "token", "broken"] {
            assert_eq!(
                CheckBucket::of(trap, None),
                CheckBucket::Pending,
                "{trap} 不应被当成其它档"
            );
        }
    }

    #[test]
    fn common_tokens_map() {
        assert_eq!(CheckBucket::of("SUCCESS", None), CheckBucket::Pass);
        assert_eq!(CheckBucket::of("FAILURE", None), CheckBucket::Fail);
        assert_eq!(CheckBucket::of("IN_PROGRESS", None), CheckBucket::Pending);
        assert_eq!(CheckBucket::of("in progress", None), CheckBucket::Pending);
        assert_eq!(CheckBucket::of("CANCELLED", None), CheckBucket::Cancel);
        assert_eq!(CheckBucket::of("TIMED_OUT", None), CheckBucket::Cancel);
        // state 缺失 / 未知时用 bucket。
        assert_eq!(CheckBucket::of("", Some("pass")), CheckBucket::Pass);
        assert_eq!(CheckBucket::of("?", None), CheckBucket::Pending);
    }
}
