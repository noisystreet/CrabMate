//! 从 `workflow_execute` 参数 JSON 解析 `WorkflowSpec`。

use crate::tools::workflow_tool_args_satisfy_required;

use super::dag::topo_layers;
use super::model::{ForEachPendingSpec, WorkflowNodeSpec, WorkflowSpec};
use super::node_tool_role::WorkflowNodeToolRole;
use super::run_if::parse_run_if_json;
use super::workflow_templates::{
    SUPPORTED_WORKFLOW_TEMPLATES, apply_code_review_search_pattern,
    apply_refactor_symbol_to_workflow, merge_workflow_template_overlay,
    workflow_template_code_review, workflow_template_refactor_precheck,
    workflow_template_rust_ci_light,
};

fn resolve_workflow_template(spec_v: serde_json::Value) -> Result<serde_json::Value, String> {
    let template_key = spec_v
        .get("workflow_template")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(key) = template_key else {
        return Ok(spec_v);
    };

    let base = match key {
        "rust_ci_light" => workflow_template_rust_ci_light(),
        "code_review" => workflow_template_code_review(),
        "refactor_precheck" => workflow_template_refactor_precheck(),
        other => {
            return Err(format!(
                "未知 workflow_template: {other}（当前支持：{}）",
                SUPPORTED_WORKFLOW_TEMPLATES.join("、")
            ));
        }
    };

    let refactor_symbol = spec_v
        .get("refactor_symbol")
        .or_else(|| spec_v.get("symbol"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let review_pattern = spec_v
        .get("review_search_pattern")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let overlay = spec_v.as_object().map(|m| {
        serde_json::Value::Object(
            m.iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "workflow_template"
                            | "refactor_symbol"
                            | "symbol"
                            | "review_search_pattern"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    });

    let mut merged = merge_workflow_template_overlay(base, overlay.as_ref());

    if key == "refactor_precheck" {
        let sym = refactor_symbol.ok_or_else(|| {
            "workflow_template=refactor_precheck 须提供 refactor_symbol（或 symbol）非空字符串"
                .to_string()
        })?;
        apply_refactor_symbol_to_workflow(&mut merged, &sym)?;
    }

    if key == "code_review"
        && let Some(pat) = review_pattern
    {
        apply_code_review_search_pattern(&mut merged, &pat)?;
    }

    Ok(merged)
}

struct WorkflowSpecConfig {
    max_parallelism: usize,
    fail_fast: bool,
    compensate_on_failure: bool,
    output_inject_max_chars: usize,
    summary_preview_max_chars: usize,
    compensation_preview_max_chars: usize,
}

fn read_workflow_spec_config(spec_v: &serde_json::Value) -> WorkflowSpecConfig {
    WorkflowSpecConfig {
        max_parallelism: spec_v
            .get("max_parallelism")
            .and_then(|x| x.as_u64())
            .unwrap_or(4) as usize,
        fail_fast: spec_v
            .get("fail_fast")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        compensate_on_failure: spec_v
            .get("compensate_on_failure")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        output_inject_max_chars: spec_v
            .get("output_inject_max_chars")
            .and_then(|x| x.as_u64())
            .unwrap_or(2000) as usize,
        summary_preview_max_chars: spec_v
            .get("summary_preview_max_chars")
            .and_then(|x| x.as_u64())
            .unwrap_or(1200) as usize,
        compensation_preview_max_chars: spec_v
            .get("compensation_preview_max_chars")
            .and_then(|x| x.as_u64())
            .unwrap_or(800) as usize,
    }
}

fn parse_workflow_nodes_field(
    nodes_v: &serde_json::Value,
) -> Result<Vec<WorkflowNodeSpec>, String> {
    if let Some(arr) = nodes_v.as_array() {
        return arr
            .iter()
            .map(|it| parse_node_from_value(it, None))
            .collect();
    }
    if let Some(obj) = nodes_v.as_object() {
        return obj
            .iter()
            .map(|(id, it)| parse_node_from_value(it, Some(id)))
            .collect();
    }
    Err("workflow.nodes 必须是数组或对象".to_string())
}

pub(crate) fn parse_workflow_spec(args_json: &str) -> Result<WorkflowSpec, String> {
    let v: serde_json::Value = serde_json::from_str(args_json).map_err(|e| e.to_string())?;
    let v = super::compile_spec::compile_workflow_author_value(v)?;
    let mut spec_v = v.get("workflow").cloned().unwrap_or_else(|| v.clone());
    spec_v = resolve_workflow_template(spec_v)?;
    let cfg = read_workflow_spec_config(&spec_v);

    let nodes_v = spec_v.get("nodes").ok_or("workflow 缺少 nodes 字段")?;
    let nodes = parse_workflow_nodes_field(nodes_v)?;
    if nodes.is_empty() && spec_v.get("for_each_pending").is_none() {
        return Err("workflow.nodes 不能为空（且无 for_each_pending）".to_string());
    }
    let for_each_pending = parse_for_each_pending_list(spec_v.get("for_each_pending"))?;
    let cached_layer_count = topo_layers(&nodes).map(|l| l.len()).unwrap_or(0);
    Ok(WorkflowSpec {
        max_parallelism: cfg.max_parallelism,
        fail_fast: cfg.fail_fast,
        compensate_on_failure: cfg.compensate_on_failure,
        output_inject_max_chars: cfg.output_inject_max_chars,
        summary_preview_max_chars: cfg.summary_preview_max_chars,
        compensation_preview_max_chars: cfg.compensation_preview_max_chars,
        nodes,
        cached_layer_count,
        for_each_pending,
    })
}

fn parse_for_each_pending_list(
    v: Option<&serde_json::Value>,
) -> Result<Vec<ForEachPendingSpec>, String> {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (i, it) in arr.iter().enumerate() {
        let obj = it
            .as_object()
            .ok_or_else(|| format!("for_each_pending[{i}] 必须是对象"))?;
        let base_id = obj
            .get("base_id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("for_each_pending[{i}] 缺少 base_id"))?
            .to_string();
        let from = obj
            .get("from")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("for_each_pending[{i}] 缺少 from"))?
            .to_string();
        out.push(ForEachPendingSpec {
            base_id,
            from,
            json_path: obj
                .get("json_path")
                .and_then(|x| x.as_str())
                .map(str::to_string),
            static_items: None,
            item_var: obj
                .get("item_var")
                .and_then(|x| x.as_str())
                .unwrap_or("item")
                .to_string(),
            max_items: obj.get("max_items").and_then(|x| x.as_u64()).unwrap_or(32) as usize,
            parallel: obj
                .get("parallel")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            tool_name: obj
                .get("tool")
                .and_then(|x| x.as_str())
                .ok_or_else(|| format!("for_each_pending[{i}] 缺少 tool"))?
                .to_string(),
            tool_args_template: obj
                .get("tool_args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
            requires_approval: false,
            timeout_secs: None,
            compensate_with: vec![],
            max_retries: 0,
            node_tool_role: None,
            extra_deps: obj
                .get("extra_deps")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(out)
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

    let mut requires_approval = v
        .get("requires_approval")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    // 写本地镜像缓存：工作流中默认强制人工审批（忽略 JSON false）。
    if tool_name == "docker_build" {
        requires_approval = true;
    }

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

    let node_tool_role = v
        .get("node_tool_role")
        .or_else(|| v.get("executor_kind"))
        .and_then(|x| serde_json::from_value::<WorkflowNodeToolRole>(x.clone()).ok());

    let run_if = v
        .get("run_if")
        .map(parse_run_if_json)
        .transpose()?
        .flatten();

    workflow_tool_args_satisfy_required(&tool_name, &tool_args)
        .map_err(|e| format!("node {id} 工具参数校验失败（tool_name={tool_name}）：{e}"))?;

    Ok(WorkflowNodeSpec {
        id,
        tool_name,
        tool_args,
        deps,
        requires_approval,
        timeout_secs,
        compensate_with,
        max_retries,
        node_tool_role,
        run_if,
    })
}
