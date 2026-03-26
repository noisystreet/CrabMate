//! 从 `workflow_execute` 参数 JSON 解析 `WorkflowSpec`。

use super::dag::topo_layers;
use super::model::{WorkflowNodeSpec, WorkflowSpec};

pub(crate) fn parse_workflow_spec(args_json: &str) -> Result<WorkflowSpec, String> {
    let v: serde_json::Value = serde_json::from_str(args_json).map_err(|e| e.to_string())?;
    let spec_v = v.get("workflow").unwrap_or(&v);

    let max_parallelism = spec_v
        .get("max_parallelism")
        .and_then(|x| x.as_u64())
        .unwrap_or(4) as usize;
    let fail_fast = spec_v
        .get("fail_fast")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let compensate_on_failure = spec_v
        .get("compensate_on_failure")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    let output_inject_max_chars = spec_v
        .get("output_inject_max_chars")
        .and_then(|x| x.as_u64())
        .unwrap_or(2000) as usize;

    let summary_preview_max_chars = spec_v
        .get("summary_preview_max_chars")
        .and_then(|x| x.as_u64())
        .unwrap_or(1200) as usize;

    let compensation_preview_max_chars = spec_v
        .get("compensation_preview_max_chars")
        .and_then(|x| x.as_u64())
        .unwrap_or(800) as usize;

    let nodes_v = spec_v.get("nodes").ok_or("workflow 缺少 nodes 字段")?;
    let mut nodes: Vec<WorkflowNodeSpec> = Vec::new();

    if let Some(arr) = nodes_v.as_array() {
        for it in arr.iter() {
            nodes.push(parse_node_from_value(it, None)?);
        }
    } else if nodes_v.is_object() {
        let obj = nodes_v
            .as_object()
            .ok_or_else(|| "workflow.nodes 不是对象".to_string())?;
        for (id, it) in obj.iter() {
            nodes.push(parse_node_from_value(it, Some(id))?);
        }
    } else {
        return Err("workflow.nodes 必须是数组或对象".to_string());
    }

    if nodes.is_empty() {
        return Err("workflow.nodes 不能为空".to_string());
    }
    let cached_layer_count = topo_layers(&nodes).map(|l| l.len()).unwrap_or(0);
    Ok(WorkflowSpec {
        max_parallelism,
        fail_fast,
        compensate_on_failure,
        output_inject_max_chars,
        summary_preview_max_chars,
        compensation_preview_max_chars,
        nodes,
        cached_layer_count,
    })
}

pub(crate) fn parse_node_from_value(
    v: &serde_json::Value,
    forced_id: Option<&String>,
) -> Result<WorkflowNodeSpec, String> {
    let id = forced_id
        .cloned()
        .or_else(|| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .ok_or("node 缺少 id")?;

    let tool_name = v
        .get("tool_name")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("tool").and_then(|x| x.as_str()))
        .ok_or(format!("node {} 缺少 tool_name", id))?
        .to_string();

    let tool_args = v
        .get("tool_args")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let deps = v
        .get("deps")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let requires_approval = v
        .get("requires_approval")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let timeout_secs = v.get("timeout_secs").and_then(|x| x.as_u64());

    let compensate_with = v
        .get("compensate_with")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let max_retries = v
        .get("max_retries")
        .and_then(|x| x.as_u64())
        .unwrap_or(0)
        .min(5) as u32;

    Ok(WorkflowNodeSpec {
        id,
        tool_name,
        tool_args,
        deps,
        requires_approval,
        timeout_secs,
        compensate_with,
        max_retries,
    })
}
