//! e2e 测试错误自动分类与排障建议。
//!
//! 复用 `error_playbook.rs` 的分类逻辑，提供面向 e2e 场景的轻量分类器。
//! 完整版见 **`docs/e2e-real-llm-testing-plan.md`** §4.5。

use std::path::Path;

/// e2e 测试错误分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E2eErrorKind {
    LlmAuth,
    LlmRateLimit,
    LlmBadRequest,
    LlmServer,
    LlmNetwork,
    LlmSseEarlyStop,
    LlmJsonParse,
    ToolExecution,
    ToolTimeout,
    AgentMaxRounds,
    Other,
}

impl E2eErrorKind {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LlmAuth => "LLM_AUTH",
            Self::LlmRateLimit => "LLM_RATE_LIMIT",
            Self::LlmBadRequest => "LLM_BAD_REQUEST",
            Self::LlmServer => "LLM_SERVER",
            Self::LlmNetwork => "LLM_NETWORK",
            Self::LlmSseEarlyStop => "LLM_SSE_EARLY_STOP",
            Self::LlmJsonParse => "LLM_JSON_PARSE",
            Self::ToolExecution => "TOOL_EXECUTION",
            Self::ToolTimeout => "TOOL_TIMEOUT",
            Self::AgentMaxRounds => "AGENT_MAX_ROUNDS",
            Self::Other => "OTHER",
        }
    }
}

/// 错误分析报告。
///
/// 骨架版中字段未使用，但保留完整定义供后续 PR 接入。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ErrorReport {
    pub kind: E2eErrorKind,
    pub raw: String,
    pub playbook_advice: Vec<String>,
    pub artifacts_dir: std::path::PathBuf,
}

/// 对错误文本进行分类，返回 [`ErrorReport`]。
///
/// 匹配规则基于常见异常字符串；不易误判时先行命中。
#[allow(dead_code)]
pub fn classify(err: &str, artifacts_dir: &Path) -> ErrorReport {
    let kind = classify_kind(err);
    let playbook_advice = advice_for_kind(kind);
    ErrorReport {
        kind,
        raw: err.to_string(),
        playbook_advice,
        artifacts_dir: artifacts_dir.to_path_buf(),
    }
}

fn classify_kind(err: &str) -> E2eErrorKind {
    let e = err.to_lowercase();
    if e.contains("401")
        || e.contains("403")
        || e.contains("unauthorized")
        || e.contains("forbidden")
    {
        return E2eErrorKind::LlmAuth;
    }
    if e.contains("429") || e.contains("rate limit") || e.contains("too many requests") {
        return E2eErrorKind::LlmRateLimit;
    }
    if e.contains("400") || e.contains("bad request") {
        return E2eErrorKind::LlmBadRequest;
    }
    if e.contains("5") && (e.contains("00") || e.contains("02") || e.contains("03")) {
        return E2eErrorKind::LlmServer;
    }
    if e.contains("dns")
        || e.contains("timeout")
        || e.contains("connection refused")
        || e.contains("connection reset")
    {
        return E2eErrorKind::LlmNetwork;
    }
    if e.contains("sse") && (e.contains("early") || e.contains("stop") || e.contains("eof")) {
        return E2eErrorKind::LlmSseEarlyStop;
    }
    if e.contains("json") && (e.contains("parse") || e.contains("syntax")) {
        return E2eErrorKind::LlmJsonParse;
    }
    if e.contains("tool") && (e.contains("timeout") || e.contains("timed out")) {
        return E2eErrorKind::ToolTimeout;
    }
    if e.contains("tool") && e.contains("execute") {
        return E2eErrorKind::ToolExecution;
    }
    if e.contains("max round") || e.contains("agent_loop_limit") || e.contains("turn budget") {
        return E2eErrorKind::AgentMaxRounds;
    }
    E2eErrorKind::Other
}

fn advice_for_kind(kind: E2eErrorKind) -> Vec<String> {
    match kind {
        E2eErrorKind::LlmAuth => vec![
            "检查 API_KEY 环境变量是否设置正确".to_string(),
            "确认 llm_http_auth_mode 与供应商要求一致".to_string(),
        ],
        E2eErrorKind::LlmRateLimit => vec![
            "降低请求频率或升级套餐".to_string(),
            "检查是否有其他进程共享同一 API 密钥".to_string(),
        ],
        E2eErrorKind::LlmBadRequest => vec![
            "检查请求体格式（messages / tools 等）".to_string(),
            "确认 model 名称在供应商端有效".to_string(),
        ],
        E2eErrorKind::LlmServer => vec![
            "LLM 服务端错误，稍后重试".to_string(),
            "检查供应商状态页".to_string(),
        ],
        E2eErrorKind::LlmNetwork => vec![
            "检查网络连接与 DNS 解析".to_string(),
            "确认 api_base URL 可访问".to_string(),
        ],
        E2eErrorKind::LlmSseEarlyStop => vec![
            "SSE 流提前结束，检查完整 trace 定位原因".to_string(),
            "可能为模型返回超长思考或内容截断".to_string(),
        ],
        E2eErrorKind::LlmJsonParse => vec!["LLM 返回非 JSON 响应，检查原始响应内容".to_string()],
        E2eErrorKind::ToolExecution => vec!["工具执行异常，检查工具返回值".to_string()],
        E2eErrorKind::ToolTimeout => vec!["工具执行超时，检查工具耗时或增大超时阈值".to_string()],
        E2eErrorKind::AgentMaxRounds => vec![
            "Agent 达到最大轮次限制，检查是否会循环".to_string(),
            "增大 max_rounds 或优化提示词".to_string(),
        ],
        E2eErrorKind::Other => vec!["未分类错误，请检查原始错误信息并补充分类规则".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_auth_error() {
        let report = classify("401 Unauthorized: invalid API key", Path::new("/tmp"));
        assert_eq!(report.kind, E2eErrorKind::LlmAuth);
        assert!(!report.playbook_advice.is_empty());
    }

    #[test]
    fn classify_rate_limit() {
        let report = classify("429 Too Many Requests", Path::new("/tmp"));
        assert_eq!(report.kind, E2eErrorKind::LlmRateLimit);
    }

    #[test]
    fn classify_network_timeout() {
        let report = classify("connection timeout after 30s", Path::new("/tmp"));
        assert_eq!(report.kind, E2eErrorKind::LlmNetwork);
    }

    #[test]
    fn classify_sse_early_stop() {
        let report = classify("SSE stream early EOF", Path::new("/tmp"));
        assert_eq!(report.kind, E2eErrorKind::LlmSseEarlyStop);
    }

    #[test]
    fn classify_unknown_other() {
        let report = classify("some random error", Path::new("/tmp"));
        assert_eq!(report.kind, E2eErrorKind::Other);
    }
}
