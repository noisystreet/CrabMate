//! `workflow_spec` v2（YAML/JSON 作者层）→ `workflow.nodes` + `for_each_pending` 编译。

use serde_json::{Map, Value};

use super::for_each_expand::{build_for_each_nodes, default_for_each_max_items};
use super::model::ForEachPendingSpec;

const MAX_STEPS: usize = 64;
const UNSUPPORTED_STEP_KEYS: &[&str] = &["repeat"];

struct CompileStepsOutput {
    nodes: Vec<Value>,
    for_each_pending: Vec<ForEachPendingSpec>,
}

/// 将含 `steps` 的作者层文档编译为 `{"workflow":{...,"nodes":[...]}}`；已是 `nodes` 则原样返回。
pub(crate) fn compile_workflow_author_value(mut root: Value) -> Result<Value, String> {
    let steps = take_steps(&mut root)?;
    let Some(steps_v) = steps else {
        return Ok(normalize_workflow_root(root));
    };

    let workflow_meta = root
        .get("workflow")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let flat = flatten_author_steps(&steps_v)?;
    let compiled = compile_flat_steps(&flat)?;
    let mut workflow_obj = workflow_meta.as_object().cloned().unwrap_or_default();
    if workflow_obj.contains_key("nodes") {
        return Err("workflow 不能同时包含 nodes 与 steps（请只保留其一）".to_string());
    }
    workflow_obj.remove("steps");
    workflow_obj.insert("nodes".to_string(), Value::Array(compiled.nodes));
    if !compiled.for_each_pending.is_empty() {
        let pending_json: Vec<Value> = compiled
            .for_each_pending
            .iter()
            .map(for_each_pending_to_json)
            .collect();
        workflow_obj.insert("for_each_pending".to_string(), Value::Array(pending_json));
    }
    Ok(Value::Object(Map::from_iter([(
        "workflow".to_string(),
        Value::Object(workflow_obj),
    )])))
}

pub(crate) fn compile_workflow_author_yaml(yaml: &str) -> Result<Value, String> {
    let root: Value =
        serde_yaml::from_str(yaml).map_err(|e| format!("workflow_spec YAML 解析失败: {e}"))?;
    compile_workflow_author_value(root)
}

fn normalize_workflow_root(root: Value) -> Value {
    if root.get("workflow").is_some() {
        return root;
    }
    if root.get("nodes").is_some() {
        return Value::Object(Map::from_iter([("workflow".to_string(), root)]));
    }
    root
}

fn take_steps(root: &mut Value) -> Result<Option<Value>, String> {
    if let Some(s) = root.get("steps").cloned() {
        if let Some(obj) = root.as_object_mut() {
            obj.remove("steps");
        }
        return Ok(Some(s));
    }
    let Some(wf) = root.get_mut("workflow") else {
        return Ok(None);
    };
    let Some(wf_obj) = wf.as_object_mut() else {
        return Err("workflow 必须是对象".to_string());
    };
    Ok(wf_obj.remove("steps"))
}

fn flatten_author_steps(steps_v: &Value) -> Result<Vec<Value>, String> {
    let arr = steps_v
        .as_array()
        .ok_or_else(|| "steps 必须是数组".to_string())?;
    let mut out = Vec::new();
    for (i, step) in arr.iter().enumerate() {
        flatten_one_step(step, i, None, &mut out)?;
    }
    Ok(out)
}

fn merge_choice_substep(
    sub: &Value,
    branch_when: Option<&Value>,
    shared_after: Option<&Value>,
) -> Value {
    let mut merged = sub.clone();
    let Some(m) = merged.as_object_mut() else {
        return merged;
    };
    if m.get("when").is_none()
        && let Some(w) = branch_when
    {
        m.insert("when".to_string(), w.clone());
    }
    if m.get("after").is_none()
        && let Some(a) = shared_after
    {
        m.insert("after".to_string(), a.clone());
    }
    merged
}

fn flatten_choice_step(
    obj: &Map<String, Value>,
    index: usize,
    inherited_when: Option<Value>,
    out: &mut Vec<Value>,
) -> Result<(), String> {
    let shared_after = obj.get("after");
    let branches = obj
        .get("branches")
        .and_then(|x| x.as_array())
        .ok_or_else(|| format!("steps[{index}] choice 缺少 branches"))?;
    for (bi, branch) in branches.iter().enumerate() {
        let bobj = branch
            .as_object()
            .ok_or_else(|| format!("steps[{index}].branches[{bi}] 必须是对象"))?;
        let branch_when = bobj.get("when");
        let nested = bobj
            .get("steps")
            .and_then(|x| x.as_array())
            .ok_or_else(|| format!("steps[{index}].branches[{bi}] 缺少 steps"))?;
        for (si, sub) in nested.iter().enumerate() {
            let merged = merge_choice_substep(sub, branch_when, shared_after);
            flatten_one_step(
                &merged,
                index * 1000 + bi * 100 + si,
                inherited_when.clone(),
                out,
            )?;
        }
    }
    Ok(())
}

fn flatten_one_step(
    step: &Value,
    index: usize,
    inherited_when: Option<Value>,
    out: &mut Vec<Value>,
) -> Result<(), String> {
    let obj = step
        .as_object()
        .ok_or_else(|| format!("steps[{index}] 必须是对象"))?;
    if obj.get("kind").and_then(|x| x.as_str()) == Some("choice") {
        return flatten_choice_step(obj, index, inherited_when, out);
    }
    let mut merged = step.clone();
    if let (Some(w), Some(m)) = (inherited_when, merged.as_object_mut())
        && m.get("when").is_none()
    {
        m.insert("when".to_string(), w);
    }
    out.push(merged);
    Ok(())
}

fn compile_for_each_author_step(
    obj: &Map<String, Value>,
    step_index: usize,
    seen_ids: &mut std::collections::HashSet<String>,
    nodes: &mut Vec<Value>,
    for_each_pending: &mut Vec<ForEachPendingSpec>,
) -> Result<(), String> {
    let spec = parse_for_each_step(obj, step_index)?;
    if !seen_ids.insert(spec.base_id.clone()) {
        return Err(format!("steps 中重复的 id: {}", spec.base_id));
    }
    if let Some(items) = &spec.static_items {
        for n in build_for_each_nodes(&spec, items) {
            nodes.push(workflow_node_spec_to_json(&n));
        }
    } else {
        for_each_pending.push(spec);
    }
    Ok(())
}

fn compile_plain_author_step(
    obj: &Map<String, Value>,
    step_index: usize,
    seen_ids: &mut std::collections::HashSet<String>,
    nodes: &mut Vec<Value>,
) -> Result<(), String> {
    let id = obj
        .get("id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}] 缺少 id"))?
        .to_string();
    if !seen_ids.insert(id.clone()) {
        return Err(format!("steps 中重复的 id: {id}"));
    }
    let mut node = compile_plain_step_node(obj, step_index)?;
    if let Some(when_v) = obj.get("when") {
        let run_if = parse_when_to_run_if_json(when_v)?;
        node.insert("run_if".to_string(), run_if);
    }
    nodes.push(Value::Object(node));
    Ok(())
}

fn compile_flat_steps(steps: &[Value]) -> Result<CompileStepsOutput, String> {
    if steps.is_empty() {
        return Err("steps 不能为空".to_string());
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("steps 超过上限 {MAX_STEPS}"));
    }
    for (i, step) in steps.iter().enumerate() {
        reject_unsupported_step_keys(step, i)?;
    }

    let mut seen_ids = std::collections::HashSet::<String>::new();
    let mut nodes = Vec::new();
    let mut for_each_pending = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let obj = step
            .as_object()
            .ok_or_else(|| format!("steps[{i}] 必须是对象"))?;
        if obj.contains_key("for_each") {
            compile_for_each_author_step(obj, i, &mut seen_ids, &mut nodes, &mut for_each_pending)?;
        } else {
            compile_plain_author_step(obj, i, &mut seen_ids, &mut nodes)?;
        }
    }

    validate_after_refs(&nodes)?;
    Ok(CompileStepsOutput {
        nodes,
        for_each_pending,
    })
}

fn reject_unsupported_step_keys(step: &Value, index: usize) -> Result<(), String> {
    let obj = step
        .as_object()
        .ok_or_else(|| format!("steps[{index}] 必须是对象"))?;
    for key in UNSUPPORTED_STEP_KEYS {
        if obj.contains_key(*key) {
            return Err(format!("steps[{index}] 含未实现字段 `{key}`"));
        }
    }
    Ok(())
}

fn compile_plain_step_node(
    obj: &Map<String, Value>,
    step_index: usize,
) -> Result<Map<String, Value>, String> {
    let tool_name = obj
        .get("tool")
        .or_else(|| obj.get("tool_name"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}] 缺少 tool"))?
        .to_string();
    let tool_args = obj
        .get("args")
        .or_else(|| obj.get("tool_args"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let deps = parse_after_deps(obj.get("after"), step_index)?;
    let id = obj
        .get("id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}] 缺少 id"))?
        .to_string();

    let mut node = Map::new();
    node.insert("id".to_string(), Value::String(id));
    node.insert("tool".to_string(), Value::String(tool_name));
    node.insert("tool_args".to_string(), tool_args);
    node.insert(
        "deps".to_string(),
        Value::Array(deps.into_iter().map(Value::String).collect()),
    );
    copy_optional_step_field(obj, &mut node, "requires_approval");
    copy_optional_step_field(obj, &mut node, "timeout_secs");
    copy_optional_step_field(obj, &mut node, "compensate_with");
    copy_optional_step_field(obj, &mut node, "max_retries");
    copy_optional_step_field(obj, &mut node, "node_tool_role");
    copy_optional_step_field(obj, &mut node, "executor_kind");
    Ok(node)
}

fn parse_when_to_run_if_json(when_v: &Value) -> Result<Value, String> {
    let mut wrapper = Map::new();
    if when_v.get("branch").is_some() {
        wrapper.insert(
            "from".to_string(),
            when_v.get("from").cloned().unwrap_or(Value::Null),
        );
        wrapper.insert(
            "branch".to_string(),
            when_v.get("branch").cloned().unwrap_or(Value::Null),
        );
        return Ok(Value::Object(wrapper));
    }
    if let Some(m) = when_v.get("match") {
        wrapper.insert(
            "from".to_string(),
            when_v.get("from").cloned().unwrap_or(Value::Null),
        );
        wrapper.insert("match".to_string(), m.clone());
        return Ok(Value::Object(wrapper));
    }
    Err("when 须含 branch 或 match".to_string())
}

struct ForEachLoopConfig {
    from: String,
    json_path: Option<String>,
    static_items: Option<Vec<String>>,
    item_var: String,
    max_items: usize,
    parallel: bool,
}

fn parse_for_each_loop_config(
    fe: &Map<String, Value>,
    step_index: usize,
) -> Result<ForEachLoopConfig, String> {
    let from = fe
        .get("from")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}].for_each 缺少 from"))?
        .to_string();
    let max_items = fe
        .get("max_items")
        .and_then(|x| x.as_u64())
        .map(|n| n as usize)
        .unwrap_or(default_for_each_max_items())
        .min(64);
    if max_items == 0 {
        return Err(format!("steps[{step_index}].for_each.max_items 须 ≥ 1"));
    }
    let json_path = fe
        .get("json_path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let static_items = fe
        .get("static_items")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        });
    if json_path.is_none() && static_items.as_ref().is_none_or(|v| v.is_empty()) {
        return Err(format!(
            "steps[{step_index}].for_each 须提供 json_path 或 static_items"
        ));
    }
    let item_var = fe
        .get("item_var")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("item")
        .to_string();
    let parallel = fe
        .get("parallel")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    Ok(ForEachLoopConfig {
        from,
        json_path,
        static_items,
        item_var,
        max_items,
        parallel,
    })
}

fn parse_for_each_step(
    obj: &Map<String, Value>,
    step_index: usize,
) -> Result<ForEachPendingSpec, String> {
    let base_id = obj
        .get("id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}] for_each 缺少 id"))?
        .to_string();
    let fe = obj
        .get("for_each")
        .and_then(|x| x.as_object())
        .ok_or_else(|| format!("steps[{step_index}].for_each 必须是对象"))?;
    let loop_cfg = parse_for_each_loop_config(fe, step_index)?;
    let tool_name = obj
        .get("tool")
        .or_else(|| obj.get("tool_name"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("steps[{step_index}] 缺少 tool"))?
        .to_string();
    let tool_args_template = obj
        .get("args")
        .or_else(|| obj.get("tool_args"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let extra_deps = parse_after_deps(obj.get("after"), step_index)?;

    Ok(ForEachPendingSpec {
        base_id,
        from: loop_cfg.from,
        json_path: loop_cfg.json_path,
        static_items: loop_cfg.static_items,
        item_var: loop_cfg.item_var,
        max_items: loop_cfg.max_items,
        parallel: loop_cfg.parallel,
        tool_name,
        tool_args_template,
        requires_approval: obj
            .get("requires_approval")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        timeout_secs: obj.get("timeout_secs").and_then(|x| x.as_u64()),
        compensate_with: obj
            .get("compensate_with")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        max_retries: obj
            .get("max_retries")
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
            .min(5) as u32,
        node_tool_role: obj
            .get("node_tool_role")
            .or_else(|| obj.get("executor_kind"))
            .and_then(|x| serde_json::from_value(x.clone()).ok()),
        extra_deps,
    })
}

fn workflow_node_spec_to_json(n: &super::model::WorkflowNodeSpec) -> Value {
    let mut node = Map::new();
    node.insert("id".to_string(), Value::String(n.id.clone()));
    node.insert("tool".to_string(), Value::String(n.tool_name.clone()));
    node.insert("tool_args".to_string(), n.tool_args.clone());
    node.insert(
        "deps".to_string(),
        Value::Array(n.deps.iter().cloned().map(Value::String).collect()),
    );
    if n.requires_approval {
        node.insert("requires_approval".to_string(), Value::Bool(true));
    }
    if let Some(t) = n.timeout_secs {
        node.insert("timeout_secs".to_string(), Value::Number(t.into()));
    }
    if !n.compensate_with.is_empty() {
        node.insert(
            "compensate_with".to_string(),
            Value::Array(
                n.compensate_with
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if n.max_retries > 0 {
        node.insert(
            "max_retries".to_string(),
            Value::Number(n.max_retries.into()),
        );
    }
    Value::Object(node)
}

fn for_each_pending_to_json(spec: &ForEachPendingSpec) -> Value {
    let mut m = Map::new();
    m.insert("base_id".to_string(), Value::String(spec.base_id.clone()));
    m.insert("from".to_string(), Value::String(spec.from.clone()));
    if let Some(p) = &spec.json_path {
        m.insert("json_path".to_string(), Value::String(p.clone()));
    }
    m.insert("item_var".to_string(), Value::String(spec.item_var.clone()));
    m.insert(
        "max_items".to_string(),
        Value::Number(spec.max_items.into()),
    );
    m.insert("parallel".to_string(), Value::Bool(spec.parallel));
    m.insert("tool".to_string(), Value::String(spec.tool_name.clone()));
    m.insert("tool_args".to_string(), spec.tool_args_template.clone());
    m.insert(
        "extra_deps".to_string(),
        Value::Array(spec.extra_deps.iter().cloned().map(Value::String).collect()),
    );
    Value::Object(m)
}

fn parse_after_deps(after_v: Option<&Value>, step_index: usize) -> Result<Vec<String>, String> {
    let Some(v) = after_v else {
        return Ok(Vec::new());
    };
    let arr = v
        .as_array()
        .ok_or_else(|| format!("steps[{step_index}].after 必须是字符串数组"))?;
    let mut deps = Vec::new();
    for item in arr {
        let s = item
            .as_str()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .ok_or_else(|| format!("steps[{step_index}].after 含非字符串项"))?;
        deps.push(s.to_string());
    }
    Ok(deps)
}

fn validate_after_refs(nodes: &[Value]) -> Result<(), String> {
    let ids: std::collections::HashSet<&str> = nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(|x| x.as_str()))
        .collect();
    for n in nodes {
        let id = n.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        if let Some(deps) = n.get("deps").and_then(|x| x.as_array()) {
            for d in deps {
                let dep = d
                    .as_str()
                    .ok_or_else(|| format!("节点 {id} deps 含非法项"))?;
                if !ids.contains(dep) {
                    return Err(format!("节点 {id} 的 after 引用未知 id: {dep}"));
                }
            }
        }
    }
    Ok(())
}

fn copy_optional_step_field(from: &Map<String, Value>, to: &mut Map<String, Value>, key: &str) {
    if let Some(v) = from.get(key) {
        to.insert(key.to_string(), v.clone());
    }
}
