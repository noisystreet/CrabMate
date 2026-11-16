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


/// 对分阶段滚动执行的当前消息历史做目标完成证据核对。
///
/// 第 ① 层只回答「**事实是什么**」：只有**领域检测器**（当前只有「写源码 → 编译 → 运行」
/// 三件套齐全的 C++ 任务）能给出 `Satisfied` / `Missing`；其余一律 `NotApplicable`，
/// 把「这些事实够不够算完成」交给第 ② 层语义判定
/// （[`crate::agent::completion_judge_llm`]，#1387 S0.5 / #1415）。
///
/// **刻意不再有通用关键词兜底**。被删掉的两条旧路径都是
/// `docs/design/agent_autonomy_enhancement.md` §1 明确否决的范式：
///
/// - `build_or_test_intent`：用 13 个子串猜任务意图。`"test"` 命中 `greatest` / `latest`、
///   `"make"` 命中 `make sure`；而「重构 X 并保证行为不变」这类综合任务一个词都不命中，
///   于是恰恰拿不到早停。
/// - `assistant_gave_conclusion`：用「完成 / 成功 / 结果如下」猜回答完成度，把 assistant
///   自述当完成证据——正是 §3.2 缺陷 ③。
///
/// 两者都是「同一事实因措辞不同而判成相反结论」，问题在范式不在覆盖，故整体移除而非继续补词。
pub fn check_goal_completion_evidence_from_messages(
    task: &str,
    messages: &[Message],
) -> GoalCompletionEvidenceCheck {
    for detector in GOAL_EVIDENCE_DETECTORS {
        if (detector.applies)(task) {
            return (detector.check_messages)(task, messages);
        }
    }
    // 没有检测器认领 = 本层不下结论。构建/测试、综合任务、只读分析都在此列，
    // 由第 ② 层语义判定（`completion_judge_enabled`）回答「够不够算完成」。
    GoalCompletionEvidenceCheck::NotApplicable
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

    /// 多轮对话：证据只在**当前 user 起**的窗口内核对（勿用会话首条 user）。
    ///
    /// 第 ① 层现在只剩 C++ 检测器能出 `Satisfied`，故用它的三件套验证窗口语义；
    /// 只读分析已交第 ② 层（见 `readonly_analysis_with_answer_is_not_applicable`）。
    #[test]
    fn multi_turn_cpp_evidence_counted_only_in_active_window() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: ."),
            msg("assistant", "当前目录包含三个压缩包，分析结果如下。"),
            msg("user", "编写一个 c++ 程序，用 cmake 编译执行"),
            tool_env(
                tool_names::CREATE_FILE,
                "create file: hello.cpp",
                "已创建文件: hello.cpp",
            ),
            tool_env(
                tool_names::RUN_COMMAND,
                "cmake --build build",
                "命令：cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
            tool_env(
                tool_names::RUN_COMMAND,
                "./build/hello",
                "命令：./build/hello\n退出码：0\n标准输出：\nHello, World!",
            ),
        ];
        assert_eq!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::Satisfied
        );
    }

    /// 编排纠偏注入的 user 消息**不得**收缩证据窗口：写源码那一步发生在注入之前，
    /// 若窗口从注入处开始，`wrote_source` 就会丢，三件套永远凑不齐。
    #[test]
    fn orchestration_injection_does_not_shrink_evidence_window() {
        let messages = vec![
            msg("user", "编写一个 c++ 程序，用 cmake 编译执行"),
            tool_env(
                tool_names::CREATE_FILE,
                "create file: hello.cpp",
                "已创建文件: hello.cpp",
            ),
            msg("user", "【编排纠偏】请实际执行工具，勿空口承诺"),
            tool_env(
                tool_names::RUN_COMMAND,
                "cmake --build build",
                "命令：cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
            tool_env(
                tool_names::RUN_COMMAND,
                "./build/hello",
                "命令：./build/hello\n退出码：0\n标准输出：\nHello, World!",
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

    /// `run_command` 跑 `make` 成功只是**弱**验证证据（文本特征），第 ① 层不再据此下结论；
    /// 是否算完成交第 ② 层语义判定（#1415）。
    #[test]
    fn make_build_command_alone_is_not_applicable() {
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
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    /// #1414 / #1415：一次 `cargo_check` 通过只证明「当前代码能编译」，不构成
    /// 「用户要我做的事已做完」的证据——否则综合任务跑到编译通过就被早停门掐断，
    /// 回合没有任何终答（实测见 #1414）。
    #[test]
    fn verify_success_alone_is_not_applicable() {
        let messages = vec![tool_env(
            tool_names::CARGO_CHECK,
            "cargo check",
            "退出码：0\n标准输出：\nFinished `dev` profile",
        )];
        assert_eq!(
            check_goal_completion_evidence_from_messages("分析这个 issue 怎么修", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }

    /// 同一份证据在**明确要求编译**的任务下也不再由第 ① 层判完成。
    ///
    /// 判定**与任务措辞无关**是这次改动的要点：旧 `build_or_test_intent` 会让「编译检查这个项目」
    /// 命中关键词而放行、换个说法就不放行，同一批证据得不到同一结论。
    #[test]
    fn verify_success_does_not_depend_on_task_wording() {
        let messages = vec![tool_env(
            tool_names::CARGO_CHECK,
            "cargo check",
            "退出码：0\n标准输出：\nFinished `dev` profile",
        )];
        assert_eq!(
            check_goal_completion_evidence_from_messages("编译检查这个项目", &messages),
            GoalCompletionEvidenceCheck::NotApplicable
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

    /// 只读分析任务同样不再由第 ① 层判完成：**读过了** ≠ **分析完并答出来了**。
    /// 后者是语义判断（旧 `assistant_gave_conclusion` 靠「总结 / 结果如下」猜回答完成度）。
    #[test]
    fn readonly_analysis_with_answer_is_not_applicable() {
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
            GoalCompletionEvidenceCheck::NotApplicable
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

    /// #1387 实测缺陷 ① 已在**证据采集层**修好：专用 `cargo_test` 带结构化退出码，
    /// 被采集为 `verify` / `strong`（见 `evidence.rs` 的同名测试），不再因工具名不同而漏判。
    ///
    /// 但「证据入账」不等于「任务完成」——第 ① 层不再替语义层下这个结论（#1415）。
    #[test]
    fn dedicated_cargo_test_alone_is_not_applicable() {
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
            GoalCompletionEvidenceCheck::NotApplicable,
            "专用工具证据入账，但完成与否交语义判定"
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

    /// 对照：纯写任务顺手跑了测试 → 仍不下结论。
    ///
    /// 旧实现这里判 `Satisfied`；实测表明它会让综合任务在「改完代码 + 一次 `cargo_check`
    /// 通过」后立刻早停停轮，而此刻模型还没写终答（#1414）。现在交给第 ② 层语义判定，
    /// 或干脆不早停——消费者最终一定能拿到答案（#1415 方案 A 兜底）。
    #[test]
    fn pure_write_with_verification_is_not_applicable() {
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
            GoalCompletionEvidenceCheck::NotApplicable
        );
    }
}
