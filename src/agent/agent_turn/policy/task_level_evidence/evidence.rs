//! 完成判定的**第 ① 层：结构化证据采集**（#1387 S0.5）。
//!
//! 把多次工具调用归一为带稳定 id 的**结构化事实**，每条只陈述「发生了什么」，
//! **不清言任务是否完成**——那是第 ② 层（LLM 语义判定）的职责。
//!
//! ## 为什么需要这一层
//!
//! 旧实现把「事实采集」与「完成判定」压在同一批关键词判断里，导致两个方向的错误：
//!
//! 1. **判定依赖工具名**：`generic_tool_is_build_or_test_success` 前置
//!    `tool_name == run_command`，于是用规范的 `cargo_test` 反而判不出——越规范越失效；
//! 2. **强度不可区分**：`run_command` 跑 `make` 只能靠 `built target` 等文本特征（弱证据），
//!    而 `cargo_test` 带结构化 `exit_code`（强证据），旧实现把两者同等对待。
//!
//! 本层把这两件事拆开：**证据来自哪个工具、有多硬**，与「够不够算完成」互不耦合。
//!
//! ## 口径
//!
//! - [`EvidenceKind::Verify`]：验证性执行。**专用工具**（`cargo_test` 等）走结构化退出码，
//!   `run_command` 则必须靠输出特征识别，因而强度天然更弱。
//! - [`EvidenceKind::Write`]：改动工作区文件。
//! - [`EvidenceKind::Other`]：其余（含只读探查）。
//!
//! `Verify` / `Write` 的名单**不是** `cm_config::tool_write_effect` 的副本：那份是「有副作用」
//! 的宽口径（含 `http_request` / `reminder` 等不改工作区的工具），本层要的是两个**更窄**的
//! 语义——「改过工作区」与「验证过结果」。两处口径不同，不应合并。

use crate::cm_names as tool_names;
use crate::cm_tools::tool_result::{normalize_tool_message_content, parse_tool_output};
use crate::cm_types::{Message, message_content_as_str};

/// 一次工具调用贡献的证据种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// 验证性执行（构建、测试、静态检查）。
    Verify,
    /// 改动工作区文件。
    Write,
    /// 其余（只读探查、网络、调度等）。
    Other,
}

impl EvidenceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::Write => "write",
            Self::Other => "other",
        }
    }
}

/// 证据强度：决定「能否据此提前停轮」。
///
/// 这是本层存在的核心理由——旧的布尔 `build_or_test_success` 无法表达
/// 「同一件事，一次是结构化退出码、一次是文本猜测」这个差距。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceStrength {
    /// 结构化事实：工具自身的 `exit_code` / 信封 `ok`，不需要文本推断。
    Strong,
    /// 文本特征推断：从输出里认出「built target」「test result: ok」之类标记。
    Weak,
}

impl EvidenceStrength {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Weak => "weak",
        }
    }
}

/// 一条结构化证据。`id` 稳定且有序（`ev_1`…），供第 ② 层判定时回引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: EvidenceKind,
    /// `Write` / `Other` 也填：写盘成功同样是结构化事实。
    pub strength: EvidenceStrength,
    pub tool: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    /// 截断后的输出片段，供判定层与日志引用。
    pub detail: String,
}

/// 输出片段保留的字符上限（避免证据本身把上下文撑爆）。
const DETAIL_MAX_CHARS: usize = 200;

/// 一次回合采集到的全部证据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvidenceSet {
    items: Vec<EvidenceItem>,
}

impl EvidenceSet {
    /// 从消息历史采集证据。
    ///
    /// 只认 `role == "tool"` 的消息：证据必须来自工具执行，`assistant` 自述**不入账**
    /// （旧实现的 `assistant_completion_claim` 正是把自述当证据的地方）。
    #[must_use]
    pub fn collect(messages: &[Message]) -> Self {
        let mut items = Vec::new();
        for m in messages {
            if m.role != "tool" {
                continue;
            }
            let Some(raw) = message_content_as_str(&m.content) else {
                continue;
            };
            let name = m.name.as_deref().unwrap_or("");
            if let Some(item) = evidence_from_tool_message(name, raw, items.len()) {
                items.push(item);
            }
        }
        Self { items }
    }

    #[must_use]
    pub fn items(&self) -> &[EvidenceItem] {
        &self.items
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否存在**强**验证证据（专用工具 + 结构化退出码）。
    #[must_use]
    pub fn has_strong_verify_success(&self) -> bool {
        self.items.iter().any(|i| {
            i.kind == EvidenceKind::Verify
                && i.strength == EvidenceStrength::Strong
                && i.ok
        })
    }

    /// 是否存在任一验证证据（含弱）。
    #[must_use]
    pub fn has_any_verify_success(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.kind == EvidenceKind::Verify && i.ok)
    }

    /// 是否改过工作区。
    #[must_use]
    pub fn has_write_success(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.kind == EvidenceKind::Write && i.ok)
    }

    /// 是否有任何成功证据（含只读探查）。
    #[must_use]
    pub fn has_any_success(&self) -> bool {
        self.items.iter().any(|i| i.ok)
    }
}

/// 验证性执行的**专用**工具：自身输出带结构化退出码，无需文本推断。
///
/// 名单全部取自 [`crate::cm_names`]（内建工具名单一真源，勿写字面量）。
fn is_dedicated_verify_tool(name: &str) -> bool {
    matches!(
        name,
        tool_names::CARGO_TEST | tool_names::CARGO_CHECK | tool_names::CARGO_CLIPPY
            | tool_names::CARGO_RUN
            | tool_names::PYTEST_RUN
    )
}

/// 改动工作区文件的工具（**窄口径**：只算真正写盘的，不含执行类）。
///
/// 与 `cm_config::tool_write_effect` 的区别是刻意的：那份把 `run_command` / `cargo_test` /
/// `http_request` / `reminder` 都算「有副作用」，而本层只回答「工作区被改过吗」。
fn is_workspace_write_tool(name: &str) -> bool {
    matches!(
        name,
        tool_names::CREATE_FILE
            | tool_names::MODIFY_FILE
            | tool_names::APPEND_FILE
            | tool_names::APPLY_PATCH
            | tool_names::SEARCH_REPLACE
            | tool_names::COPY_FILE
            | tool_names::MOVE_FILE
            | tool_names::DELETE_FILES
            | tool_names::DELETE_DIR
            | tool_names::CREATE_DIR
    )
}

/// `run_command` 输出里的构建 / 测试特征。命中即视为验证性执行，但强度只能是 `Weak`。
fn run_command_looks_like_verify(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "cargo test",
        "cargo check",
        "cargo clippy",
        "cargo build",
        "cmake",
        "make",
        "pytest",
        "built target",
        "linking cxx executable",
        "test result: ok",
        "tests passed",
        "build succeeded",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// 按 Unicode scalar 截断并压成单行，避免证据正文塞满上下文。
fn summarize_output(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = one_line.trim();
    if trimmed.chars().count() <= DETAIL_MAX_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(DETAIL_MAX_CHARS).collect();
    out.push('…');
    out
}

/// 单条工具消息 → 一条证据；无法归类的工具也记（供判定层看到「做过什么」）。
fn evidence_from_tool_message(
    fallback_name: &str,
    raw: &str,
    index: usize,
) -> Option<EvidenceItem> {
    let env = normalize_tool_message_content(raw);
    let (tool, output, ok, exit_code) = match env.as_ref() {
        Some(e) => (
            e.name.as_str(),
            e.output.as_str(),
            e.ok || e.exit_code == Some(0),
            e.exit_code,
        ),
        None => {
            // 无信封的历史行：只能靠启发式解析，强度必然是弱证据。
            let parsed = parse_tool_output(fallback_name, raw);
            (
                fallback_name,
                raw,
                parsed.ok || parsed.exit_code == Some(0),
                parsed.exit_code,
            )
        }
    };
    if tool.trim().is_empty() {
        return None;
    }
    let lower = format!("{output}\n{raw}").to_lowercase();
    let (kind, strength) = if is_dedicated_verify_tool(tool) {
        // 专用工具：有结构化退出码才算强证据。
        let strength = if exit_code.is_some() {
            EvidenceStrength::Strong
        } else {
            EvidenceStrength::Weak
        };
        (EvidenceKind::Verify, strength)
    } else if is_workspace_write_tool(tool) {
        (EvidenceKind::Write, EvidenceStrength::Strong)
    } else if tool == tool_names::RUN_COMMAND && run_command_looks_like_verify(&lower) {
        (EvidenceKind::Verify, EvidenceStrength::Weak)
    } else {
        (EvidenceKind::Other, EvidenceStrength::Weak)
    };
    Some(EvidenceItem {
        id: format!("ev_{}", index + 1),
        kind,
        strength,
        tool: tool.to_string(),
        ok,
        exit_code,
        detail: summarize_output(output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{msg, tool_env};

    fn user(text: &str) -> Message {
        msg("user", text)
    }

    #[test]
    fn assistant_claim_is_never_evidence() {
        // 只有 assistant 自述、没有任何工具消息 → 零证据。
        let messages = vec![
            user("把常量改成 42"),
            msg("assistant", "已完成修改，任务完成。"),
        ];
        assert!(EvidenceSet::collect(&messages).is_empty());
    }

    #[test]
    fn dedicated_cargo_test_is_strong_verify() {
        // 旧的 `generic_tool_is_build_or_test_success` 会因 tool != run_command 漏掉它。
        let messages = vec![
            user("跑通全部测试"),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 100 passed; 0 failed",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        assert!(ev.has_strong_verify_success(), "专用 cargo_test 应为强验证证据");
        let item = &ev.items()[0];
        assert_eq!(item.kind, EvidenceKind::Verify);
        assert_eq!(item.strength, EvidenceStrength::Strong);
        assert_eq!(item.exit_code, Some(0));
        assert_eq!(item.id, "ev_1");
    }

    #[test]
    fn run_command_verify_is_only_weak() {
        let messages = vec![
            user("编译 hpcg"),
            tool_env(
                tool_names::RUN_COMMAND,
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hpcg",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        assert!(ev.has_any_verify_success());
        assert!(
            !ev.has_strong_verify_success(),
            "run_command 靠文本特征，只能是弱证据"
        );
    }

    #[test]
    fn write_tool_is_structured_write_evidence() {
        let messages = vec![
            user("改常量"),
            tool_env(tool_names::MODIFY_FILE, "修改 src/x.rs", "已修改文件: src/x.rs"),
        ];
        let ev = EvidenceSet::collect(&messages);
        assert!(ev.has_write_success());
        assert!(!ev.has_any_verify_success(), "写盘不是验证证据");
    }

    #[test]
    fn failed_verify_is_not_success() {
        let messages = vec![
            user("跑测试"),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：101\n标准错误：\ntest result: FAILED. 3 passed; 1 failed",
            ),
        ];
        let ev = EvidenceSet::collect(&messages);
        assert!(!ev.has_strong_verify_success());
        assert!(!ev.has_any_verify_success());
        assert_eq!(ev.items()[0].exit_code, Some(101));
    }

    #[test]
    fn ids_are_sequential_and_stable() {
        let messages = vec![
            user("分析"),
            tool_env(tool_names::LIST_TREE, "list tree: .", "list tree: ."),
            tool_env(tool_names::READ_FILE, "read file: a", "read file: a"),
        ];
        let ev = EvidenceSet::collect(&messages);
        let ids: Vec<&str> = ev.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["ev_1", "ev_2"]);
    }

    #[test]
    fn detail_is_bounded_and_single_line() {
        let long = format!("行1\n{}", "x".repeat(1000));
        let messages = vec![
            user("x"),
            tool_env(tool_names::CARGO_TEST, "cargo test", &long),
        ];
        let ev = EvidenceSet::collect(&messages);
        let d = &ev.items()[0].detail;
        assert!(!d.contains('\n'), "证据正文压成单行");
        assert!(d.chars().count() <= DETAIL_MAX_CHARS + 1);
    }

    #[test]
    fn verify_tool_without_exit_code_degrades_to_weak() {
        // 无信封的历史行 → 解析不出退出码 → 强证据不成立。
        let messages = vec![
            user("跑测试"),
            msg("tool", "cargo test 完成了"),
        ];
        let ev = EvidenceSet::collect(&messages);
        assert!(!ev.has_strong_verify_success());
    }
}
