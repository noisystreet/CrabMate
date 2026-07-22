//! L2 单 Agent 外循环：编译/构建类任务在「只说不做」时的轻量门控。

use crate::agent::plan_artifact::plan_step_description_implies_build_execution;
use crate::types::{Message, last_real_user_message_index, message_content_as_str};

/// 从 tool 消息正文检查是否包含构建/编译进展信号。
fn tool_message_indicates_build_progress(m: &Message) -> bool {
    let Some(content) = message_content_as_str(&m.content) else {
        return false;
    };
    let lowered = content.to_lowercase();
    // 除 exit code 0 外，还检查编译/构建/测试类标志性关键词
    if lowered.contains("exit code 0") || lowered.contains("exit code: 0") {
        return true;
    }
    // 构建成功信号
    if lowered.contains("build successful")
        || lowered.contains("compilation successful")
        || lowered.contains("built successfully")
    {
        return true;
    }
    // 测试通过信号
    if lowered.contains("test result: ok") || lowered.contains("test passed") {
        return true;
    }
    false
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

fn assistant_text_promises_build_or_compile(text: &str) -> bool {
    let lower = text.to_lowercase();
    const MARKERS: &[&str] = &[
        "编译",
        "构建",
        "make",
        "cmake",
        "build",
        "compile",
        "makefile",
        "install",
        "运行 make",
        "执行 make",
        "开始编译",
        "进行编译",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
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
        prefix = crabmate_display_rules::OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
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
        Message {
            role: "tool".to_string(),
            content: Some(body.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.to_string()),
            tool_call_id: Some("t1".to_string()),
        }
    }

    #[test]
    fn detects_build_task_and_idle_assistant() {
        assert!(outer_loop_task_implies_build_execution("编译 hpcg"));
        assert!(outer_loop_assistant_is_build_idle_without_tools(
            &assistant("接下来将查看 Makefile 并执行 make 编译")
        ));
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
