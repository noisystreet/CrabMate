//! CLI / 契约层共用的 LLM 错误串**启发式**分类（避免多处子串判断漂移）。
//!
//! 只服务**拿不到结构化信息**的路径（CLI 退出码、契约测试）；SSE / HTTP / 队列分流都已改为
//! 按 `RunAgentTurnError` 变体判定。曾经的 `is_user_cancelled_run_agent_error`（取消串比对）
//! 已随 #1478 删除——取消现在只看变体。

const QUOTA_OR_RATE_LIMIT_NEEDLES: &[&str] = &[
    "HTTP 429",
    "http 429",
    "status=429",
    "限流",
    "quota",
    "Quota",
    "HTTP 402",
    "http 402",
    "余额",
    "insufficient",
    "HTTP 503",
    "http 503",
    "status=503",
];

/// 配额 / 限流 / 余额类（与 `llm::api` 常见中文文案及 HTTP 状态片段对齐）。
///
/// **现在只服务 CLI 退出码归类与契约测试**（`classify_model_error_message` + `tests/cli_contract.rs`）：
/// #1440 K3 之后，SSE / HTTP 面的 `LLM_RATE_LIMIT` 与 `429` 已改由
/// [`crate::cm_llm::classify_llm_failure`] 在 `LlmCallError` 构造点**结构化**判定，不再读本函数。
///
/// 保留原因：CLI 退出码路径只拿到**错误字符串**、没有 HTTP status，只能靠文案。
/// 已知误判（`余额充足` / `insufficient evidence` / `限流说明文档`）由
/// `quota_needles_known_false_positives_are_pinned` 钉住——不要为了让用例转绿而加词删词。
pub fn is_quota_or_rate_limit_llm_message(msg: &str) -> bool {
    QUOTA_OR_RATE_LIMIT_NEEDLES.iter().any(|n| msg.contains(n))
}

/// 把 LLM / `run_agent_turn` 错误文案归类为 CLI 退出码（启发式，与 `llm::api` 用户可见串对齐）。
pub fn classify_model_error_message(msg: &str) -> i32 {
    use crate::cm_runtime::cli_exit::{EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT};

    if is_quota_or_rate_limit_llm_message(msg) {
        return EXIT_QUOTA_OR_RATE_LIMIT;
    }
    if msg.contains("模型接口返回错误")
        || msg.contains("模型返回内容无法解析")
        || msg.contains("非流式响应 choices 为空")
        || msg.contains("无 tool_calls")
    {
        return EXIT_MODEL_ERROR;
    }
    EXIT_GENERAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_heuristic_429() {
        assert!(is_quota_or_rate_limit_llm_message(
            "模型接口返回错误（HTTP 429）：x"
        ));
    }

    #[test]
    fn classify_model_error_maps_429_to_quota() {
        assert_eq!(
            classify_model_error_message("模型接口返回错误（HTTP 429）：too many requests"),
            crate::cm_runtime::cli_exit::EXIT_QUOTA_OR_RATE_LIMIT
        );
    }

    #[test]
    fn classify_model_error_maps_generic_model_line() {
        let got = classify_model_error_message("模型接口返回错误（HTTP 401）：bad key");
        assert_eq!(got, crate::cm_runtime::cli_exit::EXIT_MODEL_ERROR);
    }

    /// #1422 C4：喂给 needle 表的 HTTP 状态片段全覆盖（这些是「真命中」）。
    ///
    /// **本函数现仅服务 CLI 退出码归类**（#1440 K3 起，SSE / HTTP 面走结构化判定）：
    /// 改这里不会改变线协议行为，要改协议语义请改 `cm_llm::classify_llm_failure`。
    #[test]
    fn quota_needles_cover_status_fragments() {
        for text in [
            "模型接口返回错误（HTTP 429）：too many requests",
            "status=429",
            "模型接口返回错误（HTTP 402）：payment required",
            "status=503",
        ] {
            assert!(is_quota_or_rate_limit_llm_message(text), "应命中：{text}");
        }
    }

    /// #1422 C4：已知误判钉住（**故意断言 `true`**）。
    ///
    /// 子串反查使得「账户余额充足」「insufficient evidence」这类**不含配额语义**的正文
    /// 也被判成配额类，进而把 CLI 退出码归到 `EXIT_QUOTA_OR_RATE_LIMIT`，误导用户去充值。
    ///
    /// 这里不是要修它（该路径最终应改读 HTTP status），而是把行为钉住：
    /// 将来改成结构化判定时，这几条会一起变红，逼着同步更新契约而不是默默改词表。
    #[test]
    fn quota_needles_known_false_positives_are_pinned() {
        for text in [
            "模型接口返回错误（HTTP 401）：账户余额充足",
            "模型接口返回错误（HTTP 400）：insufficient evidence in payload",
            "限流说明文档：默认 60 rpm",
        ] {
            assert!(
                is_quota_or_rate_limit_llm_message(text),
                "已知误判被改动：{text}（若已改结构化判定，请同步更新本用例）"
            );
        }
    }
}
