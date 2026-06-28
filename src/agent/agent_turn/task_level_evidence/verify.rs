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
    GoalCompletionEvidenceCheck::NotApplicable
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
    fn message_history_evidence_not_applicable_for_other_tasks() {
        let messages = vec![msg("assistant", "完成")];
        assert_eq!(
            check_goal_completion_evidence_from_messages("解释一下 Rust 所有权", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }
}
