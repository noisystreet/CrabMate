//! 规划–执行–反思（PER）协调：workflow 反思状态机 + 最终回答中的「规划」校验。
//! Web 与 TUI 的 `run_agent_turn*` 共用此层，避免双份维护。

use crate::plan_artifact;
use crate::types::Message;
use crate::workflow_reflection_controller::{self, WorkflowReflectionController};
use serde_json::Value;

const PLAN_REWRITE_USER_TEXT: &str = r#"你的最终回答缺少**结构化规划**。请在 content 中加入一段 Markdown 代码围栏（语言标记为 json），其内为合法 JSON，且必须满足：
- 顶层 "type" 为字符串 "agent_reply_plan"
- "version" 为数字 1
- "steps" 为非空数组；每项含非空字符串 "id" 与 "description"

示例：
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"layer-0","description":"先执行无依赖节点 …"}]}
```

请直接重写本轮最终回答（可有其它说明文字，但须包含上述 JSON 围栏）。"#;

/// 模型返回最终文本（非 tool_calls）后，由协调层决定是结束本轮还是要求重写。
#[derive(Debug)]
pub enum AfterFinalAssistant {
    /// 结束 `run_agent_turn` 外层的本次循环
    StopTurn,
    /// 追加一条 user 消息并继续请求模型
    RequestPlanRewrite(Message),
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

/// Web / TUI 共用的 PER 状态。
pub struct PerCoordinator {
    reflection: WorkflowReflectionController,
    require_plan_in_final_content: bool,
    plan_rewrite_attempts: usize,
}

impl PerCoordinator {
    pub const MAX_PLAN_REWRITE_ATTEMPTS: usize = 2;

    pub fn new(reflection_default_max_rounds: usize) -> Self {
        Self {
            reflection: WorkflowReflectionController::new(reflection_default_max_rounds),
            require_plan_in_final_content: false,
            plan_rewrite_attempts: 0,
        }
    }

    /// 是否包含可解析的 `agent_reply_plan` v1 JSON（见 `plan_artifact`）。
    pub fn content_has_plan(content: &str) -> bool {
        plan_artifact::content_has_valid_agent_reply_plan_v1(content)
    }

    /// 在已将 assistant 消息推入 `messages` 之后调用，根据是否需要「规划」段落决定下一步。
    pub fn after_final_assistant(&mut self, msg: &Message) -> AfterFinalAssistant {
        if !self.require_plan_in_final_content {
            return AfterFinalAssistant::StopTurn;
        }
        let content = msg.content.as_deref().unwrap_or("");
        if Self::content_has_plan(content) {
            return AfterFinalAssistant::StopTurn;
        }
        if self.plan_rewrite_attempts >= Self::MAX_PLAN_REWRITE_ATTEMPTS {
            return AfterFinalAssistant::StopTurn;
        }
        self.plan_rewrite_attempts += 1;
        AfterFinalAssistant::RequestPlanRewrite(Message {
            role: "user".to_string(),
            content: Some(PLAN_REWRITE_USER_TEXT.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        })
    }

    /// 对一次 `workflow_execute` 的 arguments 做反思决策、补丁与「要求最终带规划」标记更新。
    pub fn prepare_workflow_execute(&mut self, args_json: &str) -> PreparedWorkflowExecute {
        let decision = self.reflection.decide(args_json);
        let reflection_inject = decision.inject_instruction.clone();
        if let Some(v) = reflection_inject.as_ref()
            && v.get("instruction_type")
                .and_then(|x| x.as_str())
                == Some("workflow_reflection_plan_next")
            {
                self.require_plan_in_final_content = true;
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
        messages: &mut Vec<Message>,
        tool_call_id: String,
        result: String,
        reflection_inject: Option<Value>,
    ) {
        messages.push(Message {
            role: "tool".to_string(),
            content: Some(result),
            tool_calls: None,
            name: None,
            tool_call_id: Some(tool_call_id),
        });
        if let Some(instruction) = reflection_inject {
            let instruction_str = serde_json::to_string(&instruction).unwrap_or_default();
            messages.push(Message {
                role: "user".to_string(),
                content: Some(instruction_str),
                tool_calls: None,
                name: None,
                tool_call_id: None,
            });
        }
    }
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

    #[test]
    fn final_assistant_rewrites_then_stops() {
        let mut c = PerCoordinator::new(5);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some("no plan here".to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn final_assistant_stops_when_plan_present() {
        let mut c = PerCoordinator::new(5);
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
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(c.after_final_assistant(&ok), AfterFinalAssistant::StopTurn));
    }

    #[test]
    fn prepare_workflow_first_round_injects_plan_next() {
        let mut c = PerCoordinator::new(5);
        let args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let prep = c.prepare_workflow_execute(args);
        assert!(prep.execute);
        assert!(prep.skipped_result.is_empty());
        let ty = prep
            .reflection_inject
            .as_ref()
            .and_then(|v| v.get("instruction_type"))
            .and_then(|x| x.as_str());
        assert_eq!(ty, Some("workflow_reflection_plan_next"));
    }
}
