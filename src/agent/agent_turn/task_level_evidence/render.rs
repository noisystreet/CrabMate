//! 「关键证据」Markdown 渲染。

use std::collections::HashMap;

use crate::agent::hierarchy::goal_verifier;
use crate::agent::hierarchy::task::TaskResult;

use super::common::{
    combined_output_error, cpp_source_path, expected_output_hints_for_results,
    is_program_build_run_request,
};

fn lower_signals_build(lower: &str) -> bool {
    lower.contains("built target")
        || lower.contains("cmake --build")
        || lower.contains("linking cxx executable")
}

fn per_result_render_scan(
    r: &TaskResult,
    combined: &str,
    lower: &str,
    expected_outputs: &[String],
    matched_expected_outputs: &mut Vec<String>,
) -> (bool, bool, bool, bool) {
    let mut wrote_source = false;
    let mut built_binary = false;
    let mut ran_binary = false;
    let mut seen_expected_output = false;

    for a in &r.artifacts {
        if cpp_source_path(a.path.as_deref()) {
            wrote_source = true;
        }
        if a.path
            .as_deref()
            .is_some_and(|p| p.to_lowercase().contains("build/"))
        {
            built_binary = true;
        }
    }
    if lower_signals_build(lower) {
        built_binary = true;
    }

    if r.tools_invoked.iter().any(|n| n == "run_executable")
        || (r.tools_invoked.iter().any(|n| n == "run_command")
            && goal_verifier::run_command_invocation_matches_expected_output(
                combined,
                expected_outputs,
            ))
    {
        ran_binary = true;
    }

    for hint in expected_outputs {
        if hint.is_empty() {
            continue;
        }
        if lower.contains(&hint.to_lowercase()) {
            seen_expected_output = true;
            if !matched_expected_outputs
                .iter()
                .any(|x| x.eq_ignore_ascii_case(hint))
            {
                matched_expected_outputs.push(hint.clone());
            }
        }
    }

    (wrote_source, built_binary, ran_binary, seen_expected_output)
}

pub(crate) fn render_task_level_evidence(
    task: &str,
    results: &[TaskResult],
    goal_expected_outputs: &HashMap<String, Vec<String>>,
) -> String {
    if !is_program_build_run_request(task) {
        return String::new();
    }

    let mut wrote_source = false;
    let mut built_binary = false;
    let mut ran_binary = false;
    let mut seen_expected_output = false;
    let expected_outputs = expected_output_hints_for_results(task, results, goal_expected_outputs);
    let mut matched_expected_outputs: Vec<String> = Vec::new();

    for r in results {
        let combined = combined_output_error(r);
        let lower = combined.to_lowercase();
        let (w, b, run, seen) = per_result_render_scan(
            r,
            &combined,
            &lower,
            &expected_outputs,
            &mut matched_expected_outputs,
        );
        wrote_source |= w;
        built_binary |= b;
        ran_binary |= run;
        seen_expected_output |= seen;
    }

    let mut lines = vec!["## 关键证据".to_string(), String::new()];
    lines.push(format!(
        "- 源码落地：{}",
        if wrote_source {
            "已检测到 `.cpp` 源文件写入"
        } else {
            "未检测到明确证据"
        }
    ));
    lines.push(format!(
        "- 编译产物：{}",
        if built_binary {
            "已检测到构建/链接成功信号"
        } else {
            "未检测到明确证据"
        }
    ));
    lines.push(format!(
        "- 运行验证：{}",
        if ran_binary || seen_expected_output {
            if expected_outputs.is_empty() {
                "已检测到程序执行（含可核对输出）"
            } else {
                "已检测到程序执行（含期望输出）"
            }
        } else {
            "未检测到明确证据"
        }
    ));
    if !expected_outputs.is_empty() {
        let expected_joined = expected_outputs
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join("、");
        lines.push(format!("- acceptance 期望输出：{}", expected_joined));
        if matched_expected_outputs.is_empty() {
            lines.push("- acceptance 核对结果：未在工具输出中检测到期望片段".to_string());
        } else {
            let matched_joined = matched_expected_outputs
                .iter()
                .map(|s| format!("`{}`", s))
                .collect::<Vec<_>>()
                .join("、");
            lines.push(format!(
                "- acceptance 核对结果：已检测到 {}",
                matched_joined
            ));
        }
    }
    lines.join("\n")
}
