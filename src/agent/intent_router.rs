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
const EXECUTE_HIGH_THRESHOLD: f32 = 0.45;

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

const READONLY_OBJECT_KEYWORDS: &[&str] = &[
    "目录", "文件", "源码", "项目", "仓库", "提交", "分支", "函数", "src", ".rs", ".ts", ".md",
];

const RUN_TEST_BUILD_ACTION_KEYWORDS: &[&str] = &[
    "跑",
    "运行",
    "执行",
    "测试",
    "test",
    "cargo test",
    "pytest",
    "编译",
    "构建",
    "build",
    "cargo build",
];

const DEBUG_ACTION_KEYWORDS: &[&str] = &[
    "报错", "异常", "panic", "失败", "定位", "排查", "分析", "调试", "error", "bug",
];

const CODE_CHANGE_ACTION_KEYWORDS: &[&str] = &["改", "修改", "重构", "实现"];
const CODE_CHANGE_OBJECT_KEYWORDS: &[&str] = &["函数", "文件", "模块", ".rs", ".ts", ".py", "代码"];
const DOCS_ACTION_KEYWORDS: &[&str] = &["更新", "补充", "完善", "整理", "编写"];
const DOCS_OBJECT_KEYWORDS: &[&str] = &["readme", "文档", "docs", "注释", ".md"];
const GIT_WRITE_ACTION_KEYWORDS: &[&str] = &[
    "提交",
    "commit",
    "pr",
    "pull request",
    "rebase",
    "cherry-pick",
    "merge",
];

const READONLY_LISTING_KEYWORDS: &[&str] = &[
    "当前目录",
    "有哪些",
    "有什么",
    "有没有",
    "有无",
    "在不在",
    "是否有",
    "是否存在",
    "列出",
    "查看",
    "列一下",
    "清单",
    "文件列表",
    "源文件",
    "源码",
    "目录下",
    "list",
    "show files",
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

const EXPLICIT_EXECUTE_CONFIRM_KEYWORDS: &[&str] = &[
    "直接开始执行",
    "开始执行",
    "直接执行",
    "确认执行",
    "继续执行",
    "现在执行",
    "马上执行",
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
    if is_readonly_listing_request(&normalized) {
        return IntentAssessment {
            kind: IntentKind::Execute,
            confidence: 0.78,
            route: IntentRoute::Execute,
        };
    }
    if is_negative_execute_request(&normalized) {
        return IntentAssessment {
            kind: IntentKind::Qa,
            confidence: 0.8,
            route: IntentRoute::DirectReply(
                "收到，我先不执行任何改动。你可以告诉我你想先了解哪部分。".to_string(),
            ),
        };
    }
    if is_explicit_execute_confirmation(&normalized) {
        return IntentAssessment {
            kind: IntentKind::Execute,
            confidence: 0.52,
            route: IntentRoute::ConfirmThenExecute(EXECUTE_CONFIRM.to_string()),
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
    if has_pair_hit(
        s,
        RUN_TEST_BUILD_ACTION_KEYWORDS,
        &["test", "cargo", "构建", "编译"],
    ) {
        score += 0.2;
    }
    if has_pair_hit(
        s,
        DEBUG_ACTION_KEYWORDS,
        &["报错", "异常", "panic", "error"],
    ) {
        score += 0.15;
    }
    if has_pair_hit(s, CODE_CHANGE_ACTION_KEYWORDS, CODE_CHANGE_OBJECT_KEYWORDS) {
        score += 0.15;
    }
    if has_pair_hit(s, DOCS_ACTION_KEYWORDS, DOCS_OBJECT_KEYWORDS) {
        score += 0.22;
    }
    if GIT_WRITE_ACTION_KEYWORDS.iter().any(|k| s.contains(k)) {
        score += 0.2;
    }
    if s.chars().count() > 20 {
        score += 0.1;
    }
    score.min(1.0)
}

fn is_readonly_listing_request(s: &str) -> bool {
    let has_listing_phrase = READONLY_LISTING_KEYWORDS.iter().any(|k| s.contains(k));
    let has_listing_object = READONLY_OBJECT_KEYWORDS.iter().any(|k| s.contains(k));
    (has_listing_phrase && has_listing_object)
        || (s.contains('有') && (s.contains("源码") || s.contains("文件")) && s.contains('吗'))
}

fn has_pair_hit(s: &str, actions: &[&str], objects: &[&str]) -> bool {
    actions.iter().any(|k| s.contains(k)) && objects.iter().any(|k| s.contains(k))
}

fn is_negative_execute_request(s: &str) -> bool {
    let negations = [
        "不要执行",
        "先别改",
        "先不要改",
        "只是想知道",
        "仅解释",
        "先解释",
    ];
    negations.iter().any(|k| s.contains(k))
}

pub fn is_explicit_execute_confirmation(s: &str) -> bool {
    EXPLICIT_EXECUTE_CONFIRM_KEYWORDS
        .iter()
        .any(|k| s.contains(k))
}

/// 助手是否正在等待用户确认执行（供多轮上下文复用，避免调用方硬编码文案片段）。
pub fn is_waiting_execute_confirmation_prompt(assistant_text: &str) -> bool {
    let t = assistant_text.trim();
    let t_lower = t.to_lowercase();
    !t.is_empty()
        && (t == EXECUTE_CONFIRM
            || t.contains("请确认是否“直接开始执行”")
            || (t.contains("请确认是否") && (t.contains("开始执行") || t.contains("直接执行")))
            || (t_lower.contains("confirm")
                && (t_lower.contains("execute") || t_lower.contains("run"))))
}

#[cfg(test)]
mod tests {
    use super::{
        EXECUTE_CONFIRM, IntentKind, IntentRoute, is_waiting_execute_confirmation_prompt,
        route_user_task,
    };

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

    #[test]
    fn explicit_start_execute_routes_to_execute() {
        let r = route_user_task("直接开始执行");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::ConfirmThenExecute(_)));
    }

    #[test]
    fn waiting_execute_confirmation_prompt_detected() {
        assert!(is_waiting_execute_confirmation_prompt(EXECUTE_CONFIRM));
        assert!(is_waiting_execute_confirmation_prompt(
            "我判断你可能想让我直接执行任务。请确认是否“直接开始执行”"
        ));
        assert!(is_waiting_execute_confirmation_prompt(
            "请确认是否开始执行，或补充具体范围。"
        ));
        assert!(!is_waiting_execute_confirmation_prompt("你好！"));
    }

    #[test]
    fn readonly_listing_request_routes_to_execute() {
        let r = route_user_task("当前目录下有哪些源文件");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::Execute));
    }

    #[test]
    fn current_dir_has_what_routes_to_execute() {
        let r = route_user_task("当前目录下有什么");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::Execute));
    }

    #[test]
    fn has_hpcg_source_code_routes_to_execute() {
        let r = route_user_task("有hpcg的源码吗？");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(r.route, IntentRoute::Execute));
    }

    #[test]
    fn run_test_pair_routes_to_execute() {
        let r = route_user_task("帮我跑一下 cargo test");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(
            r.route,
            IntentRoute::Execute | IntentRoute::ConfirmThenExecute(_)
        ));
    }

    #[test]
    fn debug_pair_routes_to_execute() {
        let r = route_user_task("这个报错帮我定位下原因");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(
            r.route,
            IntentRoute::Execute | IntentRoute::ConfirmThenExecute(_)
        ));
    }

    #[test]
    fn code_change_pair_routes_to_execute_or_confirm() {
        let r = route_user_task("帮我修改这个函数实现");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(
            r.route,
            IntentRoute::Execute | IntentRoute::ConfirmThenExecute(_)
        ));
    }

    #[test]
    fn ability_question_should_not_route_to_execute() {
        let r = route_user_task("你有哪些技能");
        assert_eq!(r.kind, IntentKind::Qa);
    }

    #[test]
    fn docs_action_pair_routes_to_execute_or_confirm() {
        let r = route_user_task("请更新 README 文档");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(
            r.route,
            IntentRoute::Execute | IntentRoute::ConfirmThenExecute(_)
        ));
    }

    #[test]
    fn git_write_routes_to_execute_or_confirm() {
        let r = route_user_task("把这些改动提交并开 PR");
        assert_eq!(r.kind, IntentKind::Execute);
        assert!(matches!(
            r.route,
            IntentRoute::Execute | IntentRoute::ConfirmThenExecute(_)
        ));
    }

    #[test]
    fn negative_execute_routes_to_qa_direct_reply() {
        let r = route_user_task("先别改代码，我只是想知道这个报错什么意思");
        assert_eq!(r.kind, IntentKind::Qa);
        assert!(matches!(r.route, IntentRoute::DirectReply(_)));
    }
}
