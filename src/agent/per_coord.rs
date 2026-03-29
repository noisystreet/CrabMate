//! 规划–执行–反思（PER）协调：workflow 反思状态机 + 最终回答中的「规划」校验。
//! Web 与 CLI 的 `run_agent_turn` 共用此层，避免双份维护。

use crate::types::Message;

use super::plan_artifact;
use super::workflow_reflection_controller::{self, WorkflowReflectionController};
use serde_json::Value;

const PLAN_REWRITE_EXHAUSTED_SSE: &str =
    "结构化规划仍未满足要求（已达最大重写次数），已结束本轮；请调整需求后重试。";

/// 何时要求模型在**最终** assistant 正文中嵌入可解析的 `agent_reply_plan` v1（见 `plan_artifact`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalPlanRequirementMode {
    /// 从不强制；工作流反思仍会注入指令，但不触发 `after_final_assistant` 的重写循环。
    Never,
    /// 默认：仅当本轮工具路径注入了 [`workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`] 时，对随后的终答校验。
    #[default]
    WorkflowReflection,
    /// 每次模型以非 `tool_calls` 结束时均校验（实验性，易增加额外模型轮次）。
    Always,
}

impl FinalPlanRequirementMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "never" => Ok(Self::Never),
            "workflow_reflection" => Ok(Self::WorkflowReflection),
            "always" => Ok(Self::Always),
            _ => Err(format!(
                "未知 final_plan_requirement {:?}，应为 never / workflow_reflection / always",
                s.trim()
            )),
        }
    }
}

/// 标识 plan 需求的来源，使工作流反思与终答反思的交互点可审计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanRequirementSource {
    /// 无需求
    None,
    /// 来自 `FinalPlanRequirementMode::Always` 配置
    ConfigAlways,
    /// 来自工作流反思第一轮注入的 `INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`
    WorkflowReflection,
}

fn plan_rewrite_user_text() -> String {
    format!(
        "你的最终回答缺少**结构化规划**。请在 content 中加入一段 Markdown 代码围栏（语言标记为 json），其内为合法 JSON，且必须满足：\n{}\n\n示例：\n```json\n{}\n```\n\n请直接重写本轮最终回答（可有其它说明文字，但须包含上述 JSON 围栏）。",
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON
    )
}

/// 模型返回最终文本（非 tool_calls）后，由协调层决定是结束本轮还是要求重写。
#[derive(Debug)]
pub enum AfterFinalAssistant {
    /// 结束 `run_agent_turn` 外层的本次循环
    StopTurn,
    /// 追加一条 user 消息并继续请求模型
    RequestPlanRewrite(Message),
    /// 已达 `plan_rewrite_max_attempts` 且规划仍不合格；assistant 已在 `messages` 中，由运行时发 SSE 后结束。
    StopTurnPlanRewriteExhausted,
}

/// `workflow_execute` 经反思控制器处理后的结果：要么执行补丁后的参数，要么直接返回跳过结果字符串。
#[derive(Debug)]
pub struct PreparedWorkflowExecute {
    pub patched_args: String,
    pub execute: bool,
    /// 当 `execute == false` 时作为 tool 结果内容
    pub skipped_result: String,
    pub reflection_inject: Option<Value>,
}

/// Web / CLI 共用的 PER 状态。
pub struct PerCoordinator {
    reflection: WorkflowReflectionController,
    final_plan_policy: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    /// 在 [`FinalPlanRequirementMode::WorkflowReflection`] 下，由 `prepare_workflow_execute` 根据反思注入置位。
    plan_requirement_source: PlanRequirementSource,
    plan_rewrite_attempts: usize,
    /// 缓存 [`last_workflow_validate_layer_count`]：`messages.len()` 未变时复用上一次的扫描结果。
    /// [`Self::append_tool_result_and_reflection`] 在追加后按新历史重算；[`Self::invalidate_workflow_validate_layer_cache_after_context_mutation`] 在上下文裁剪/摘要后清空，避免误用旧值。
    cached_workflow_validate_layer_count: Option<usize>,
    layer_count_cache_at_message_len: usize,
}

impl PerCoordinator {
    pub fn plan_rewrite_exhausted_sse_message() -> &'static str {
        PLAN_REWRITE_EXHAUSTED_SSE
    }

    /// 供 `/status` 等只读镜像：`after_final_assistant` 已递增后的重写次数。
    pub fn plan_rewrite_attempts_snapshot(&self) -> usize {
        self.plan_rewrite_attempts
    }

    /// 下一回模型若以非 `tool_calls` 结束，是否必须嵌入可解析的 `agent_reply_plan`（工作流反思路径下由工具结果置位）。
    pub fn require_plan_in_final_flag_snapshot(&self) -> bool {
        self.plan_requirement_source != PlanRequirementSource::None
    }

    pub fn new(
        reflection_default_max_rounds: usize,
        final_plan_policy: FinalPlanRequirementMode,
        plan_rewrite_max_attempts: usize,
    ) -> Self {
        let initial_source = match final_plan_policy {
            FinalPlanRequirementMode::Always => PlanRequirementSource::ConfigAlways,
            _ => PlanRequirementSource::None,
        };
        Self {
            reflection: WorkflowReflectionController::new(reflection_default_max_rounds),
            final_plan_policy,
            plan_rewrite_max_attempts: plan_rewrite_max_attempts.max(1),
            plan_requirement_source: initial_source,
            plan_rewrite_attempts: 0,
            cached_workflow_validate_layer_count: None,
            layer_count_cache_at_message_len: 0,
        }
    }

    /// `context_window` 在裁剪/摘要等**就地**改写 `messages` 后调用，避免 `layer_count` 缓存指向已删除的 `workflow_validate` 工具结果。
    pub fn invalidate_workflow_validate_layer_cache_after_context_mutation(&mut self) {
        self.cached_workflow_validate_layer_count = None;
        self.layer_count_cache_at_message_len = 0;
    }

    fn workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        let len = messages.len();
        if len != self.layer_count_cache_at_message_len {
            let n = last_workflow_validate_layer_count(messages);
            self.cached_workflow_validate_layer_count = n;
            self.layer_count_cache_at_message_len = len;
            return n;
        }
        if self.cached_workflow_validate_layer_count.is_some() {
            return self.cached_workflow_validate_layer_count;
        }
        let n = last_workflow_validate_layer_count(messages);
        self.cached_workflow_validate_layer_count = n;
        self.layer_count_cache_at_message_len = len;
        n
    }

    /// 是否包含可解析的 `agent_reply_plan` v1 JSON（见 `plan_artifact`）。
    #[allow(dead_code)] // 供外部集成或调试保留；主路径用 `parse_agent_reply_plan_v1`
    pub fn content_has_plan(content: &str) -> bool {
        plan_artifact::content_has_valid_agent_reply_plan_v1(content)
    }

    /// 在已将 assistant 消息推入 `messages` 之后调用，根据是否需要「规划」段落决定下一步。
    pub fn after_final_assistant(
        &mut self,
        msg: &Message,
        messages: &[Message],
    ) -> AfterFinalAssistant {
        let require_plan = match self.final_plan_policy {
            FinalPlanRequirementMode::Never => false,
            FinalPlanRequirementMode::WorkflowReflection => {
                self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
            }
            FinalPlanRequirementMode::Always => true,
        };

        log::info!(
            target: "crabmate::per",
            "after_final_assistant enter policy={:?} require_plan={} plan_requirement_source={:?} reflection_stage_round={} plan_rewrite_attempts={} plan_rewrite_max={}",
            self.final_plan_policy,
            require_plan,
            self.plan_requirement_source,
            self.reflection.stage_round(),
            self.plan_rewrite_attempts,
            self.plan_rewrite_max_attempts
        );

        if !require_plan {
            log::info!(
                target: "crabmate::per",
                "after_final_assistant outcome=stop_no_requirement"
            );
            return AfterFinalAssistant::StopTurn;
        }

        let apply_layer_semantics = match self.final_plan_policy {
            FinalPlanRequirementMode::Never => false,
            FinalPlanRequirementMode::WorkflowReflection => {
                self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
            }
            FinalPlanRequirementMode::Always => true,
        };
        let layer_need = self.workflow_validate_layer_need(messages);

        let content = msg.content.as_deref().unwrap_or("");
        if let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1(content) {
            let layers_ok = match layer_need {
                Some(n) if n > 0 && apply_layer_semantics => plan.steps.len() >= n,
                _ => true,
            };
            let workflow_ids_ok = match last_workflow_tool_node_ids(messages) {
                Some(ids) => {
                    plan_artifact::validate_plan_workflow_node_ids_subset(&plan, &ids).is_ok()
                }
                None => true,
            };
            if layers_ok && workflow_ids_ok {
                log::info!(
                    target: "crabmate::per",
                    "after_final_assistant outcome=stop_plan_ok plan_steps={} layer_need={:?}",
                    plan.steps.len(),
                    layer_need
                );
                return AfterFinalAssistant::StopTurn;
            }
            log::info!(
                target: "crabmate::per",
                "after_final_assistant outcome=plan_schema_ok_semantics_fail plan_steps={} layer_need={:?} workflow_node_ids_ok={}",
                plan.steps.len(),
                layer_need,
                workflow_ids_ok
            );
        }

        if self.plan_rewrite_attempts >= self.plan_rewrite_max_attempts {
            log::warn!(
                target: "crabmate::per",
                "after_final_assistant outcome=plan_rewrite_exhausted layer_need={:?}",
                layer_need
            );
            return AfterFinalAssistant::StopTurnPlanRewriteExhausted;
        }
        self.plan_rewrite_attempts += 1;
        let rewrite_text = match (
            layer_need.filter(|&n| n > 0 && apply_layer_semantics),
            last_workflow_tool_node_ids(messages),
        ) {
            (Some(n), Some(ids)) if !ids.is_empty() => format!(
                "{}\n\n补充：\n- 最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。\n- 若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与 `nodes[].id` 对齐）：{}。",
                plan_rewrite_user_text(),
                ids.join(", ")
            ),
            (Some(n), _) => format!(
                "{}\n\n补充：最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。",
                plan_rewrite_user_text()
            ),
            (None, Some(ids)) if !ids.is_empty() => format!(
                "{}\n\n补充：若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与最近一次 `workflow_execute` 工具结果中 `nodes[].id` 对齐）：{}。",
                plan_rewrite_user_text(),
                ids.join(", ")
            ),
            (None, _) => plan_rewrite_user_text(),
        };
        log::info!(
            target: "crabmate::per",
            "after_final_assistant outcome=request_plan_rewrite attempt={} layer_need={:?}",
            self.plan_rewrite_attempts,
            layer_need
        );
        AfterFinalAssistant::RequestPlanRewrite(Message {
            role: "user".to_string(),
            content: Some(rewrite_text),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        })
    }

    /// 对一次 `workflow_execute` 的 arguments 做反思决策、补丁与「要求最终带规划」标记更新。
    pub fn prepare_workflow_execute(&mut self, args_json: &str) -> PreparedWorkflowExecute {
        let decision = self.reflection.decide(args_json);
        let reflection_inject = decision.inject_instruction.clone();
        if matches!(
            self.final_plan_policy,
            FinalPlanRequirementMode::WorkflowReflection
        ) && let Some(v) = reflection_inject.as_ref()
            && v.get("instruction_type").and_then(|x| x.as_str())
                == Some(workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT)
        {
            self.plan_requirement_source = PlanRequirementSource::WorkflowReflection;
            log::info!(
                target: "crabmate::per",
                "prepare_workflow_execute event=require_final_plan_set reflection_stage_round={}",
                self.reflection.stage_round()
            );
        }
        let patched_args = match decision.workflow_args_patch.as_ref() {
            Some(patch) => workflow_reflection_controller::apply_workflow_patch(args_json, patch),
            None => args_json.to_string(),
        };
        let skipped_result = if decision.execute {
            String::new()
        } else {
            stop_output_to_string(decision.stop_output)
        };
        PreparedWorkflowExecute {
            patched_args,
            execute: decision.execute,
            skipped_result,
            reflection_inject,
        }
    }

    /// 追加 tool 消息以及可选的反思注入 user 消息（与原先两处 `run_agent_turn*` 行为一致）。
    pub fn append_tool_result_and_reflection(
        per_coord: &mut PerCoordinator,
        messages: &mut Vec<Message>,
        tool_call_id: String,
        result: String,
        reflection_inject: Option<Value>,
    ) {
        messages.push(Message {
            role: "tool".to_string(),
            content: Some(result),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some(tool_call_id),
        });
        if let Some(instruction) = reflection_inject {
            let instruction_str = match serde_json::to_string(&instruction) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        target: "crabmate::per",
                        "workflow_reflection_inject_serialize_failed error={}",
                        e
                    );
                    r#"{"instruction_type":"crabmate_reflection_serialize_failed","detail":"工作流反思指令无法序列化为 JSON，请根据上一轮工具结果继续。"}"#
                        .to_string()
                }
            };
            // 与 `prepare_workflow_execute` 一致：反思首轮要求终答含 `agent_reply_plan`；后续轮次注入也应保持同一策略，避免模型收到「须带规划」文案却未置位 PER。
            if matches!(
                per_coord.final_plan_policy,
                FinalPlanRequirementMode::WorkflowReflection
            ) && let Some(t) = instruction.get("instruction_type").and_then(|x| x.as_str())
                && (t == workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT
                    || t == "workflow_reflection_next")
            {
                per_coord.plan_requirement_source = PlanRequirementSource::WorkflowReflection;
                log::info!(
                    target: "crabmate::per",
                    "append_tool_result_and_reflection event=require_final_plan_set instruction_type={t} reflection_stage_round={}",
                    per_coord.reflection.stage_round()
                );
            }
            messages.push(Message {
                role: "user".to_string(),
                content: Some(instruction_str),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            });
        }
        per_coord.layer_count_cache_at_message_len = messages.len();
        per_coord.cached_workflow_validate_layer_count =
            last_workflow_validate_layer_count(messages.as_slice());
    }
}

#[cfg(test)]
impl PerCoordinator {
    fn test_workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        self.workflow_validate_layer_need(messages)
    }

    fn test_layer_cache_snapshot(&self) -> (Option<usize>, usize) {
        (
            self.cached_workflow_validate_layer_count,
            self.layer_count_cache_at_message_len,
        )
    }
}

/// 从对话历史中取**最近一次** `workflow_execute` 工具结果中的 `nodes[].id`（`workflow_validate_result` / `workflow_execute_result`）。
fn last_workflow_tool_node_ids(messages: &[Message]) -> Option<Vec<String>> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let name = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)
            .map(|c| c.function.name.as_str())?;
        if name != "workflow_execute" {
            continue;
        }
        let body = m.content.as_deref()?;
        let payload = crate::tool_result::tool_message_payload_for_inner_parse(body);
        let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
        let rt = v.get("report_type").and_then(|x| x.as_str());
        if !matches!(
            rt,
            Some("workflow_validate_result") | Some("workflow_execute_result")
        ) {
            continue;
        }
        let nodes = v.get("nodes").and_then(|x| x.as_array())?;
        let mut ids = Vec::new();
        for n in nodes {
            let id = n.get("id").and_then(|x| x.as_str())?;
            ids.push(id.to_string());
        }
        if ids.is_empty() {
            continue;
        }
        return Some(ids);
    }
    None
}

/// 从对话历史中取**最近一次** `workflow_execute` 的 `workflow_validate_only` 工具结果里的 `spec.layer_count`。
fn last_workflow_validate_layer_count(messages: &[Message]) -> Option<usize> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let name = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)
            .map(|c| c.function.name.as_str())?;
        if name != "workflow_execute" {
            continue;
        }
        let body = m.content.as_deref()?;
        let payload = crate::tool_result::tool_message_payload_for_inner_parse(body);
        let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
        if v.get("report_type").and_then(|x| x.as_str()) != Some("workflow_validate_result") {
            continue;
        }
        let n = v
            .get("spec")
            .and_then(|s| s.get("layer_count"))
            .and_then(|x| x.as_u64())? as usize;
        return Some(n);
    }
    None
}

fn assistant_index_for_tool_call(
    messages: &[Message],
    tool_idx: usize,
    tool_call_id: &str,
) -> Option<usize> {
    for j in (0..tool_idx).rev() {
        if messages[j].role != "assistant" {
            continue;
        }
        let calls = messages[j].tool_calls.as_ref()?;
        if calls.iter().any(|c| c.id == tool_call_id) {
            return Some(j);
        }
    }
    None
}

fn stop_output_to_string(stop_output: Option<Value>) -> String {
    let stop_v = stop_output.unwrap_or_else(|| {
        Value::String("workflow_execute 已停止（反思控制器拒绝继续执行）。".to_string())
    });
    match stop_v {
        Value::String(s) => s,
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall};

    #[test]
    fn final_assistant_rewrites_then_stops() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan here".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist: Vec<Message> = vec![];
        assert!(matches!(
            c.after_final_assistant(&empty, &hist),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &hist),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &hist),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted
        ));
    }

    #[test]
    fn final_assistant_stops_when_plan_present() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"step"}]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok, &[]),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn plan_semantics_requires_enough_steps_vs_validate_layers() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 3);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3}}"#
                        .to_string(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let one_step = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"only","description":"only one step here"}]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&one_step, &hist),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let three_steps = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"a","description":"layer 0"},
  {"id":"b","description":"layer 1"},
  {"id":"c","description":"layer 2"}
]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&three_steps, &hist),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn plan_workflow_node_id_must_match_last_workflow_nodes() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 3);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"fmt"},{"id":"test"}]}"#
                        .to_string(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let bad_link = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"step1","description":"run fmt","workflow_node_id":"no-such-node"}]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&bad_link, &hist),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let ok_link = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"step1","description":"run fmt","workflow_node_id":"fmt"}]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok_link, &hist),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn prepare_workflow_first_round_injects_plan_next() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let prep = c.prepare_workflow_execute(args);
        assert!(prep.execute);
        assert!(prep.skipped_result.is_empty());
        let ty = prep
            .reflection_inject
            .as_ref()
            .and_then(|v| v.get("instruction_type"))
            .and_then(|x| x.as_str());
        assert_eq!(
            ty,
            Some(workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT)
        );
    }

    #[test]
    fn never_policy_skips_plan_rewrite_even_after_workflow_reflection_inject() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan here".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn always_policy_requests_rewrite_without_workflow() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Always, 2);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
    }

    #[test]
    fn always_policy_stops_when_plan_present() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Always, 2);
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"Here is my plan:
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"do the thing"}]}
```"#
                    .to_string(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok, &[]),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn always_policy_exhausts_rewrites_then_stops() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Always, 1);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan at all".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted
        ));
    }

    #[test]
    fn workflow_reflection_no_inject_means_no_requirement() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn workflow_reflection_done_true_does_not_set_plan_requirement() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args_round1 =
            r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args_round1);
        assert!(c.require_plan_in_final_flag_snapshot());

        let wf_args_done =
            r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":true}}"#;
        let prep = c.prepare_workflow_execute(wf_args_done);
        assert!(prep.execute);
    }

    #[test]
    fn prepare_workflow_reflection_disabled_passes_through() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        let args = r#"{"workflow":{"nodes":[{"id":"a","tool":"ls"}]}}"#;
        let prep = c.prepare_workflow_execute(args);
        assert!(prep.execute);
        assert!(prep.reflection_inject.is_none());
        assert!(!c.require_plan_in_final_flag_snapshot());
    }

    #[test]
    fn plan_rewrite_attempts_increments_correctly() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Always, 3);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan".to_string()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 0);
        let _ = c.after_final_assistant(&empty, &[]);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 1);
        let _ = c.after_final_assistant(&empty, &[]);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 2);
        let _ = c.after_final_assistant(&empty, &[]);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 3);
        assert!(matches!(
            c.after_final_assistant(&empty, &[]),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted
        ));
    }

    #[test]
    fn append_tool_result_and_reflection_with_inject() {
        let mut msgs: Vec<Message> = vec![];
        let inject = serde_json::json!({
            "instruction_type": "test_instruction",
            "body": "do something"
        });
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 2);
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-99".to_string(),
            "tool output".to_string(),
            Some(inject),
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "tool");
        assert_eq!(msgs[0].content.as_deref(), Some("tool output"));
        assert_eq!(msgs[0].tool_call_id.as_deref(), Some("tc-99"));
        assert_eq!(msgs[1].role, "user");
        assert!(
            msgs[1]
                .content
                .as_deref()
                .unwrap()
                .contains("test_instruction")
        );
    }

    #[test]
    fn workflow_reflection_next_inject_sets_plan_requirement_source() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::WorkflowReflection, 2);
        assert!(!c.require_plan_in_final_flag_snapshot());
        let mut msgs: Vec<Message> = vec![];
        let inject = serde_json::json!({
            "instruction_type": "workflow_reflection_next",
            "round": 2
        });
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-wf".to_string(),
            "ok".to_string(),
            Some(inject),
        );
        assert!(c.require_plan_in_final_flag_snapshot());
    }

    #[test]
    fn append_tool_result_without_reflection() {
        let mut msgs: Vec<Message> = vec![];
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 2);
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-1".to_string(),
            "result".to_string(),
            None,
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "tool");
    }

    #[test]
    fn layer_count_from_empty_history() {
        assert_eq!(last_workflow_validate_layer_count(&[]), None);
    }

    #[test]
    fn layer_count_from_non_workflow_history() {
        let msgs = vec![
            Message {
                role: "user".to_string(),
                content: Some("hello".to_string()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some("hi".to_string()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        assert_eq!(last_workflow_validate_layer_count(&msgs), None);
    }

    fn hist_with_validate_layer_3() -> Vec<Message> {
        vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3}}"#
                        .to_string(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ]
    }

    #[test]
    fn workflow_validate_layer_cache_reuses_when_len_unchanged() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 2);
        let hist = hist_with_validate_layer_3();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
        assert_eq!(c.test_layer_cache_snapshot(), (Some(3), hist.len()));
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
    }

    #[test]
    fn workflow_validate_layer_cache_invalidates_on_context_mutation() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 2);
        let mut hist = hist_with_validate_layer_3();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
        hist.pop();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), None);
        assert_eq!(c.test_layer_cache_snapshot(), (None, hist.len()));
    }

    #[test]
    fn workflow_validate_layer_from_crabmate_tool_envelope() {
        let inner = r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3}}"#;
        let parsed = crate::tool_result::parse_legacy_output("workflow_execute", inner);
        let wrapped = crate::tool_result::encode_tool_message_envelope_v1(
            "workflow_execute",
            "wf".into(),
            &parsed,
            inner,
            None,
        );
        let mut hist = hist_with_validate_layer_3();
        hist[1].content = Some(wrapped);
        assert_eq!(last_workflow_validate_layer_count(&hist), Some(3));
    }
}
