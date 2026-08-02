//! 意图类型与门控辅助文案。
//!
//! 旧 L1 关键词路由（`route_user_task`）已移除：L2 不可用时由管线 **fail-open 进 Execute / 主模型**。
//! 本模块保留 `IntentKind`、确认流识别与 DirectReply 占位文案（供 L2 映射与多轮确认）。

/// 意图类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Greeting,
    Qa,
    Execute,
    Ambiguous,
}

const GREETING_REPLY: &str = "你好！我在这，想先处理什么问题？";
const QA_REPLY: &str = "我可以帮你定位和修复 bug、改代码、跑构建和测试、解释报错、做代码审查、整理文档和提交 commit。你想先让我做哪一项？";
/// 能力范围 / 自我介绍类占位文案（`qa.meta*`；意图门控开启时改由主模型生成，通常不展示）。
const QA_META_REPLY: &str = "我是 CrabMate，面向你当前工作区的编程助手：可读代码与目录、解释报错与概念、在确认后改代码与跑测试、整理文档与 Git 流程。你现在最想解决的是哪一类问题？";
const AMBIGUOUS_ASK: &str =
    "我理解你可能希望我直接动手处理。请补充具体目标（文件/报错/命令/期望结果），我再开始执行。";
pub const EXECUTE_CONFIRM: &str =
    "我判断你可能想让我直接执行任务。请确认是否“直接开始执行”，或补充更具体范围。";

const EXECUTE_LOW_THRESHOLD: f32 = 0.2;
const EXECUTE_HIGH_THRESHOLD: f32 = 0.45;

/// 执行意图阈值（历史配置仍可读；L1 路由移除后**不再**驱动决策，仅兼容 `IntentContext`）。
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

const EXPLICIT_EXECUTE_CONFIRM_KEYWORDS: &[&str] = &[
    "直接开始执行",
    "开始执行",
    "直接执行",
    "确认执行",
    "继续执行",
    "现在执行",
    "马上执行",
];

/// 首轮门控「只读 QA」类：`qa.readonly*`、`qa.codebase`（与 L2 `primary_intent` 对齐）。
#[must_use]
pub fn qa_readonly_style_primary(primary_intent: &str) -> bool {
    primary_intent.starts_with("qa.readonly") || primary_intent == "qa.codebase"
}

/// 能力/自我介绍类：`qa.meta` 与 `qa.meta.*`（与 L2 `primary_intent` 对齐）。
#[must_use]
pub fn qa_meta_style_primary(primary_intent: &str) -> bool {
    primary_intent == "qa.meta" || primary_intent.starts_with("qa.meta.")
}

/// 概念/含义解释类：`qa.explain`（与 L2 `primary_intent` 对齐）。
#[must_use]
pub fn qa_explain_style_primary(primary_intent: &str) -> bool {
    primary_intent == "qa.explain"
}

/// 管线中虽为 `DirectReply`，但意图门控**不**下发 canned，改由主模型生成（占位见 `qa_direct_reply_for_primary`）。
#[must_use]
pub fn intent_reply_delegates_to_main_model(kind: IntentKind, primary_intent: &str) -> bool {
    matches!(kind, IntentKind::Greeting)
        || qa_meta_style_primary(primary_intent)
        || qa_explain_style_primary(primary_intent)
}

/// 按 `primary_intent` 选择门控直接回复正文（`qa.meta*` / `qa.explain` 等为占位；门控开启时常改走主模型）。
#[must_use]
pub fn qa_direct_reply_for_primary(primary_intent: &str) -> String {
    if qa_readonly_style_primary(primary_intent) {
        return "我会只读查看你仓库里的相关文件与目录来回答，不会主动改代码；若需要我修改或运行命令，请直接说明。".to_string();
    }
    if qa_meta_style_primary(primary_intent) {
        return QA_META_REPLY.to_string();
    }
    QA_REPLY.to_string()
}

#[must_use]
pub fn greeting_reply_message() -> &'static str {
    GREETING_REPLY
}

#[must_use]
pub fn ambiguous_ask_message() -> &'static str {
    AMBIGUOUS_ASK
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
        EXECUTE_CONFIRM, IntentKind, intent_reply_delegates_to_main_model,
        is_explicit_execute_confirmation, is_waiting_execute_confirmation_prompt,
        qa_direct_reply_for_primary, qa_readonly_style_primary,
    };

    #[test]
    fn waiting_confirm_prompt_matches_canned_and_paraphrase() {
        assert!(is_waiting_execute_confirmation_prompt(EXECUTE_CONFIRM));
        assert!(is_waiting_execute_confirmation_prompt(
            "请确认是否开始执行上述改动。"
        ));
        assert!(!is_waiting_execute_confirmation_prompt("随便聊聊"));
    }

    #[test]
    fn explicit_confirm_keywords() {
        assert!(is_explicit_execute_confirmation("直接开始执行"));
        assert!(is_explicit_execute_confirmation("请继续执行吧"));
        assert!(!is_explicit_execute_confirmation("继续看看文档"));
    }

    #[test]
    fn qa_meta_and_readonly_helpers() {
        let s = qa_direct_reply_for_primary("qa.meta");
        assert!(s.contains("CrabMate"));
        let s2 = qa_direct_reply_for_primary("qa.meta.capability");
        assert_eq!(s, s2);
        assert!(qa_readonly_style_primary("qa.readonly"));
        assert!(qa_readonly_style_primary("qa.codebase"));
        assert!(!qa_readonly_style_primary("qa.codebase.explore"));
    }

    #[test]
    fn intent_reply_delegates_to_main_model_covers_greeting_meta_explain() {
        assert!(intent_reply_delegates_to_main_model(
            IntentKind::Greeting,
            "meta.greeting"
        ));
        assert!(intent_reply_delegates_to_main_model(
            IntentKind::Qa,
            "qa.meta"
        ));
        assert!(intent_reply_delegates_to_main_model(
            IntentKind::Qa,
            "qa.explain"
        ));
        assert!(!intent_reply_delegates_to_main_model(
            IntentKind::Execute,
            "execute.code_change"
        ));
        assert!(!intent_reply_delegates_to_main_model(
            IntentKind::Qa,
            "qa.readonly"
        ));
    }
}
