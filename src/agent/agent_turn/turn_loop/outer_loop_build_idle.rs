//! L2 单 Agent 外循环：编译/构建类任务在「只说不做」时的轻量门控。

use crate::agent::agent_turn::policy::task_level_evidence::command_line_looks_like_verify;
use crate::agent::plan_artifact::plan_step_description_implies_build_execution;
use crate::cm_agent::text_markers::{marker_matches, text_matches_any};
use crate::cm_display_rules::OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX;
use crate::cm_names as tool_names;
use crate::cm_tools::tool_result::normalize_tool_message_content;
use crate::types::{Message, last_real_user_message_index, message_content_as_str};

/// 此工具结果是否指示了「构建类进展」（非简单退出码 0 检测）。
///
/// #1422 A5：`run_command` 不再读**输出正文**判构建，改读结构化字段
/// `structured_payload.invocation`（与 `evidence.rs` 同源、同一个 `command_line_looks_like_verify`）。
/// 读正文两个方向都会错：`make sure` 这类自然语言会误命中，而 `ninja` / `cargo nextest`
/// 的真实输出里根本没有 `built target`。
pub(crate) fn tool_message_indicates_build_progress(m: &Message) -> bool {
    let Some(name) = m.name.as_deref() else {
        return false;
    };
    if matches!(
        name,
        tool_names::CARGO_TEST
            | tool_names::CARGO_CHECK
            | tool_names::CARGO_CLIPPY
            | tool_names::PYTEST_RUN
    ) {
        return true;
    }
    if name != tool_names::RUN_COMMAND {
        return false;
    }
    let Some(raw) = message_content_as_str(&m.content) else {
        return false;
    };
    if let Some(invocation) = run_command_invocation(raw) {
        return command_line_looks_like_verify(&invocation);
    }
    // 没有信封的旧消息只能看输出特征——兜底路径，不覆盖正常链路。
    build_markers_in_output(&raw.to_lowercase())
}

/// 取 `crabmate_tool.structured_payload.invocation`（完整命令行）。
///
/// 空串等同「没有 invocation」：与 `evidence.rs::run_command_is_verify` 同口径。
/// 少了这道过滤，信封里字段存在但解析失败的消息会走「结构化判定为假」分支而**不**回退
/// 到输出特征，表现为这类旧消息永远「无构建进展」（#1422 review）。
fn run_command_invocation(raw: &str) -> Option<String> {
    let env = normalize_tool_message_content(raw)?;
    env.structured_payload
        .as_ref()?
        .get("invocation")?
        .as_str()
        .map(str::to_string)
        .filter(|invocation| !invocation.trim().is_empty())
}

/// 输出里的构建 / 测试特征——**仅用于没有 `invocation` 的旧消息**。
///
/// 词表只留多词特征：`make` / `cmake` 这种短词在正文里必然误命中（`make sure`），
/// 而它们现在由结构化命令判定覆盖，兜底路径不需要再猜（#1422 A5）。
fn build_markers_in_output(text_lower: &str) -> bool {
    const BUILD_MARKERS: &[&str] = &[
        "make ",
        "make\n",
        "cmake --build",
        "cargo build",
        "cargo test",
        "cargo run",
        "ninja ",
        "meson compile",
        "meson test",
        "go build",
        "go test",
        "npm run build",
        "npm test",
        "ctest ",
        "bazel build",
        "buck build",
    ];
    text_matches_any(text_lower, BUILD_MARKERS)
}

/// 连续无构建进展的终答轮次达到该值后注入硬反馈（含当前轮）。
pub(crate) const OUTER_LOOP_BUILD_IDLE_STREAK_THRESHOLD: u32 = 2;

/// 最多注入次数，避免与外层安全上限死磕。
pub(crate) const OUTER_LOOP_BUILD_IDLE_FEEDBACK_MAX: u32 = 4;

pub(crate) fn outer_loop_task_implies_build_execution(task: &str) -> bool {
    plan_step_description_implies_build_execution(task)
}

fn last_user_message_index(messages: &[Message]) -> Option<usize> {
    last_real_user_message_index(messages, false)
}

pub(crate) fn outer_loop_window_has_build_progress_since_last_user(messages: &[Message]) -> bool {
    let Some(user_idx) = last_user_message_index(messages) else {
        return false;
    };
    messages[user_idx.saturating_add(1)..].iter().any(|m| {
        if m.role != "tool" {
            return false;
        }
        tool_message_indicates_build_progress(m)
    })
}

/// 助手终答是否在**承诺**编译 / 构建却没给工具调用。
///
/// #1422 A4：原来的表里有裸 `make`，于是 `make sure …` / `make it …` 全被算成承诺，
/// 凭空注入「只说不做」纠偏。裸 `make` 在自然语言里没有任何词边界可依
/// （`make sure` 是完整的两个词），所以它不是「加边界」能救的，只能拆成双条件。
fn assistant_text_promises_build_or_compile(text: &str) -> bool {
    let lower = text.to_lowercase();
    /// 构建动作词：正文里出现即在谈构建，无需再要意图词。
    /// 刻意**不含裸 `make`** —— 它走下面的双条件。
    const BUILD_ACTION_MARKERS: &[&str] = &[
        "编译",
        "构建",
        "makefile",
        "cmake",
        "build",
        "compile",
        "install",
        "运行 make",
        "执行 make",
        "开始编译",
        "进行编译",
    ];
    /// 「我要去做」的信号。只在与 `make` 同现时才有意义。
    ///
    /// 刻意不含 `先` / `现在` 这类几乎每段正文都会出现的词：它们和 `make` 同现时
    /// 会把「先说明一下，`make` 的用法是…」判成承诺（#1422 review）。
    const INTENT_MARKERS: &[&str] = &[
        "将",
        "马上",
        "开始",
        "准备",
        "接下来",
        "执行",
        "运行",
        "will",
        "let me",
        "going to",
        "about to",
    ];
    text_matches_any(&lower, BUILD_ACTION_MARKERS)
        || (marker_matches(&lower, "make") && text_matches_any(&lower, INTENT_MARKERS))
}

pub(crate) fn outer_loop_assistant_is_build_idle_without_tools(msg: &Message) -> bool {
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    message_content_as_str(&msg.content).is_some_and(assistant_text_promises_build_or_compile)
}

pub(crate) fn outer_loop_build_idle_feedback_body(streak: u32) -> String {
    format!(
        "{prefix}用户目标涉及编译/构建，但已连续 {streak} 轮助手回复未产生任何构建类工具结果（如 `run_command` 的 make/cmake/cargo build 等）。\
         **禁止**再用自然语言承诺「将要编译/读取 Makefile」；本轮必须通过 `tool_calls` 实际执行构建或读取构建说明（`read_file` / `read_dir`），\
         且若需解压源码包请优先 `archive_unpack` 到 `output_dir=\".\"`（勿自创嵌套目录名）。",
        prefix = OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
    )
}

/// 若应注入纠偏 user 并继续外循环，返回 `Some(feedback)`。
pub(crate) fn outer_loop_build_idle_feedback_if_needed(
    task: &str,
    messages: &[Message],
    assistant: &Message,
    streak: u32,
    feedback_injected_count: u32,
) -> Option<String> {
    if !outer_loop_task_implies_build_execution(task) {
        return None;
    }
    if outer_loop_window_has_build_progress_since_last_user(messages) {
        return None;
    }
    if !outer_loop_assistant_is_build_idle_without_tools(assistant) {
        return None;
    }
    if streak < OUTER_LOOP_BUILD_IDLE_STREAK_THRESHOLD {
        return None;
    }
    if feedback_injected_count >= OUTER_LOOP_BUILD_IDLE_FEEDBACK_MAX {
        return None;
    }
    Some(outer_loop_build_idle_feedback_body(streak))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    fn user(text: &str) -> Message {
        Message::user_only(text.to_string())
    }

    fn assistant(text: &str) -> Message {
        Message::assistant_only(text.to_string())
    }

    fn tool(name: &str, body: &str) -> Message {
        Message::tool_result_named("t1".to_string(), name.to_string(), body)
    }

    /// 信封版工具消息：`tool_env` 不设 `Message.name`，但本模块的判定要先看工具名。
    fn named_env(name: &str, summary: &str, output: &str) -> Message {
        let mut m = crate::test_fixtures::tool_env(name, summary, output);
        m.name = Some(name.to_string());
        m
    }

    #[test]
    fn detects_build_task_and_idle_assistant() {
        assert!(outer_loop_task_implies_build_execution("编译 hpcg"));
        assert!(outer_loop_assistant_is_build_idle_without_tools(
            &assistant("接下来将查看 Makefile 并执行 make 编译")
        ));
    }

    /// #1422 A5：`run_command` 的构建进展只看结构化 `invocation`，不看输出正文。
    #[test]
    fn build_progress_uses_structured_invocation_not_output_text() {
        // 结构化字段说这是 make：输出里一个构建词都没有，仍算进展。
        let make_env = named_env(
            tool_names::RUN_COMMAND,
            "make",
            "命令：make -j8\n退出码：0\n标准输出：\n(无输出)",
        );
        assert!(tool_message_indicates_build_progress(&make_env));

        // 结构化字段说这是 cat：输出正文里出现 `make ` 也不算构建进展。
        let cat_env = named_env(
            tool_names::RUN_COMMAND,
            "cat README",
            "命令：cat README\n退出码：0\n标准输出：\nmake sure to run make\n",
        );
        assert!(
            !tool_message_indicates_build_progress(&cat_env),
            "输出正文不该再参与构建进展判定"
        );
    }

    #[test]
    fn build_progress_from_make_tool_skips_feedback() {
        let msgs = vec![
            user("编译 hpcg"),
            assistant("先 make"),
            tool(
                "run_command",
                "命令：make arch=GCC_OMP\n退出码：2\n标准错误：\nerror",
            ),
        ];
        assert!(outer_loop_window_has_build_progress_since_last_user(&msgs));
        assert!(
            outer_loop_build_idle_feedback_if_needed(
                "编译 hpcg",
                &msgs,
                &assistant("将再次编译"),
                3,
                0
            )
            .is_none()
        );
    }

    /// 信封里 `invocation` 为空时**必须**回落到输出特征，否则这类消息永远「无进展」
    /// 而反复注入纠偏（#1422 review）。
    #[test]
    fn empty_invocation_falls_back_to_output_text() {
        let env = crate::test_fixtures::tool_env(tool_names::RUN_COMMAND, "", "");
        let raw = message_content_as_str(&env.content).unwrap_or_default();
        assert!(
            run_command_invocation(raw).is_none(),
            "空 invocation 等同没有 invocation"
        );
    }

    /// #1422 A4：裸 `make` 不再单独算「承诺构建」。
    #[test]
    fn make_sure_is_not_a_build_promise() {
        assert!(!outer_loop_assistant_is_build_idle_without_tools(
            &assistant("Make sure the tests still pass")
        ));
        assert!(!outer_loop_assistant_is_build_idle_without_tools(
            &assistant("I'll make the API consistent")
        ));
        // 真承诺仍然命中：动作词，或「意图词 + make」。
        assert!(outer_loop_assistant_is_build_idle_without_tools(&assistant(
            "接下来执行 make 构建"
        )));
        assert!(outer_loop_assistant_is_build_idle_without_tools(&assistant(
            "接下来运行 make"
        )));
    }

    #[test]
    fn injects_after_streak_threshold() {
        let msgs = vec![
            user("编译 hpcg"),
            assistant("将读取 Makefile"),
            assistant("准备 make 编译"),
        ];
        let fb = outer_loop_build_idle_feedback_if_needed(
            "编译 hpcg",
            &msgs,
            &assistant("马上开始 make"),
            2,
            0,
        );
        assert!(fb.is_some());
        assert!(fb.unwrap().contains("编排纠偏"));
    }
}
