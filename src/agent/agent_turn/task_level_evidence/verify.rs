//! 任务级执行证据核对（写源码 / 编译 / 运行）。

use crate::agent::hierarchy::goal_verifier;
use crate::agent::hierarchy::task::{ArtifactKind, BuildArtifactKind, TaskResult};
use std::collections::HashMap;

use super::common::{
    combined_output_error, cpp_source_path, expected_output_hints_for_results,
    is_program_build_run_request,
};

fn artifact_evidence_flags(r: &TaskResult) -> (bool, bool) {
    let mut wrote_source = false;
    let mut compiled = false;
    for a in &r.artifacts {
        match a.kind {
            ArtifactKind::File => {
                if cpp_source_path(a.path.as_deref()) {
                    wrote_source = true;
                }
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
