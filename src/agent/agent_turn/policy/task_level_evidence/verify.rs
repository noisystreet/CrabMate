//! 任务级执行证据核对（写源码 / 编译 / 运行）。

use crate::cm_names as tool_names;
use crate::cm_types::{Message, message_content_as_str};

use super::common::is_program_build_run_request;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalCompletionEvidenceCheck {
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
}


fn history_contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn message_history_build_flags(history_lower: &str) -> (bool, bool, bool) {
    const WRITE_HINTS: &[&str] = &[
        "create file",
        "创建文件",
        "已创建文件",
        "apply_patch",
    ];
    const COMPILE_HINTS: &[&str] = &[
        "cmake --build",
        "built target",
        "linking cxx executable",
    ];
    const EXIT_OK: &[&str] = &["退出码：0", "exit code: 0"];
    const BUILD_PATH: &[&str] = &["build/", "./build/"];
    const STDOUT: &[&str] = &["标准输出", "stdout"];
    let wrote_source = history_lower.contains(".cpp") && history_contains_any(history_lower, WRITE_HINTS);
    let compiled = history_contains_any(history_lower, COMPILE_HINTS)
        || (history_lower.contains("configuring done") && history_lower.contains("generating done"));
    let ran_program = history_contains_any(history_lower, EXIT_OK)
        && history_contains_any(history_lower, BUILD_PATH)
        && history_contains_any(history_lower, STDOUT);
    (wrote_source, compiled, ran_program)
}

fn normalized_tool_text(env: &crate::cm_tools::tool_result::NormalizedToolEnvelope) -> String {
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

fn run_command_invocation_from_env(
    env: &crate::cm_tools::tool_result::NormalizedToolEnvelope,
) -> String {
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

fn structured_stdout_nonempty(env: &crate::cm_tools::tool_result::NormalizedToolEnvelope) -> bool {
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

fn tool_exit_ok(env: &crate::cm_tools::tool_result::NormalizedToolEnvelope) -> bool {
    env.ok || env.exit_code == Some(0)
}

fn maybe_mark_wrote_cpp_source(
    evidence: &mut ProgramBuildRunEvidence,
    env: &crate::cm_tools::tool_result::NormalizedToolEnvelope,
    lower: &str,
) {
    if matches!(
        env.name.as_str(),
        tool_names::CREATE_FILE | tool_names::APPLY_PATCH
    ) && lower.contains(".cpp")
    {
        evidence.wrote_source = true;
    }
}

fn maybe_mark_compiled_from_run_command(
    evidence: &mut ProgramBuildRunEvidence,
    invocation: &str,
    lower: &str,
) {
    if invocation.contains("cmake --build")
        || lower.contains("cmake --build")
        || lower.contains("built target")
        || lower.contains("linking cxx executable")
    {
        evidence.compiled = true;
    }
}

fn maybe_mark_ran_program(
    evidence: &mut ProgramBuildRunEvidence,
    env: &crate::cm_tools::tool_result::NormalizedToolEnvelope,
    invocation: &str,
    lower: &str,
) {
    let parsed =
        crate::cm_tools::tool_result::parse_tool_output(env.name.as_str(), env.output.as_str());
    let stdout_nonempty = structured_stdout_nonempty(env) || !parsed.stdout.trim().is_empty();
    if looks_like_build_artifact_run(invocation, lower) && stdout_nonempty {
        evidence.ran_program = true;
    }
}

fn update_program_evidence_from_tool_env(
    evidence: &mut ProgramBuildRunEvidence,
    env: &crate::cm_tools::tool_result::NormalizedToolEnvelope,
) {
    let text = normalized_tool_text(env);
    let lower = text.to_lowercase();
    maybe_mark_wrote_cpp_source(evidence, env, &lower);
    if env.name != tool_names::RUN_COMMAND || !tool_exit_ok(env) {
        return;
    }
    let invocation = run_command_invocation_from_env(env).to_lowercase();
    maybe_mark_compiled_from_run_command(evidence, &invocation, &lower);
    maybe_mark_ran_program(evidence, env, &invocation, &lower);
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
        if let Some(env) = crate::cm_tools::tool_result::normalize_tool_message_content(raw) {
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


/// 用户任务是否像编译/构建/测试类（供外循环门控与完成证据抑制共用）。
pub fn generic_task_intent_implies_build_or_test(task: &str) -> bool {
    generic_task_intent_from_task(task).build_or_test
}

fn messages_slice_since_last_user(messages: &[Message]) -> Option<&[Message]> {
    crate::cm_types::messages_slice_since_last_real_user(messages, false)
}

/// L2 外循环与分阶段滚动视界：以**最新一条用户消息**为目标，且仅在自该 user 起的消息窗口内核对完成证据。
///
/// 勿用会话首条 user 或分阶段「不变层」历史锚点（[`crate::cm_types::first_real_user_task_content`]），
/// 否则多轮对话会把上一任务（如「分析目录」）误判为当前目标（如「编译 hpcg」）已完成。
pub fn check_active_user_goal_completion_evidence(
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    let Some(task) = crate::cm_types::last_real_user_task_content(messages, false) else {
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
    GenericTaskIntent { build_or_test }
}

/// 只读分析类任务：assistant 是否已给出結论式回答。
///
/// **这是本文中唯一保留的自述依赖**，旧路径同样靠它。理由：结构化证据只能证明
/// 「读过文件」，无法区分「刚开始看」与「已给出分析」——两者属于第 ② 层
/// （LLM 语义判定，#1387）的职责。在那一层落地前保留旧判定，机制上不让该场景
/// 的早停能力回退。
fn assistant_gave_conclusion(messages: &[Message]) -> bool {
    messages.iter().rev().take(12).any(|m| {
        if m.role != "assistant" || m.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
            return false;
        }
        let Some(raw) = message_content_as_str(&m.content) else {
            return false;
        };
        assistant_completion_claim(raw) || assistant_substantive_answer(raw)
    })
}

fn assistant_completion_claim(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    const MARKERS: &[&str] = &[
        "完成", "成功", "就绪", "通过", "结果如下", "总结如下", "done", "completed", "succeeded",
        "success", "ready",
    ];
    const BLOCKERS: &[&str] = &[
        "未完成",
        "没有完成",
        "失败",
        "无法完成",
        "未成功",
        "error",
        "failed",
        "not completed",
    ];
    MARKERS.iter().any(|m| lower.contains(m)) && !BLOCKERS.iter().any(|m| lower.contains(m))
}

fn assistant_substantive_answer(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() < 40 {
        return false;
    }
    let lower = t.to_lowercase();
    const SHAPES: &[&str] = &[
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
    ];
    let has_shape = SHAPES.iter().any(|k| lower.contains(k));
    let just_question = t.ends_with('？') || t.ends_with('?');
    has_shape && !just_question
}

/// 通用兜底：按**结构化证据**判定（#1387 S0.5 第 ① 层）。
///
/// 与旧实现的关键差异：
///
/// 1. **构建/测试路径不再靠工具名猜**：旧 `generic_tool_is_build_or_test_success` 前置
///    `tool_name == run_command`，于是用规范的 `cargo_test` / `cargo_check` 反而判不出；
/// 2. **纯写任务无验证证据时返回 `NotApplicable`**（旧实现返回 `Satisfied`，即零验证放行）。
///    结构化层只能确认「写过了」，「改对了没有」属第 ② 层（LLM 语义判定）职责；
/// 3. **构建/测试路径不再把自述当必要条件**：旧路径要求工具成功 **AND** 正文含「完成」，
///    导致「修复 xx 失败」类任务成功后复述原文即判不完成。
///
/// 仍保留的自述依赖只有只读分析一条，理由见 [`assistant_gave_conclusion`]。
fn check_generic_successful_tool_then_completion(
    task: &str,
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    let intent = generic_task_intent_from_task(task);
    let ev = super::evidence::EvidenceSet::collect(messages);

    // 判定顺序刻意是「证据优先、意图兜底」：意图靠关键词表，天然漏词
    // （例如「把常量**改成** 42」不含「修改」，`write_change` 不会命中）。
    // 若让关键词在前，这类任务就会掉进兜底分支并被旧的自述规则放行。

    // ① 任务明说要构建/测试，却没有成功的验证性执行 → 不下结论。
    if intent.build_or_test && !ev.has_any_verify_success() {
        return GoalCompletionEvidenceCheck::NotApplicable;
    }
    // ② 有任何成功的验证性执行（专用工具结构化退出码 / run_command 构建测试特征）
    //    → 结构化层足以确认「验证过了」。
    if ev.has_any_verify_success() {
        return GoalCompletionEvidenceCheck::Satisfied;
    }
    // ③ 改过工作区但没有验证证据 → 只能确认「写过了」，不下结论（第 ② 层职责）。
    if ev.has_write_success() {
        return GoalCompletionEvidenceCheck::NotApplicable;
    }
    // ④ 只读分析 / 其他：需工具成功 + 已给出结论式回答
    //    （自述依赖仅保留在此，理由见 [`assistant_gave_conclusion`]）。
    if ev.has_any_success() && assistant_gave_conclusion(messages) {
        GoalCompletionEvidenceCheck::Satisfied
    } else {
        GoalCompletionEvidenceCheck::NotApplicable
    }
}

/// 对分阶段滚动执行的当前消息历史做目标完成证据核对。
///
/// 编排层只消费三态结果；具体领域规则（当前先覆盖程序写入/编译/运行）在本模块内扩展。
pub fn check_goal_completion_evidence_from_messages(
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_fixtures::{msg, tool_env};


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

    /// #1387 实测缺陷 ①：旧实现前置 `tool_name == run_command`，
    /// 于是用规范的 `cargo_test` 反而判不出（同一事实、仅工具名不同、结论相反）。
    #[test]
    fn dedicated_cargo_test_satisfies_build_task() {
        let messages = vec![
            msg("user", "修复 bug 并跑通全部测试"),
            tool_env(
                tool_names::MODIFY_FILE,
                "修改 src/x.rs",
                "已修改文件: src/x.rs",
            ),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 100 passed; 0 failed",
            ),
            msg("assistant", "已修复该 bug，全部测试通过，任务完成。"),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("修复 bug 并跑通全部测试", &messages),
            GoalCompletionEvidenceCheck::Satisfied,
            "专用 cargo_test 的结构化退出码应被采信"
        );
    }

    /// #1387 实测缺陷 ②：旧实现把「改文件 + assistant 说完成」直接判 `Satisfied`，
    /// 零验证放行。结构化层只能确认「写过了」，因此应不下结论。
    #[test]
    fn pure_write_without_verification_is_not_satisfied() {
        let messages = vec![
            msg("user", "把常量改成 42"),
            tool_env(
                tool_names::MODIFY_FILE,
                "修改 src/x.rs",
                "已修改文件: src/x.rs",
            ),
            msg("assistant", "已完成修改，任务完成。"),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("把常量改成 42", &messages),
            GoalCompletionEvidenceCheck::NotApplicable,
            "纯写任务无验证证据时不应判完成"
        );
    }

    /// 对照：同一纯写任务若顺手跑了测试（有验证证据），则可以判完成。
    #[test]
    fn pure_write_with_verification_is_satisfied() {
        let messages = vec![
            msg("user", "把常量改成 42"),
            tool_env(
                tool_names::MODIFY_FILE,
                "修改 src/x.rs",
                "已修改文件: src/x.rs",
            ),
            tool_env(
                tool_names::CARGO_TEST,
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 12 passed; 0 failed",
            ),
        ];
        assert_eq!(
            check_goal_completion_evidence_from_messages("把常量改成 42", &messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }
}
