//! `delegate_task` 的 agent 层调度（#839 P1）。
//!
//! 与 `workflow_tool_dispatch` 同款：本模块**不持有 LLM 凭据**，只消费调用方注入的
//! `SubAgentDelegator` 能力对象（构造见 [`crate::agent::delegation`]）。
//!
//! 参数面刻意极小：模型只能给 `prompt`。深度、预算、工具集、回传上限全部由服务端按
//! `cfg.subagent` 决定——**模型无法通过这些参数放大自己的额度**。

use crate::cm_tools::delegation_host::{
    SubAgentDelegator, SubTaskLimits, SubTaskReport, SubTaskRequest, SubTaskStatus,
};
use crate::cm_tools::tools::DelegateTaskArgs;

/// 把 `SubTaskReport` 渲染成工具正文。
///
/// 正文要能让模型直接据以决策：状态、用量、摘要、关键路径、以及是否被截断。
fn render_report(r: &SubTaskReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "子任务状态：{}（LLM 调用 {} 次，用时 {} 秒）\n",
        r.status.as_str(),
        r.stats.llm_calls,
        r.stats.elapsed_secs
    ));
    if let Some(code) = r.error_code.as_deref() {
        out.push_str(&format!("错误码：{code}\n"));
    }
    out.push('\n');
    out.push_str(&r.summary);
    out.push('\n');
    if !r.key_paths.is_empty() {
        out.push_str("\n关键路径：\n");
        for p in &r.key_paths {
            out.push_str(&format!("- {p}\n"));
        }
    }
    if r.truncated {
        out.push_str("\n（回传已截断，子任务产出可能不完整）\n");
    }
    out
}

/// 构造子任务请求。
fn build_request(prompt: String, limits: SubTaskLimits, max_report_chars: usize) -> SubTaskRequest {
    SubTaskRequest {
        prompt,
        // 模型不能指定 depth：首版不支持嵌套（`max_depth = 1`），父回合直接派即 0 层。
        depth: 0,
        // `None` = 由 delegator 按只读性派生全集（配置无法放大）。
        allowed_tools: None,
        limits,
        max_report_chars,
    }
}

/// 执行一次 `delegate_task` 调用。
///
/// 返回 `(工具正文, 结构化 payload)`；payload 供 SSE `structured_preview` 与金样使用。
/// 失败分两类：**装配前的拒绝**（未启用 / 参数非法）返回错误正文；**子任务失败**则
/// 仍是成功的工具调用——report 里已带 `status`，父回合据它决策（设计 §7「掐断不传染」）。
pub async fn dispatch_delegate_task_tool(
    delegator: Option<&dyn SubAgentDelegator>,
    limits: SubTaskLimits,
    max_report_chars: usize,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let Some(deg) = delegator else {
        return (
            "错误：delegate_task 未启用（`subagent_enabled = false`）或本调用路径未接线。"
                .to_string(),
            None,
        );
    };
    let parsed: DelegateTaskArgs = match serde_json::from_str(args.trim()) {
        Ok(a) => a,
        Err(e) => return (format!("错误：delegate_task 参数无效：{e}"), None),
    };
    let req = build_request(parsed.prompt, limits, max_report_chars);
    match deg.run(req).await {
        Ok(report) => {
            let body = render_report(&report);
            let payload = serde_json::to_value(&report).ok();
            // 子任务未拿到产出时给出明确的失败前缀，避免正文被启发式判成成功。
            if report.status == SubTaskStatus::Ok {
                (body, payload)
            } else {
                (format!("错误：子任务未完成\n{body}"), payload)
            }
        }
        Err(e) => (format!("错误：委托失败：{}", e.message), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_tools::delegation_host::SubTaskStats;

    fn limits() -> SubTaskLimits {
        SubTaskLimits {
            max_llm_calls: 8,
            max_tool_calls: u32::MAX,
            max_wall_secs: 300,
        }
    }

    /// 未注入 delegator（未启用 / 未接线）→ 明确错误，不静默失败。
    #[tokio::test]
    async fn missing_delegator_reports_clear_error() {
        let (body, payload) = dispatch_delegate_task_tool(
            None,
            limits(),
            4000,
            r#"{"prompt":"看看 src"}"#,
        )
        .await;
        assert!(body.starts_with("错误："), "{body}");
        assert!(body.contains("subagent_enabled"));
        assert!(payload.is_none());
    }

    /// 参数非法 → 在装配前拒掉，不白烧一次子回合。
    #[tokio::test]
    async fn invalid_args_rejected_before_run() {
        struct NeverRun;
        #[async_trait::async_trait]
        impl SubAgentDelegator for NeverRun {
            async fn run(
                &self,
                _r: SubTaskRequest,
            ) -> Result<SubTaskReport, crate::cm_tools::tool_result::ToolError> {
                panic!("参数非法时不应启动子回合");
            }
        }
        let (body, _) = dispatch_delegate_task_tool(
            Some(&NeverRun),
            limits(),
            4000,
            r#"{"prompt":123}"#,
        )
        .await;
        assert!(body.contains("参数无效"), "{body}");
    }

    /// 报告渲染：状态 / 用量 / 摘要 / 截断提示都要出现。
    #[test]
    fn report_render_includes_key_fields() {
        let r = SubTaskReport::bounded(
            SubTaskStatus::BudgetExhausted,
            "读到三个目录",
            vec!["src/a.rs".to_string()],
            SubTaskStats {
                llm_calls: 2,
                tool_calls: 0,
                elapsed_secs: 7,
                estimated_tokens: 100,
            },
            4000,
        )
        .with_error_code("subtask_budget_exhausted");
        let body = render_report(&r);
        assert!(body.contains("budget_exhausted"));
        assert!(body.contains("LLM 调用 2 次"));
        assert!(body.contains("读到三个目录"));
        assert!(body.contains("src/a.rs"));
        assert!(body.contains("subtask_budget_exhausted"));
    }

    /// `max_tool_calls` 传 `u32::MAX` 而非伪造数字（该维度当前施加不了）。
    #[test]
    fn build_request_marks_tool_cap_as_unbounded() {
        let req = build_request("x".to_string(), limits(), 4000);
        assert_eq!(req.limits.max_tool_calls, u32::MAX);
        assert_eq!(req.depth, 0, "模型不能抬高 depth");
        assert!(req.allowed_tools.is_none(), "工具集由 delegator 派生");
    }
}
