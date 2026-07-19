//! 节点 `run_if`（由作者层 `when` 编译）求值。

use std::collections::HashMap;

use serde_json::Value;

use crate::resolve_json_path::resolve_json_path_value;

use super::model::{WorkflowBranch, WorkflowRunIf};
use super::types::{NodeRunResult, NodeRunStatus};

/// 依赖是否均已终结（含 `skipped` / `passed` / `failed`）。
pub(crate) fn node_deps_resolved(
    deps: &[String],
    completed: &HashMap<String, NodeRunResult>,
) -> bool {
    deps.iter().all(|d| completed.contains_key(d))
}

/// 是否应执行该节点（`run_if` 为 `None` 时恒为 `true`）。
pub(crate) fn node_run_if_satisfied(
    run_if: Option<&WorkflowRunIf>,
    completed: &HashMap<String, NodeRunResult>,
) -> bool {
    let Some(cond) = run_if else {
        return true;
    };
    match cond {
        WorkflowRunIf::Branch { from, branch } => {
            let Some(r) = completed.get(from) else {
                return false;
            };
            match r.status {
                NodeRunStatus::Passed => *branch == WorkflowBranch::Success,
                NodeRunStatus::Failed => *branch == WorkflowBranch::Failure,
                NodeRunStatus::Skipped => false,
            }
        }
        WorkflowRunIf::Match {
            from,
            field,
            equals,
            in_list,
        } => {
            let Some(r) = completed.get(from) else {
                return false;
            };
            let Some(actual) = extract_match_value(r, field) else {
                return false;
            };
            if let Some(expected) = equals {
                return json_values_equal(&actual, expected);
            }
            if let Some(list) = in_list {
                return list.iter().any(|v| json_values_equal(&actual, v));
            }
            false
        }
    }
}

fn extract_match_value(r: &NodeRunResult, field: &str) -> Option<Value> {
    let text = r.output.as_ref();
    let field = field.trim();
    if field.starts_with('/') || field.starts_with('$') {
        return resolve_json_path_value(text, field).ok();
    }
    let root: Value = serde_json::from_str(text.trim()).ok()?;
    if let Some(obj) = root.as_object() {
        return obj.get(field).cloned();
    }
    None
}

fn json_values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x == y,
        _ => a == b,
    }
}

/// 从 `workflow.nodes[].run_if` JSON 解析。
pub(crate) fn parse_run_if_json(v: &Value) -> Result<Option<WorkflowRunIf>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let obj = v
        .as_object()
        .ok_or_else(|| "run_if 必须是对象".to_string())?;

    if let Some(branch) = obj.get("branch").and_then(|x| x.as_str()) {
        let from = obj
            .get("from")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("run_if.branch 须提供 from")?
            .to_string();
        let branch = match branch {
            "success" => WorkflowBranch::Success,
            "failure" => WorkflowBranch::Failure,
            other => {
                return Err(format!(
                    "run_if.branch 未知值: {other}（支持 success|failure）"
                ));
            }
        };
        return Ok(Some(WorkflowRunIf::Branch { from, branch }));
    }

    let match_v = obj.get("match").ok_or("run_if 须含 branch 或 match")?;
    let match_obj = match_v.as_object().ok_or("run_if.match 必须是对象")?;
    let from = obj
        .get("from")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("run_if.match 须提供 from")?
        .to_string();
    let field = match_obj
        .get("field")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("run_if.match 须提供 field")?
        .to_string();
    let equals = match_obj.get("equals").cloned();
    let in_list = match_obj.get("in").and_then(|x| x.as_array()).cloned();
    if equals.is_none() && in_list.is_none() {
        return Err("run_if.match 须提供 equals 或 in".to_string());
    }
    Ok(Some(WorkflowRunIf::Match {
        from,
        field,
        equals,
        in_list,
    }))
}
