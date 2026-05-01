//! `OperatorAgent::analyze_tool_execution` 的拆分实现（降低单函数圈复杂度）。

use super::super::goal_verifier;
use super::super::task::SubGoal;
use super::super::tool_executor::{ExtractedArtifactKind, ToolExecutionResult};
use super::state::ToolExecutionOutcome;

pub(super) fn analyze_operator_tool_execution(
    result: &ToolExecutionResult,
    goal: &SubGoal,
) -> ToolExecutionOutcome {
    if !result.success {
        return ToolExecutionOutcome::Normal;
    }

    let output = &result.output;
    let tool_name = &result.tool_name;
    let goal_desc = goal.description.to_lowercase();

    if let Some(out) = outcome_run_executable_success(tool_name, output) {
        return out;
    }
    if let Some(out) = outcome_run_command_hello_verified(tool_name, goal, output) {
        return out;
    }
    if tool_name == "run_command" && output.contains("ELF") && output.contains("executable") {
        return ToolExecutionOutcome::Normal;
    }

    if let Some(out) = outcome_build_linking_executable(tool_name, goal, output) {
        return out;
    }
    if let Some(out) = outcome_build_goal_extracted_executable(goal, &goal_desc, result) {
        return out;
    }

    ToolExecutionOutcome::Normal
}

fn outcome_run_executable_success(tool_name: &str, output: &str) -> Option<ToolExecutionOutcome> {
    if tool_name != "run_executable" {
        return None;
    }
    if output.to_lowercase().contains("hello")
        || (output.contains("退出码：0")
            && output.chars().count() < 8000
            && !output.contains("cmake version"))
    {
        Some(ToolExecutionOutcome::TaskCompleted {
            reason: "Program executed successfully with expected output".to_string(),
        })
    } else {
        None
    }
}

fn outcome_run_command_hello_verified(
    tool_name: &str,
    goal: &SubGoal,
    output: &str,
) -> Option<ToolExecutionOutcome> {
    if tool_name != "run_command" || !goal_verifier::is_run_executable_subgoal(goal) {
        return None;
    }
    if goal_verifier::run_command_invocation_mentions_hello(&format!(
        "Tool run_command executed successfully: {}",
        output
    )) {
        Some(ToolExecutionOutcome::TaskCompleted {
            reason: "Program executed successfully via run_command (verified stdout)".to_string(),
        })
    } else {
        None
    }
}

fn outcome_build_linking_executable(
    tool_name: &str,
    goal: &SubGoal,
    output: &str,
) -> Option<ToolExecutionOutcome> {
    if !(tool_name == "run_command" || tool_name == "cmake" || tool_name == "make") {
        return None;
    }
    if goal_verifier::is_run_executable_subgoal(goal)
        || !output.contains("[100%]")
        || !output.contains("Linking")
        || !output.contains("executable")
    {
        return None;
    }
    let line = output
        .lines()
        .find(|l| l.contains("Linking") && l.contains("executable"))?;
    let name = line.split_whitespace().last()?;
    Some(ToolExecutionOutcome::TaskCompleted {
        reason: format!("Build completed: executable '{name}' generated"),
    })
}

fn goal_looks_like_build(goal_desc: &str) -> bool {
    goal_desc.contains("编译")
        || goal_desc.contains("build")
        || goal_desc.contains("make")
        || goal_desc.contains("cmake")
        || goal_desc.contains("链接")
}

fn outcome_build_goal_extracted_executable(
    goal: &SubGoal,
    goal_desc: &str,
    result: &ToolExecutionResult,
) -> Option<ToolExecutionOutcome> {
    if goal_verifier::is_run_executable_subgoal(goal)
        || !goal_looks_like_build(goal_desc)
        || result.extracted_artifacts.is_empty()
    {
        return None;
    }
    for artifact in &result.extracted_artifacts {
        if matches!(artifact.kind, ExtractedArtifactKind::Executable) {
            return Some(ToolExecutionOutcome::TaskCompleted {
                reason: format!("Build completed: {} generated", artifact.path.display()),
            });
        }
    }
    None
}
