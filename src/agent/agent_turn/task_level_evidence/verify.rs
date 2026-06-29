//! 任务级执行证据核对（写源码 / 编译 / 运行）。

use crate::agent::hierarchy::goal_verifier;
use crate::agent::hierarchy::task::{ArtifactKind, BuildArtifactKind, TaskResult};
use crate::types::{Message, message_content_as_str};
use std::collections::HashMap;

use super::common::{
    combined_output_error, cpp_source_path, expected_output_hints_for_results,
    is_program_build_run_request,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoalCompletionEvidenceCheck {
    Satisfied,
    Missing(String),
    NotApplicable,
}

struct GoalEvidenceDetector {
    applies: fn(&str) -> bool,
    check_messages: fn(&str, &[Message]) -> GoalCompletionEvidenceCheck,
}

const GOAL_EVIDENCE_DETECTORS: &[GoalEvidenceDetector] = &[GoalEvidenceDetector {
    applies: is_program_build_run_request,
    check_messages: check_program_build_run_messages,
}];

#[derive(Debug, Default)]
struct ProgramBuildRunEvidence {
    wrote_source: bool,
    compiled: bool,
    ran_program: bool,
}

#[derive(Debug, Default)]
struct GenericTaskIntent {
    build_or_test: bool,
    write_change: bool,
    readonly_analysis: bool,
}

#[derive(Debug, Default)]
struct GenericToolEvidence {
    any_success: bool,
    build_or_test_success: bool,
    write_success: bool,
    readonly_success: bool,
}

fn artifact_evidence_flags(r: &TaskResult) -> (bool, bool) {
    let mut wrote_source = false;
    let mut compiled = false;
    for a in &r.artifacts {
        match a.kind {
            ArtifactKind::File if cpp_source_path(a.path.as_deref()) => {
                wrote_source = true;
            }
            ArtifactKind::BuildArtifact(kind) => match kind {
                BuildArtifactKind::SourceFile => wrote_source = true,
                BuildArtifactKind::ObjectFile => compiled = true,
                _ => {}
            },
            _ => {}
        }
    }
    (wrote_source, compiled)
}

fn combined_text_build_flags(combined_lower: &str) -> (bool, bool) {
    const WRITE_HINTS: &[&str] = &[
        "create_file",
        "已创建文件",
        "created file",
        "write_file",
        "apply_patch",
        ".cpp",
    ];
    const COMPILE_HINTS: &[&str] = &["g++", "clang++", "编译", "cmake", "make", "build"];
    let wrote_source = WRITE_HINTS.iter().any(|hint| combined_lower.contains(hint));
    let compiled = COMPILE_HINTS
        .iter()
        .any(|hint| combined_lower.contains(hint));
    (wrote_source, compiled)
}

fn ran_program_from_tools_and_output(
    r: &TaskResult,
    combined_full: &str,
    expected_outputs: &[String],
) -> bool {
    r.tools_invoked.iter().any(|n| n == "run_executable")
        || (r.tools_invoked.iter().any(|n| n == "run_command")
            && goal_verifier::run_command_invocation_matches_expected_output(
                combined_full,
                expected_outputs,
            ))
}

fn per_result_verify_flags(
    r: &TaskResult,
    combined_lower: &str,
    combined_full: &str,
    expected_outputs: &[String],
) -> (bool, bool, bool) {
    let (art_write, art_compile) = artifact_evidence_flags(r);
    let (text_write, text_compile) = combined_text_build_flags(combined_lower);
    let ran_program = ran_program_from_tools_and_output(r, combined_full, expected_outputs);
    (
        art_write || text_write,
        art_compile || text_compile,
        ran_program,
    )
}

fn message_history_build_flags(history_lower: &str) -> (bool, bool, bool) {
    let wrote_source = history_lower.contains(".cpp")
        && (history_lower.contains("create file")
            || history_lower.contains("创建文件")
            || history_lower.contains("已创建文件")
            || history_lower.contains("write_file")
            || history_lower.contains("apply_patch"));
    let compiled = history_lower.contains("cmake --build")
        || history_lower.contains("built target")
        || history_lower.contains("linking cxx executable")
        || (history_lower.contains("configuring done")
            && history_lower.contains("generating done"));
    let ran_program = (history_lower.contains("退出码：0")
        || history_lower.contains("exit code: 0"))
        && (history_lower.contains("build/") || history_lower.contains("./build/"))
        && (history_lower.contains("标准输出") || history_lower.contains("stdout"));
    (wrote_source, compiled, ran_program)
}

fn normalized_tool_text(env: &crate::tool_result::NormalizedToolEnvelope) -> String {
    let structured = env
        .structured_payload
        .as_ref()
        .map(serde_json::Value::to_string)
        .unwrap_or_default();
    format!(
        "{}\n{}\n{}\n{}",
        env.name, env.summary, env.output, structured
    )
}

fn run_command_invocation_from_env(env: &crate::tool_result::NormalizedToolEnvelope) -> String {
    let payload_invocation = env
        .structured_payload
        .as_ref()
        .and_then(|v| v.get("invocation"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if !payload_invocation.trim().is_empty() {
        return payload_invocation.to_string();
    }
    env.summary.clone()
}

fn structured_stdout_nonempty(env: &crate::tool_result::NormalizedToolEnvelope) -> bool {
    env.structured_payload
        .as_ref()
        .and_then(|v| v.get("stdout_nonempty"))
        .and_then(|x| x.as_bool())
        == Some(true)
}

fn looks_like_build_artifact_run(invocation_lower: &str, lower_text: &str) -> bool {
    const RUN_HINTS: &[&str] = &[
        "./build/",
        " build/",
        "$ build/",
        "$ ./build/",
        "命令：./build/",
        "命令：build/",
    ];
    RUN_HINTS
        .iter()
        .any(|hint| invocation_lower.contains(hint) || lower_text.contains(hint))
}

fn tool_exit_ok(env: &crate::tool_result::NormalizedToolEnvelope) -> bool {
    env.ok || env.exit_code == Some(0)
}

fn update_program_evidence_from_tool_env(
    evidence: &mut ProgramBuildRunEvidence,
    env: &crate::tool_result::NormalizedToolEnvelope,
) {
    let text = normalized_tool_text(env);
    let lower = text.to_lowercase();
    if matches!(
        env.name.as_str(),
        "create_file" | "write_file" | "edit_file" | "apply_patch"
    ) && lower.contains(".cpp")
    {
        evidence.wrote_source = true;
    }
    if env.name == "run_command" && tool_exit_ok(env) {
        let invocation = run_command_invocation_from_env(env).to_lowercase();
        if invocation.contains("cmake --build")
            || lower.contains("cmake --build")
            || lower.contains("built target")
            || lower.contains("linking cxx executable")
        {
            evidence.compiled = true;
        }
        let parsed =
            crate::tool_result::parse_legacy_output(env.name.as_str(), env.output.as_str());
        let stdout_nonempty = structured_stdout_nonempty(env) || !parsed.stdout.trim().is_empty();
        if looks_like_build_artifact_run(&invocation, &lower)
            && tool_exit_ok(env)
            && stdout_nonempty
        {
            evidence.ran_program = true;
        }
    }
}

fn update_program_evidence_from_legacy_text(evidence: &mut ProgramBuildRunEvidence, raw: &str) {
    let lower = raw.to_lowercase();
    let (wrote_source, compiled, ran_program) = message_history_build_flags(&lower);
    evidence.wrote_source |= wrote_source;
    evidence.compiled |= compiled;
    evidence.ran_program |= ran_program;
}

fn program_build_run_evidence_from_messages(messages: &[Message]) -> ProgramBuildRunEvidence {
    let mut evidence = ProgramBuildRunEvidence::default();
    for m in messages {
        let Some(raw) = message_content_as_str(&m.content) else {
            continue;
        };
        if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
            update_program_evidence_from_tool_env(&mut evidence, &env);
        } else {
            update_program_evidence_from_legacy_text(&mut evidence, raw);
        }
    }
    evidence
}

fn program_build_run_missing_from_messages(messages: &[Message]) -> Option<String> {
    let evidence = program_build_run_evidence_from_messages(messages);

    let mut missing = Vec::new();
    if !evidence.wrote_source {
        missing.push("write_source");
    }
    if !evidence.compiled {
        missing.push("compile");
    }
    if !evidence.ran_program {
        missing.push("run");
    }
    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "missing: {}; 当前消息历史尚未同时证明写源码(.cpp)+编译+运行成功",
            missing.join(",")
        ))
    }
}

fn check_program_build_run_messages(
    _task: &str,
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    match program_build_run_missing_from_messages(messages) {
        None => GoalCompletionEvidenceCheck::Satisfied,
        Some(reason) => GoalCompletionEvidenceCheck::Missing(reason),
    }
}

fn tool_message_has_success_evidence(raw: &str) -> bool {
    if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
        return env.ok || env.exit_code == Some(0);
    }
    let parsed = crate::tool_result::parse_legacy_output("generic_tool", raw);
    if parsed.ok || parsed.exit_code == Some(0) {
        return true;
    }
    let lower = raw.to_lowercase();
    lower.contains("file exists")
        || lower.contains("已创建文件")
        || lower.contains("已解压")
        || lower.contains("成功")
        || lower.contains("completed")
        || lower.contains("succeeded")
}

/// 用户任务是否像编译/构建/测试类（供外循环门控与完成证据抑制共用）。
pub(crate) fn generic_task_intent_implies_build_or_test(task: &str) -> bool {
    generic_task_intent_from_task(task).build_or_test
}

fn messages_slice_since_last_user(messages: &[Message]) -> Option<&[Message]> {
    crate::types::messages_slice_since_last_real_user(messages, false)
}

/// L2 外循环与分阶段滚动视界：以**最新一条用户消息**为目标，且仅在自该 user 起的消息窗口内核对完成证据。
///
/// 勿用会话首条 user 或分阶段「不变层」历史锚点（[`crate::types::first_real_user_task_content`]），
/// 否则多轮对话会把上一任务（如「分析目录」）误判为当前目标（如「编译 hpcg」）已完成。
pub(crate) fn check_active_user_goal_completion_evidence(
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    let Some(task) = crate::agent::plan_optimizer::staged_plan_trigger_user_content(messages)
    else {
        return GoalCompletionEvidenceCheck::NotApplicable;
    };
    let Some(window) = messages_slice_since_last_user(messages) else {
        return GoalCompletionEvidenceCheck::NotApplicable;
    };
    check_goal_completion_evidence_from_messages(task, window)
}

fn generic_task_intent_from_task(task: &str) -> GenericTaskIntent {
    let t = task.to_lowercase();
    let build_or_test = [
        "编译",
        "构建",
        "测试",
        "运行测试",
        "build",
        "compile",
        "make",
        "cmake",
        "test",
        "pytest",
        "cargo test",
        "cargo check",
        "clippy",
    ]
    .iter()
    .any(|k| t.contains(k));
    let write_change = [
        "编写",
        "实现",
        "修改",
        "创建",
        "新增",
        "删除",
        "修复",
        "write",
        "implement",
        "modify",
        "create",
        "add",
        "delete",
        "fix",
    ]
    .iter()
    .any(|k| t.contains(k));
    let readonly_analysis = [
        "分析", "查看", "看看", "列出", "梳理", "介绍", "read", "inspect", "analyze", "list",
        "show", "explain",
    ]
    .iter()
    .any(|k| t.contains(k));
    GenericTaskIntent {
        build_or_test,
        write_change,
        readonly_analysis,
    }
}

fn generic_tool_is_write_success(tool_name: &str, lower: &str) -> bool {
    matches!(
        tool_name,
        "create_file" | "write_file" | "edit_file" | "apply_patch" | "modify_file"
    ) || ["已创建文件", "created file", "apply_patch"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn generic_tool_is_build_or_test_success(tool_name: &str, lower: &str) -> bool {
    tool_name == "run_command"
        && [
            "cmake",
            "make",
            "cargo test",
            "cargo check",
            "pytest",
            "built target",
            "test result: ok",
            "tests passed",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn generic_tool_is_readonly_success(tool_name: &str, lower: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "read_dir"
            | "list_tree"
            | "glob"
            | "search"
            | "extract_in_file"
            | "repo_overview_sweep"
    ) || ["read file:", "read dir:", "list tree:"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn update_generic_tool_evidence_from_tool_text(
    evidence: &mut GenericToolEvidence,
    tool_name: &str,
    text: &str,
    ok: bool,
) {
    if !ok {
        return;
    }
    evidence.any_success = true;
    let lower = text.to_lowercase();
    if generic_tool_is_write_success(tool_name, &lower) {
        evidence.write_success = true;
    }
    if generic_tool_is_build_or_test_success(tool_name, &lower) {
        evidence.build_or_test_success = true;
    }
    if generic_tool_is_readonly_success(tool_name, &lower) {
        evidence.readonly_success = true;
    }
}

fn update_generic_tool_evidence_from_raw(evidence: &mut GenericToolEvidence, raw: &str) {
    if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
        let text = normalized_tool_text(&env);
        let ok = env.ok || env.exit_code == Some(0);
        update_generic_tool_evidence_from_tool_text(evidence, env.name.as_str(), &text, ok);
        return;
    }
    let parsed = crate::tool_result::parse_legacy_output("generic_tool", raw);
    let ok = parsed.ok || parsed.exit_code == Some(0) || tool_message_has_success_evidence(raw);
    update_generic_tool_evidence_from_tool_text(evidence, "generic_tool", raw, ok);
}

fn recent_generic_tool_evidence(messages: &[Message]) -> GenericToolEvidence {
    let mut evidence = GenericToolEvidence::default();
    for m in messages.iter().rev().take(24) {
        let Some(raw) = message_content_as_str(&m.content) else {
            continue;
        };
        if m.role == "tool" || m.tool_call_id.is_some() || m.name.is_some() {
            update_generic_tool_evidence_from_raw(&mut evidence, raw);
        }
    }
    evidence
}

fn assistant_completion_claim(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    let has_completion_marker = [
        "完成",
        "成功",
        "就绪",
        "通过",
        "结果如下",
        "总结如下",
        "done",
        "completed",
        "succeeded",
        "success",
        "ready",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if !has_completion_marker {
        return false;
    }
    let has_hard_blocker = [
        "未完成",
        "没有完成",
        "失败",
        "无法完成",
        "未成功",
        "error",
        "failed",
        "not completed",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    !has_hard_blocker
}

fn assistant_substantive_answer(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() < 40 {
        return false;
    }
    let lower = t.to_lowercase();
    let has_answer_shape = [
        "当前",
        "结果",
        "包含",
        "目录",
        "文件",
        "说明",
        "总结",
        "建议",
        "analysis",
        "summary",
        "contains",
        "directory",
        "files",
    ]
    .iter()
    .any(|k| lower.contains(k));
    let just_question = t.ends_with('？') || t.ends_with('?');
    has_answer_shape && !just_question
}

fn last_assistant_completion_claim(messages: &[Message]) -> bool {
    messages.iter().rev().take(12).any(|m| {
        if m.role != "assistant" || m.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            return false;
        }
        message_content_as_str(&m.content).is_some_and(assistant_completion_claim)
    })
}

fn last_assistant_substantive_answer(messages: &[Message]) -> bool {
    messages.iter().rev().take(12).any(|m| {
        if m.role != "assistant" || m.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            return false;
        }
        message_content_as_str(&m.content).is_some_and(assistant_substantive_answer)
    })
}

fn check_generic_successful_tool_then_completion(
    task: &str,
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    let intent = generic_task_intent_from_task(task);
    let evidence = recent_generic_tool_evidence(messages);
    let completion = last_assistant_completion_claim(messages);
    let substantive = last_assistant_substantive_answer(messages);
    let has_impl_intent = intent.build_or_test || intent.write_change;
    let satisfied = if has_impl_intent {
        (!intent.build_or_test || (evidence.build_or_test_success && completion))
            && (!intent.write_change || (evidence.write_success && completion))
    } else if intent.readonly_analysis {
        evidence.readonly_success && (completion || substantive)
    } else {
        evidence.any_success && completion
    };
    if satisfied {
        GoalCompletionEvidenceCheck::Satisfied
    } else {
        GoalCompletionEvidenceCheck::NotApplicable
    }
}

/// 对分阶段滚动执行的当前消息历史做目标完成证据核对。
///
/// 编排层只消费三态结果；具体领域规则（当前先覆盖程序写入/编译/运行）在本模块内扩展。
pub(crate) fn check_goal_completion_evidence_from_messages(
    task: &str,
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    for detector in GOAL_EVIDENCE_DETECTORS {
        if (detector.applies)(task) {
            return (detector.check_messages)(task, messages);
        }
    }
    check_generic_successful_tool_then_completion(task, messages)
}

/// 对「写 C++ + 编译 + 运行」类任务做轻量证据核对；缺项时返回说明字符串。
pub(crate) fn verify_task_level_execution_evidence(
    task: &str,
    results: &[TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if !is_program_build_run_request(task) {
        return None;
    }
    let mut wrote_source = false;
    let mut compiled = false;
    let mut ran_program = false;
    let expected_outputs = expected_output_hints_for_results(task, results, goal_expected_outputs);

    for r in results {
        let combined_full = combined_output_error(r);
        let combined_lower = combined_full.to_lowercase();
        let (w, c, run) =
            per_result_verify_flags(r, &combined_lower, &combined_full, &expected_outputs);
        wrote_source |= w;
        compiled |= c;
        ran_program |= run;
    }

    let mut missing = Vec::new();
    if !wrote_source {
        missing.push("write_source");
    }
    if !compiled {
        missing.push("compile");
    }
    if !ran_program {
        missing.push("run");
    }
    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "missing: {}; 需要至少包含写源码(.cpp)+编译(g++/clang++)+运行(可执行输出)",
            missing.join(",")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(text.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn tool_env(name: &str, summary: &str, output: &str) -> Message {
        let parsed = crate::tool_result::parse_legacy_output(name, output);
        msg(
            "tool",
            &crate::tool_result::encode_tool_message_envelope_v1(
                name,
                summary.to_string(),
                &parsed,
                output,
                None,
            ),
        )
    }

    #[test]
    fn message_history_evidence_passes_cpp_build_run() {
        let messages = vec![
            msg("tool", "create file: hello.cpp\n已创建文件: hello.cpp"),
            msg(
                "tool",
                "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
            msg(
                "tool",
                "$ build/hello\n退出码：0\n标准输出：\nHello from CrabMate C++ program!",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages(
                "在当前目录下编写一个c++程序，使用cmake编译执行",
                &messages,
            ),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn crabmate_tool_envelope_evidence_passes_cpp_build_run() {
        let messages = vec![
            tool_env(
                "create_file",
                "create file: hello.cpp",
                "已创建文件: hello.cpp",
            ),
            tool_env(
                "run_command",
                "cmake --build build",
                "命令：cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
            tool_env(
                "run_command",
                "./build/hello ./hello",
                "命令：./build/hello ./hello\n退出码：0\n标准输出：\nHello, World!",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages(
                "编写一个简单c++程序，使用cmake编译执行",
                &messages,
            ),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn cmake_config_build_run_sequence_satisfies_cpp_task() {
        let messages = vec![
            tool_env(
                "create_file",
                "create file: main.cpp",
                "已创建文件: main.cpp",
            ),
            tool_env(
                "create_file",
                "create file: CMakeLists.txt",
                "已创建文件: CMakeLists.txt",
            ),
            tool_env(
                "run_command",
                "cmake -S . -B build",
                "命令：cmake -S . -B build\n退出码：0\n标准输出：\n-- Configuring done\n-- Generating done",
            ),
            tool_env(
                "run_command",
                "cmake --build build",
                "命令：cmake --build build\n退出码：0\n标准输出：\n[100%] Linking CXX executable demo\n[100%] Built target demo",
            ),
            tool_env(
                "run_command",
                "./build/demo",
                "命令：./build/demo\n退出码：0\n标准输出：\nHello from CrabMate!",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages(
                "帮我编写一个简单c++程序，然后使用cmake编译执行",
                &messages,
            ),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn message_history_evidence_requires_run() {
        let messages = vec![
            msg("tool", "create file: hello.cpp\n已创建文件: hello.cpp"),
            msg(
                "tool",
                "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
        ];
        assert!(matches!(
            check_goal_completion_evidence_from_messages(
                "在当前目录下编写一个c++程序，使用cmake编译执行",
                &messages,
            ),
            GoalCompletionEvidenceCheck::Missing(_)
        ));
    }

    #[test]
    fn multi_turn_compile_not_satisfied_by_earlier_readonly_turn() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: .\n三个压缩包"),
            msg("assistant", "当前目录包含三个压缩包，已分析完成。"),
            msg("user", "编译hpcg"),
            msg("assistant", "好的，先解压看看结构。"),
        ];
        assert_eq!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    #[test]
    fn multi_turn_readonly_satisfied_only_for_active_window() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包与归档文件，分析结果如下。",
            ),
        ];
        assert_eq!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn orchestration_injection_does_not_shrink_evidence_window() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: ."),
            msg("user", "【编排纠偏】请实际执行工具，勿空口承诺"),
            msg(
                "assistant",
                "当前目录包含三个压缩包与归档文件，分析结果如下。",
            ),
        ];
        assert_eq!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn unpack_only_then_completion_claim_does_not_satisfy_build_task() {
        let messages = vec![
            tool_env(
                "archive_unpack",
                "unpack hpcg",
                "已解压 187 个文件到: .\n顶层条目: hpcg-HPCG-release-3-1-0",
            ),
            msg("assistant", "解压完成，编译成功。"),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("编译hpcg", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    #[test]
    fn generic_successful_tool_then_completion_satisfies_build_only_task() {
        let messages = vec![
            tool_env(
                "run_command",
                "make arch=Linux_Serial -C hpcg-HPCG-release-3-1-0",
                "命令：make arch=Linux_Serial -C hpcg-HPCG-release-3-1-0\n退出码：0\n标准输出：\n/usr/bin/g++ src/main.o -o bin/xhpcg",
            ),
            msg(
                "assistant",
                "HPCG 编译完成。产物：hpcg-HPCG-release-3-1-0/bin/xhpcg。",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("编译hpcg", &messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn generic_completion_claim_without_tool_success_is_not_applicable() {
        let messages = vec![msg("assistant", "任务已完成。")];
        assert_eq!(
            check_goal_completion_evidence_from_messages("编译项目", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    #[test]
    fn mixed_build_and_analysis_requires_build_evidence_not_readonly_shortcut() {
        let messages = vec![
            tool_env(
                "list_tree",
                "list tree",
                "list tree: .\n源码与 CMakeLists.txt",
            ),
            msg(
                "assistant",
                "目录结构分析如下：包含源码目录与构建配置，总结完成。",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("编译 hpcg 并分析目录结构", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    #[test]
    fn generic_readonly_analysis_with_tool_and_answer_satisfies() {
        let messages = vec![
            tool_env(
                "read_file",
                "read file: README.md",
                "读取文件\n退出码：0\n标准输出：\n# Project\n",
            ),
            msg(
                "assistant",
                "当前项目包含 README 和源码目录。总结如下：主要入口在 src，建议后续查看构建配置。",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("分析当前项目", &messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    #[test]
    fn generic_started_message_does_not_satisfy() {
        let messages = vec![
            tool_env(
                "read_file",
                "read file: README.md",
                "读取文件\n退出码：0\n标准输出：\n# Project\n",
            ),
            msg("assistant", "我已开始分析当前项目。"),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("分析当前项目", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    #[test]
    fn message_history_evidence_not_applicable_for_other_tasks() {
        let messages = vec![msg("assistant", "完成")];
        assert_eq!(
            check_goal_completion_evidence_from_messages("解释一下 Rust 所有权", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }
}
