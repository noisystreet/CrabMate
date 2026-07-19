//! `for_each` 运行时展开（`static_items` 编译期展开见 `compile_spec`）。

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::resolve_json_path::resolve_json_path_value;

use super::model::{ForEachPendingSpec, WorkflowNodeSpec};
use super::types::{NodeRunResult, NodeRunStatus};

const DEFAULT_FOR_EACH_MAX: usize = 32;

pub(crate) fn default_for_each_max_items() -> usize {
    DEFAULT_FOR_EACH_MAX
}

/// 前驱完成后展开 pending；返回新加入的节点 id 列表（供 trace）。
pub(crate) fn expand_pending_for_each(
    pending: &mut Vec<ForEachPendingSpec>,
    nodes: &mut Vec<WorkflowNodeSpec>,
    completed: &HashMap<String, NodeRunResult>,
) -> Vec<String> {
    let mut expanded_ids = Vec::new();
    let mut idx = 0;
    while idx < pending.len() {
        let spec = pending[idx].clone();
        if !completed.contains_key(&spec.from) {
            idx += 1;
            continue;
        }
        let from_res = &completed[&spec.from];
        if from_res.status == NodeRunStatus::Skipped {
            pending.remove(idx);
            continue;
        }
        let items = match resolve_for_each_items(&spec, from_res) {
            Ok(v) => v,
            Err(_) => {
                pending.remove(idx);
                continue;
            }
        };
        if items.is_empty() {
            pending.remove(idx);
            continue;
        }
        let new_nodes = build_for_each_nodes(&spec, &items);
        for n in &new_nodes {
            expanded_ids.push(n.id.clone());
        }
        nodes.extend(new_nodes);
        pending.remove(idx);
    }
    expanded_ids
}

fn resolve_for_each_items(
    spec: &ForEachPendingSpec,
    from_res: &NodeRunResult,
) -> Result<Vec<String>, String> {
    if let Some(static_items) = &spec.static_items {
        let cap = spec.max_items.min(static_items.len());
        return Ok(static_items.iter().take(cap).cloned().collect());
    }
    let json_path = spec
        .json_path
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or("for_each 缺少 json_path 或 static_items")?;
    let value = resolve_json_path_value(from_res.output.as_ref(), json_path)
        .map_err(|e| format!("for_each json_path: {}", e.user_reason()))?;
    let arr = value
        .as_array()
        .ok_or_else(|| "for_each json_path 须指向 JSON 数组".to_string())?;
    let mut out = Vec::new();
    for item in arr.iter().take(spec.max_items) {
        let s = item_to_string(item)?;
        out.push(s);
    }
    Ok(out)
}

fn item_to_string(item: &Value) -> Result<String, String> {
    match item {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()),
    }
}

pub(crate) fn build_for_each_nodes(
    spec: &ForEachPendingSpec,
    items: &[String],
) -> Vec<WorkflowNodeSpec> {
    let mut nodes = Vec::with_capacity(items.len());
    let mut prev_id: Option<String> = None;
    for (i, item) in items.iter().enumerate() {
        let id = format!("{}_{}", spec.base_id, i);
        let mut deps = spec.extra_deps.clone();
        if spec.parallel {
            // 同层依赖 extra_deps（含 from）
        } else if let Some(p) = &prev_id {
            deps.push(p.clone());
        }
        let tool_args = substitute_item_in_value(&spec.tool_args_template, &spec.item_var, item);
        nodes.push(WorkflowNodeSpec {
            id: id.clone(),
            tool_name: spec.tool_name.clone(),
            tool_args,
            deps,
            requires_approval: spec.requires_approval,
            timeout_secs: spec.timeout_secs,
            compensate_with: spec.compensate_with.clone(),
            max_retries: spec.max_retries,
            node_tool_role: spec.node_tool_role,
            run_if: None,
        });
        prev_id = Some(id);
    }
    nodes
}

pub(crate) fn substitute_item_in_value(template: &Value, item_var: &str, item: &str) -> Value {
    let needle = format!("{{{{{item_var}}}}}");
    match template {
        Value::String(s) => Value::String(s.replace(&needle, item)),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| substitute_item_in_value(v, item_var, item))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute_item_in_value(v, item_var, item)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}
