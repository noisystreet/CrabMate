//! 首轮意图快路径：将输入分为 greeting / qa / execute / ambiguous，
//! 并附带置信度阈值分流，避免误触发工具执行。

/// 意图类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Greeting,
    Qa,
    Execute,
    Ambiguous,
}

/// 路由动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentRoute {
    /// 直接回复，不进入工具执行。
    DirectReply(String),
    /// 先追问再执行。
    AskThenExecute(String),
    /// 中等置信度：先确认再执行。
    ConfirmThenExecute(String),
    /// 高置信度：直接执行。
    Execute,
}

/// 评估结果。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentAssessment {
    pub kind: IntentKind,
    pub confidence: f32,
    pub route: IntentRoute,
}

const GREETING_REPLY: &str = "你好！我在这，想先处理什么问题？";
const QA_REPLY: &str = "我可以帮你定位和修复 bug、改代码、跑构建和测试、解释报错、做代码审查、整理文档和提交 commit。你想先让我做哪一项？";
const AMBIGUOUS_ASK: &str =
    "我理解你可能希望我直接动手处理。请补充具体目标（文件/报错/命令/期望结果），我再开始执行。";
const EXECUTE_CONFIRM: &str =
    "我判断你可能想让我直接执行任务。请确认是否“直接开始执行”，或补充更具体范围。";

const EXECUTE_LOW_THRESHOLD: f32 = 0.2;
const EXECUTE_HIGH_THRESHOLD: f32 = 0.55;

/// 执行意图阈值（用于阈值可配置化）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecuteIntentThresholds {
    pub low: f32,
    pub high: f32,
}

impl Default for ExecuteIntentThresholds {
    fn default() -> Self {
        Self {
            low: EXECUTE_LOW_THRESHOLD,
            high: EXECUTE_HIGH_THRESHOLD,
        }
    }
}

const GREETING_KEYWORDS: &[&str] = &[
    "你好",
    "您好",
    "哈喽",
    "嗨",
    "在吗",
    "hello",
    "hi",
    "hey",
    "thanks",
    "thank you",
    "谢谢",
];

const EXECUTION_HINT_KEYWORDS: &[&str] = &[
    "修", "改", "实现", "增加", "删除", "重构", "排查", "调试", "运行", "执行", "提交", "commit",
    "push", "deploy", "报错", "error", "异常", "panic", "cargo", "npm", "pnpm", "git", "python",
];

const QA_HINT_KEYWORDS: &[&str] = &[
    "怎么",
    "如何",
    "什么",
    "为什么",
    "区别",
    "建议",
    "原理",
    "思路",
    "解释",
    "技能",
    "你会什么",
    "你能做什么",
    "你有哪些",
    "what can you do",
    "what",
    "why",
    "how",
];

/// 多分类意图路由 + 置信度阈值分流。
pub fn route_user_task(task: &str) -> IntentAssessment {
    route_user_task_with_thresholds(task, ExecuteIntentThresholds::default())
}

/// 多分类意图路由 + 可配置执行阈值分流。
pub fn route_user_task_with_thresholds(
    task: &str,
    thresholds: ExecuteIntentThresholds,
) -> IntentAssessment {
    let normalized = task.trim().to_lowercase();

    if is_greeting_only(task) {
        return IntentAssessment {
            kind: IntentKind::Greeting,
            confidence: 0.98,
            route: IntentRoute::DirectReply(GREETING_REPLY.to_string()),
        };
    }
    if is_qa_only(&normalized) {
        return IntentAssessment {
            kind: IntentKind::Qa,
            confidence: 0.85,
            route: IntentRoute::DirectReply(QA_REPLY.to_string()),
        };
    }

    let execute_score = execution_confidence(&normalized);
    if execute_score >= thresholds.high {
        return IntentAssessment {
            kind: IntentKind::Execute,
            confidence: execute_score,
            route: IntentRoute::Execute,
        };
    }
    if execute_score >= thresholds.low {
        return IntentAssessment {
            kind: IntentKind::Execute,
            confidence: execute_score,
            route: IntentRoute::ConfirmThenExecute(EXECUTE_CONFIRM.to_string()),
        };
    }

    IntentAssessment {
        kind: IntentKind::Ambiguous,
        confidence: 0.3,
        route: IntentRoute::AskThenExecute(AMBIGUOUS_ASK.to_string()),
    }
}

fn is_greeting_only(raw: &str) -> bool {
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if contains_execution_hint(&normalized) {
        return false;
    }

    // 去掉常见标点后做关键字匹配，避免“你好！！！”漏判。
    let compact: String = normalized
        .chars()
        .filter(|c| {
            !c.is_whitespace() && !matches!(c, ',' | '.' | '!' | '?' | '，' | '。' | '！' | '？')
        })
        .collect();

    if GREETING_KEYWORDS.iter().any(|k| compact == *k) {
        return true;
    }
    // 短句且完全由寒暄词组成，也视为寒暄。
    compact.chars().count() <= 12 && GREETING_KEYWORDS.iter().any(|k| compact.contains(k))
}

fn contains_execution_hint(s: &str) -> bool {
    EXECUTION_HINT_KEYWORDS.iter().any(|k| s.contains(k))
        || s.contains('/')
        || s.contains(".rs")
        || s.contains(".ts")
        || s.contains(".md")
        || s.contains('@')
}

fn is_qa_only(s: &str) -> bool {
    if s.is_empty() || contains_execution_hint(s) {
        return false;
    }
    QA_HINT_KEYWORDS.iter().any(|k| s.contains(k))
}

fn execution_confidence(s: &str) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let mut score = 0.0_f32;
    for k in EXECUTION_HINT_KEYWORDS {
        if s.contains(k) {
            score += 0.28;
        }
    }
    if s.contains('/') || s.contains('@') {
        score += 0.2;
    }
    if s.contains(".rs") || s.contains(".ts") || s.contains(".md") {
        score += 0.2;
    }
    if s.chars().count() > 20 {
        score += 0.1;
    }
    score.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::{IntentKind, IntentRoute, route_user_task};

    #[test]
    fn greeting_routes_to_direct_reply() {
        let r = route_user_task("你好");
        assert_eq!(r.kind, IntentKind::Greeting);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn greeting_with_punctuation_routes_to_direct_reply() {
        let r = route_user_task("hello!!!");
        assert_eq!(r.kind, IntentKind::Greeting);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn execution_request_high_confidence_routes_to_execute() {
        let r = route_user_task("帮我修复 tauri 导出 markdown 报错");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::Execute));
    }

    #[test]
    fn qa_routes_to_direct_reply() {
        let r = route_user_task("这个错误是什么意思？");
        assert_eq!(r.kind, IntentKind::Qa);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn skill_question_routes_to_qa() {
        let r = route_user_task("你有哪些技能");
        assert_eq!(r.kind, IntentKind::Qa);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn capability_question_routes_to_qa() {
        let r = route_user_task("你能帮我做什么");
        assert_eq!(r.kind, IntentKind::Qa);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn ability_scope_question_routes_to_qa() {
        let r = route_user_task("你的能力范围是什么");
        assert_eq!(r.kind, IntentKind::Qa);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }

    #[test]
    fn ambiguous_routes_to_ask_then_execute() {
        let r = route_user_task("帮我看看");
        assert_eq!(r.kind, IntentKind::Ambiguous);
        assert!(matches!(r.route, IntentRoute::AskThenExecute(_)));
    }

    #[test]
    fn medium_confidence_execute_routes_to_confirm() {
        let r = route_user_task("帮我改下这个");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::ConfirmThenExecute(_)));
    }
}
