//! Workflow 反思（Review）控制器：将“反思阶段 -> 模型修订计划 -> 再执行”做成可测试、可复用的决策逻辑。

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
struct ReflectionControl {
    enabled: bool,
    done: bool,
    max_rounds: usize,
}

#[derive(Debug, Clone)]
pub struct WorkflowReflectionDecision {
    /// 是否真的调用 `workflow_execute`（DAG 调度由 `workflow.rs` 内部完成；当 done=true 时会被跳过）。
    pub execute: bool,
    /// 当 `execute=false` 时，作为 tool_result 返回给模型/用户的输出。
    pub stop_output: Option<Value>,
    /// 在 tool_result 消息之后，运行时应额外注入给模型的“下一步反思指令”。
    pub inject_instruction: Option<Value>,
    /// 运行时在调用 `workflow_execute` 时，对 `workflow` 字段进行的额外覆盖（例如 validate_only=true）。
    /// 如为 None 表示不修改模型传入的 args。
    pub workflow_args_patch: Option<Value>,
}

/// 状态机：activation -> stage(loop) -> (done | max_rounds -> locked)
#[derive(Debug, Clone)]
pub struct WorkflowReflectionController {
    mode_active: bool,
    locked: bool,
    max_rounds: usize,
    round: usize,
    default_max_rounds: usize,
}

impl WorkflowReflectionController {
    pub fn new(default_max_rounds: usize) -> Self {
        Self {
            mode_active: false,
            locked: false,
            max_rounds: default_max_rounds.max(1),
            round: 0,
            default_max_rounds: default_max_rounds.max(1),
        }
    }

    fn parse_control(&self, args_json: &str) -> ReflectionControl {
        let default = self.default_max_rounds;
        let enabled = false;
        let done = false;
        let max_rounds = default;

        let Ok(v) = serde_json::from_str::<Value>(args_json) else {
            return ReflectionControl {
                enabled,
                done,
                max_rounds,
            };
        };
        let wf_v = v.get("workflow").unwrap_or(&v);
        let enabled = wf_v
            .get("reflection")
            .and_then(|r| r.get("enabled"))
            .and_then(|x| x.as_bool())
            .unwrap_or(enabled);
        let done = wf_v.get("done").and_then(|x| x.as_bool()).unwrap_or(done);
        let max_rounds = wf_v
            .get("reflection")
            .and_then(|r| r.get("max_rounds"))
            .and_then(|x| x.as_u64())
            .map(|n| (n as usize).max(1))
            .unwrap_or(max_rounds);

        ReflectionControl {
            enabled,
            done,
            max_rounds,
        }
    }

    pub fn decide(&mut self, args_json: &str) -> WorkflowReflectionDecision {
        let control = self.parse_control(args_json);

        // 只有当 enabled=true 且尚未进入反思模式时，才激活本次反思会话。
        if control.enabled && !self.mode_active {
            self.mode_active = true;
            self.locked = false;
            self.max_rounds = control.max_rounds;
            self.round = 0;
        }

        if !self.mode_active {
            return WorkflowReflectionDecision {
                execute: true,
                stop_output: None,
                inject_instruction: None,
                workflow_args_patch: None,
            };
        }

        // 反思模式内：由模型的 done=true 决定结束；其余情况下按 max_rounds 兜底。
        if control.done {
            self.locked = true;
            return WorkflowReflectionDecision {
                execute: true,
                stop_output: None,
                inject_instruction: None,
                workflow_args_patch: None,
            };
        }

        if self.locked {
            // locked 状态且 done=false：不再执行 DAG，改为引导模型给出最终回复。
            return WorkflowReflectionDecision {
                execute: false,
                stop_output: Some(json!({
                    "type": "workflow_reflection_stop",
                    "instruction_type": "workflow_reflection_locked",
                    "max_rounds": self.max_rounds,
                    "human_summary": format!(
                        "workflow_execute 已停止：反思已锁定（max_rounds={}）。",
                        self.max_rounds
                    ),
                })),
                inject_instruction: Some(json!({
                    "instruction_type": "workflow_reflection_locked",
                    "max_rounds": self.max_rounds
                })),
                workflow_args_patch: None,
            };
        }

        if self.round >= self.max_rounds {
            self.locked = true;
            return WorkflowReflectionDecision {
                execute: false,
                stop_output: Some(json!({
                    "type": "workflow_reflection_stop",
                    "instruction_type": "workflow_reflection_max_rounds_reached",
                    "max_rounds": self.max_rounds,
                    "human_summary": format!(
                        "workflow_execute 已停止：达到反思重试上限（max_rounds={}）。",
                        self.max_rounds
                    ),
                })),
                inject_instruction: Some(json!({
                    "instruction_type": "workflow_reflection_max_rounds_reached",
                    "max_rounds": self.max_rounds
                })),
                workflow_args_patch: None,
            };
        }

        // 进入下一轮 stage：允许执行，并要求模型修订计划。
        self.round += 1;

        // 规划阶段：第 1 轮只做 validate_only，避免对工作区产生副作用。
        // 为避免模型在后续轮次仍错误设置 validate_only=true，这里显式覆盖：
        // - 第 1 轮：validate_only=true（Plan）
        // - 第 2+ 轮：validate_only=false（Do）
        let workflow_args_patch = Some(json!({
            "validate_only": self.round == 1
        }));

        WorkflowReflectionDecision {
            execute: true,
            stop_output: None,
            inject_instruction: Some(json!({
                "instruction_type": if self.round == 1 {
                    "workflow_reflection_plan_next"
                } else {
                    "workflow_reflection_next"
                },
                "round": self.round,
                "max_rounds": self.max_rounds,
                "required_model_action": if self.round == 1 {
                    "plan_from_validate_only_result: you MUST embed a ```json fenced block whose JSON is {\"type\":\"agent_reply_plan\",\"version\":1,\"steps\":[{\"id\":\"...\",\"description\":\"...\"},...]} with at least one step. Each description should reflect the validate_only tool_result (execution_layers, deps, required_approval, timeout_secs, compensate_with, mapping layer->nodes). Natural language outside the fence is OK. After producing the plan, call workflow_execute again with workflow.validate_only=false (Do stage), unless you already set workflow.done=true."
                } else {
                    "revise_workflow_then_call_workflow_execute_or_set_workflow_done_true_when_goal_reached"
                },
                "next_call_hint": {
                    "call_tool": "workflow_execute",
                    "workflow_done": false
                }
            })),
            workflow_args_patch,
        }
    }
}

/// 将 `workflow_args_patch` 应用到原始 `args_json` 中，并返回新的 args_json 字符串。
/// - 若 args_json 顶层包含 `workflow` 对象，则 patch 合并进 `workflow`
/// - 否则 patch 合并进顶层对象
pub fn apply_workflow_patch(args_json: &str, workflow_patch: &Value) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(args_json) else {
        return args_json.to_string();
    };
    let Some(patch_obj) = workflow_patch.as_object() else {
        return v.to_string();
    };

    // 优先写入 workflow 字段
    let target = v.get_mut("workflow").and_then(|w| w.as_object_mut());
    if let Some(t) = target {
        for (k, val) in patch_obj {
            t.insert(k.clone(), val.clone());
        }
        return v.to_string();
    }

    // 否则写入顶层
    if let Some(root) = v.as_object_mut() {
        for (k, val) in patch_obj {
            root.insert(k.clone(), val.clone());
        }
    }
    v.to_string()
}

/// Do 阶段契约校验：当 `workflow.validate_only != true` 时，
/// 在真正执行 DAG 之前，确保 workflow 的 nodes/依赖结构是一个可执行的基本形态。
///
/// 返回 Err(Value) 用于直接作为 tool_result 返回给模型（包含 human_summary）。
pub fn validate_workflow_execute_do_contract(args_json: &str) -> Result<(), Value> {
    let v: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => {
            return Err(json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": "workflow_execute_do_contract：参数不是合法 JSON"
            }));
        }
    };

    let wf_v = v.get("workflow").unwrap_or(&v);
    let validate_only = wf_v
        .get("validate_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    // Plan 阶段只做 validate，不检查契约
    if validate_only {
        return Ok(());
    }

    let nodes_v = match wf_v.get("nodes") {
        Some(n) => n,
        None => {
            return Err(json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": "Do 阶段必须提供 workflow.nodes（不能为空）"
            }));
        }
    };

    let mut node_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut nodes_as_iter: Vec<(&serde_json::Value, String)> = Vec::new();

    if let Some(arr) = nodes_v.as_array() {
        if arr.is_empty() {
            return Err(json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": "Do 阶段 workflow.nodes 不能为空"
            }));
        }
        for node in arr.iter() {
            let id = node
                .get("id")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let Some(id) = id else {
                return Err(json!({
                    "type": "workflow_execute_do_contract_error",
                    "human_summary": "Do 阶段 nodes 数组中的每个 node 都必须有字符串 id"
                }));
            };
            node_ids.insert(id.clone());
            nodes_as_iter.push((node, id));
        }
    } else if let Some(obj) = nodes_v.as_object() {
        if obj.is_empty() {
            return Err(json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": "Do 阶段 workflow.nodes 对象不能为空"
            }));
        }
        for (id, node) in obj.iter() {
            node_ids.insert(id.clone());
            nodes_as_iter.push((node, id.clone()));
        }
    } else {
        return Err(json!({
            "type": "workflow_execute_do_contract_error",
            "human_summary": "Do 阶段 workflow.nodes 必须是数组或对象"
        }));
    }

        // 验证 deps 形态 + deps 引用必须在 nodes_id 集合中
    for (node, id) in nodes_as_iter.iter() {
        let node_obj = node.as_object().ok_or_else(|| {
            json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": format!("node {} 必须是对象", id)
            })
        })?;

            // 与 parse_workflow_spec 保持一致：deps 缺失视为 []。
            let deps_values: Vec<Value> = match node_obj.get("deps") {
                None => Vec::new(),
                Some(dv) => {
                    dv.as_array()
                        .ok_or_else(|| {
                            json!({
                                "type": "workflow_execute_do_contract_error",
                                "human_summary": format!("node {} 的 deps 必须是数组", id)
                            })
                        })?
                        .clone()
                }
            };

            for dep in deps_values.iter() {
                let dep_id = dep.as_str().ok_or_else(|| {
                    json!({
                        "type": "workflow_execute_do_contract_error",
                        "human_summary": format!("node {} 的 deps 元素必须是字符串", id)
                    })
                })?;
                if !node_ids.contains(dep_id) {
                    return Err(json!({
                        "type": "workflow_execute_do_contract_error",
                        "human_summary": format!("node {} 的 deps 引用了未知节点 {}", id, dep_id)
                    }));
                }
            }

        // tool_name 必须存在，避免后续运行时 unknown tool
        let tool_name = node_obj
            .get("tool_name")
            .or_else(|| node_obj.get("tool"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if tool_name.trim().is_empty() {
            return Err(json!({
                "type": "workflow_execute_do_contract_error",
                "human_summary": format!("node {} 缺少 tool_name（或 tool）", id)
            }));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args(enabled: bool, done: bool, max_rounds: usize) -> String {
        format!(
            r#"{{
              "workflow": {{
                "reflection": {{"enabled": {}, "max_rounds": {}}},
                "done": {}
              }}
            }}"#,
            enabled, max_rounds, done
        )
    }

    #[test]
    fn test_activate_and_stage_injection_then_stop_on_max_rounds() {
        let mut c = WorkflowReflectionController::new(5);
        let max_rounds = 2;

        // round=0 -> first call => round becomes 1, execute=true, injection
        let d1 = c.decide(&base_args(true, false, max_rounds));
        assert!(d1.execute);
        assert!(d1.inject_instruction.is_some());
        assert!(d1.stop_output.is_none());
        assert!(
            d1.workflow_args_patch
                .as_ref()
                .and_then(|v| v.get("validate_only"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        );

        // second call => round becomes 2, execute=true
        let d2 = c.decide(&base_args(true, false, max_rounds));
        assert!(d2.execute);
        assert!(d2.inject_instruction.is_some());
        assert!(
            !d2.workflow_args_patch
                .as_ref()
                .and_then(|v| v.get("validate_only"))
                .and_then(|x| x.as_bool())
                .unwrap_or(true)
        );

        // third call => round>=max_rounds, stop and lock
        let d3 = c.decide(&base_args(true, false, max_rounds));
        assert!(!d3.execute);
        assert!(d3.stop_output.is_some());
        assert!(d3.inject_instruction.is_some());
        assert_eq!(
            d3.inject_instruction
                .as_ref()
                .and_then(|v| v.get("instruction_type"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "workflow_reflection_max_rounds_reached"
        );

        // subsequent call => locked stop (different stop text)
        let d4 = c.decide(&base_args(false, false, max_rounds));
        assert!(!d4.execute);
        assert_eq!(
            d4.stop_output
                .as_ref()
                .and_then(|v| v.get("instruction_type"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "workflow_reflection_locked"
        );
        assert_eq!(
            d4.inject_instruction
                .as_ref()
                .and_then(|v| v.get("instruction_type"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "workflow_reflection_locked"
        );
    }

    #[test]
    fn test_done_true_ends_without_injection() {
        let mut c = WorkflowReflectionController::new(5);

        let d1 = c.decide(&base_args(true, true, 3));
        assert!(d1.execute);
        assert!(d1.inject_instruction.is_none());
        assert!(d1.stop_output.is_none());

        // locked now, done=false -> stop
        let d2 = c.decide(&base_args(false, false, 3));
        assert!(!d2.execute);
    }

    #[test]
    fn test_validate_do_contract_requires_nodes_and_deps_shape() {
        // validate_only=false + nodes 缺失 => 错
        let bad_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":5},"done":false}}"#;
        let err = validate_workflow_execute_do_contract(bad_args).unwrap_err();
        assert_eq!(err.get("type").and_then(|v| v.as_str()).unwrap_or(""), "workflow_execute_do_contract_error");

        // nodes 存在但 deps 非数组 => 错
        let bad_deps = r#"{
          "workflow":{
            "validate_only":false,
            "nodes":[{"id":"a","tool_name":"calc","deps":"not_array"}]
          }
        }"#;
        let err = validate_workflow_execute_do_contract(bad_deps).unwrap_err();
        assert_eq!(
            err.get("type").and_then(|v| v.as_str()).unwrap_or(""),
            "workflow_execute_do_contract_error"
        );

        // deps 引用了未知节点 => 错
        let unknown_dep = r#"{
          "workflow":{
            "validate_only":false,
            "nodes":[
              {"id":"a","tool_name":"calc","deps":["b"]}
            ]
          }
        }"#;
        let err = validate_workflow_execute_do_contract(unknown_dep).unwrap_err();
        assert_eq!(
            err.get("type").and_then(|v| v.as_str()).unwrap_or(""),
            "workflow_execute_do_contract_error"
        );

        // 结构正确 => ok
        let ok_args = r#"{
          "workflow":{
            "validate_only":false,
            "nodes":[
              {"id":"a","tool_name":"calc","deps":[]}
            ]
          }
        }"#;
        assert!(validate_workflow_execute_do_contract(ok_args).is_ok());

        // deps 缺失 => 允许（按 parse_workflow_spec 视为 []）
        let ok_args_no_deps = r#"{
          "workflow":{
            "validate_only":false,
            "nodes":[
              {"id":"a","tool_name":"calc"}
            ]
          }
        }"#;
        assert!(validate_workflow_execute_do_contract(ok_args_no_deps).is_ok());
    }
}

