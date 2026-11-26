//! 完成判定第 ② 层的 **LLM 实现**（#1387 S0.5）。
//!
//! 契约在 [`crate::cm_agent::completion_judge`]；本模块只负责「把结构化证据渲染成短 prompt、
//! 调一次无工具侧向请求、把回复解析回 `CompletionVerdict`」。
//!
//! 写法照 `crate::agent::per_plan_semantic_check` 的既有范式：
//!
//! - **JSON-only**：system 明令只输出一个 JSON 对象，不带围栏与前后缀；
//! - **短 prompt**：任务与证据各自截断，`max_tokens` 钳制在 `32..=1024`；
//! - **无工具**：`no_tools_chat_request`，不让判定器自己去调工具；
//! - **不 fail-open 在本层**：失败一律返回 `Err`，由调用方维持第 ① 层结论
//!   （`per_plan_semantic_check` 是内部 fail-open，那是它的语义；这里分开更好测）。
//!
//! **锚点校验不在本层**：统一由 [`crate::cm_agent::completion_judge::judge_and_validate`] 施加，
//! 这样任何实现（含 mock）都不会绕过「`satisfied` 必须引用硬证据」。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

use crate::agent::turn_budget::TurnBudgetCounter;
use crate::cm_agent::completion_judge::{
    CompletionJudge, CompletionJudgeInput, CompletionStatus, CompletionVerdict, JudgedEvidence,
};
use crate::cm_tool_result::{ToolError, ToolFailureCategory};
use crate::cm_types::Message;
use crate::config::AgentConfig;
use crate::llm::{
    ChatCompletionsBackend, CompleteChatRetryingParams, LlmRetryingTransportOpts,
    complete_chat_retrying, no_tools_chat_request,
};

/// 判定器的 system 提示。
///
/// 关键措辞：**明确告知判定器看不到执行者的自我陈述**（输入里本来也没有），
/// 并要求 `evidence_refs` 只能引用给定 id——这两句共同压制「顺着自述点头」的偏差。
const JUDGE_SYSTEM: &str = "你是 CrabMate 的**任务完成度审计员**。\
你只能依据给定的「用户任务」与「工具执行证据」判断任务是否已经完成；\
你看不到执行过程的自然语言总结，也不得假设未列出的证据存在。\n\
必须**只输出一个 JSON 对象**（不要 Markdown 代码围栏、不要前后缀说明）。字段要求：\n\
- {\"status\":\"satisfied\",\"evidence_refs\":[\"ev_1\"]} —— 证据足以认定任务目标已达成；\
evidence_refs **必须非空**，且至少引用一条 kind 为 verify 或 write 的成功证据\n\
- {\"status\":\"partial\",\"evidence_refs\":[],\"missing\":[\"还差什么\"]} —— 部分达成\n\
- {\"status\":\"unsatisfied\"} —— 证据不足以认定达成\n\
evidence_refs 只能引用给定证据的 id，**不得编造**；missing 为字符串数组，每项不超过 60 字。";

/// 任务与证据的渲染上限（字符）。
const MAX_TASK_CHARS: usize = 2000;
const MAX_EVIDENCE_ITEMS: usize = 40;
const MAX_DETAIL_CHARS: usize = 200;

/// 侧向调用的入参（避免单函数参数过多）。
pub(crate) struct LlmJudgeCtx<'a> {
    pub llm_backend: &'a (dyn ChatCompletionsBackend + Send + Sync),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub cancel: Option<&'a AtomicBool>,
    pub turn_budget: Option<&'a Arc<TurnBudgetCounter>>,
    /// 覆盖模型；`None` = 走配置默认（**不**强制与执行模型分离，见设计 §10）。
    pub model_override: Option<String>,
    pub max_tokens: u32,
}

/// LLM 实现的完成判定器。
pub(crate) struct LlmCompletionJudge<'a> {
    ctx: LlmJudgeCtx<'a>,
}

impl<'a> LlmCompletionJudge<'a> {
    pub(crate) fn new(ctx: LlmJudgeCtx<'a>) -> Self {
        Self { ctx }
    }

    /// 按配置构造判定器；`completion_judge_enabled = false` 时返回 `None`（**默认开**，#1415），
    /// 调用方据此**完全不产生额外 LLM 调用**。
    ///
    /// 这是三个 `completion_judge_*` 配置键的消费者；接线点只需调本函数。
    pub(crate) fn from_config(
        cfg: &'a Arc<AgentConfig>,
        llm_backend: &'a (dyn ChatCompletionsBackend + Send + Sync),
        client: &'a reqwest::Client,
        api_key: &'a str,
        cancel: Option<&'a AtomicBool>,
        turn_budget: Option<&'a Arc<TurnBudgetCounter>>,
    ) -> Option<Self> {
        if !cfg.per_plan_policy.completion_judge_enabled {
            return None;
        }
        Some(Self::new(LlmJudgeCtx {
            llm_backend,
            client,
            api_key,
            cfg,
            cancel,
            turn_budget,
            model_override: cfg.per_plan_policy.completion_judge_model.clone(),
            max_tokens: cfg.per_plan_policy.completion_judge_max_tokens,
        }))
    }
}

/// 按 Unicode scalar 截断。
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 || s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// 把一条证据渲染成一行（单行，避免把 prompt 撑成多段）。
fn render_evidence(e: &JudgedEvidence) -> String {
    let exit = e
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "[{}] kind={} strength={} tool={} ok={} exit_code={} | {}",
        e.id,
        e.kind,
        e.strength,
        e.tool,
        e.ok,
        exit,
        truncate_chars(&e.detail, MAX_DETAIL_CHARS)
    )
}

/// 组装 user 正文。
fn render_user_body(input: &CompletionJudgeInput) -> String {
    let mut out = String::new();
    out.push_str("### 用户任务\n");
    out.push_str(&truncate_chars(input.task.trim(), MAX_TASK_CHARS));
    out.push_str("\n\n### 工具执行证据\n");
    if input.evidence.is_empty() {
        out.push_str("（无证据）\n");
        return out;
    }
    for e in input.evidence.iter().take(MAX_EVIDENCE_ITEMS) {
        out.push_str(&render_evidence(e));
        out.push('\n');
    }
    if input.evidence.len() > MAX_EVIDENCE_ITEMS {
        out.push_str(&format!(
            "…（另有 {} 条证据未列出）\n",
            input.evidence.len() - MAX_EVIDENCE_ITEMS
        ));
    }
    out
}

fn parse_str_array(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 解析判定器回复。`status` 无法识别即视为失败（由调用方 fail-open）。
fn parse_verdict(text: &str) -> Option<CompletionVerdict> {
    let trimmed = text.trim();
    // 容错：模型偶尔仍包一层围栏。
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim_end_matches("```")
        .trim();
    let v = if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        v
    } else {
        // 从首个 '{' 到末个 '}' 再试一次。
        let start = body.find('{')?;
        let end = body.rfind('}')?;
        serde_json::from_str::<serde_json::Value>(&body[start..=end]).ok()?
    };
    let status = CompletionStatus::parse(v.get("status")?.as_str()?)?;
    let evidence_refs = parse_str_array(v.get("evidence_refs"));
    let missing = parse_str_array(v.get("missing"));
    Some(CompletionVerdict::new(status, evidence_refs, missing))
}

#[async_trait]
impl CompletionJudge for LlmCompletionJudge<'_> {
    async fn judge(&self, input: CompletionJudgeInput) -> Result<CompletionVerdict, ToolError> {
        if let Some(cancel) = self.ctx.cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ToolError {
                category: ToolFailureCategory::Internal,
                code: "completion_judge_cancelled".to_string(),
                message: "完成判定被取消".to_string(),
                retryable: false,
                parsed: crate::cm_tool_result::ParsedToolOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: None,
                },
            });
        }
        let messages = vec![
            Message::system_only(JUDGE_SYSTEM),
            Message::user_only(render_user_body(&input)),
        ];
        let llm_cfg = crate::cm_types::llm_config::LlmConfig {
            llm: self.ctx.cfg.llm.clone(),
            sampling: self.ctx.cfg.llm_sampling.clone(),
            vendor_flags: self.ctx.cfg.llm_vendor_flags.clone(),
            http_retry: self.ctx.cfg.llm_http_retry.clone(),
        };
        let model_override = self.ctx.model_override.as_deref();
        let mut req = no_tools_chat_request(
            &llm_cfg,
            &messages,
            None,
            model_override,
            crate::types::LlmSeedOverride::FromConfig,
        );
        req.max_tokens = self.ctx.max_tokens.clamp(32, 1024);

        let cc = CompleteChatRetryingParams::new(
            self.ctx.llm_backend,
            self.ctx.client,
            self.ctx.api_key,
            self.ctx.cfg,
            LlmRetryingTransportOpts {
                out: None,
                no_stream: true,
                cancel: self.ctx.cancel,
            },
            None,
            model_override,
        )
        .with_turn_budget(self.ctx.turn_budget);

        let (reply, _finish) = complete_chat_retrying(&cc, &req)
            .await
            .map_err(|e| ToolError {
                category: ToolFailureCategory::External,
                code: "completion_judge_llm_failed".to_string(),
                message: format!("完成判定 LLM 调用失败：{e}"),
                retryable: true,
                parsed: crate::cm_tool_result::ParsedToolOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: None,
                },
            })?;
        let text = crate::types::message_content_as_str(&reply.content)
            .unwrap_or("")
            .trim()
            .to_string();
        parse_verdict(&text).ok_or_else(|| ToolError {
            category: ToolFailureCategory::Internal,
            code: "completion_judge_bad_reply".to_string(),
            message: format!(
                "完成判定回复无法解析为约定 JSON：{}",
                crate::redact::preview_chars(&text, 120)
            ),
            retryable: false,
            parsed: crate::cm_tool_result::ParsedToolOutput {
                ok: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error_code: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use crate::cm_llm::StreamChatParams;
    use crate::cm_types::ChatRequest;

    /// 测试用 stub 后端：固定返回一次响应（或固定失败）。
    ///
    /// 经 `complete_chat_retrying` 间接调用，所以**不需要**自己构造
    /// `StreamChatParams`（那要 25+ 方法的 `StreamChatHost`）。
    struct StubBackend(Result<(Message, String), String>);

    #[async_trait]
    impl ChatCompletionsBackend for StubBackend {
        async fn stream_chat(
            &self,
            _params: &StreamChatParams<'_>,
            _req: &mut ChatRequest,
        ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
            match &self.0 {
                Ok((m, f)) => Ok((m.clone(), f.clone())),
                Err(e) => Err(e.clone().into()),
            }
        }
    }

    fn judge_input() -> CompletionJudgeInput {
        CompletionJudgeInput {
            task: "跑通全部测试".to_string(),
            evidence: vec![JudgedEvidence {
                id: "ev_1".to_string(),
                kind: "verify".to_string(),
                strength: "strong".to_string(),
                tool: "cargo_test".to_string(),
                ok: true,
                exit_code: Some(0),
                detail: "test result: ok".to_string(),
            }],
        }
    }

    /// 跑一次真实 `judge()`（LLM 侧走 stub），失败时返回分类码。
    async fn run_judge(
        backend: &StubBackend,
        cancel: Option<&AtomicBool>,
    ) -> Result<CompletionVerdict, String> {
        let cfg = Arc::new(crate::cm_config::load_config(None).expect("嵌入默认配置应可加载"));
        let client = reqwest::Client::new();
        let judge = LlmCompletionJudge::new(LlmJudgeCtx {
            llm_backend: backend,
            client: &client,
            api_key: "test-key",
            cfg: &cfg,
            cancel,
            turn_budget: None,
            model_override: None,
            max_tokens: 512,
        });
        judge.judge(judge_input()).await.map_err(|e| e.code)
    }

    /// 正常路径：模型按约定 JSON 回答 → 解析成 `Satisfied`。
    #[tokio::test]
    async fn judge_returns_parsed_verdict_on_good_reply() {
        let backend = StubBackend(Ok((
            Message::assistant_only(r#"{"status":"satisfied","evidence_refs":["ev_1"]}"#),
            "stop".to_string(),
        )));
        let v = run_judge(&backend, None).await.expect("应成功");
        assert_eq!(v.status, CompletionStatus::Satisfied);
        assert_eq!(v.evidence_refs, vec!["ev_1".to_string()]);
    }

    /// 取消已置位 → 直接返回取消码，**不发起 LLM 调用**。
    ///
    /// backend 故意给「不该被用到」的失败响应：若代码真去调它，错误码会不同。
    #[tokio::test]
    async fn judge_short_circuits_when_cancelled() {
        let cancel = AtomicBool::new(true);
        let backend = StubBackend(Err("不应被调用".to_string()));
        let code = run_judge(&backend, Some(&cancel))
            .await
            .expect_err("应失败");
        assert_eq!(code, "completion_judge_cancelled");
    }

    /// LLM 调用失败 → 返回可识别码（调用方据此 fail-open）。
    #[tokio::test]
    async fn judge_reports_llm_failure() {
        let backend = StubBackend(Err("boom".to_string()));
        let code = run_judge(&backend, None).await.expect_err("应失败");
        assert_eq!(code, "completion_judge_llm_failed");
    }

    /// 模型答非约定 JSON → 返回 bad_reply 码（调用方据此 fail-open，绝不早停）。
    #[tokio::test]
    async fn judge_reports_bad_reply() {
        let backend = StubBackend(Ok((
            Message::assistant_only("我觉得差不多完成了"),
            "stop".to_string(),
        )));
        let code = run_judge(&backend, None).await.expect_err("应失败");
        assert_eq!(code, "completion_judge_bad_reply");
    }

    fn ev(id: &str, kind: &str, ok: bool) -> JudgedEvidence {
        JudgedEvidence {
            id: id.to_string(),
            kind: kind.to_string(),
            strength: "strong".to_string(),
            tool: "cargo_test".to_string(),
            ok,
            exit_code: Some(0),
            detail: "test result: ok. 100 passed".to_string(),
        }
    }

    #[test]
    fn parses_minimal_satisfied() {
        let v = parse_verdict(r#"{"status":"satisfied","evidence_refs":["ev_1"]}"#).expect("parse");
        assert_eq!(v.status, CompletionStatus::Satisfied);
        assert_eq!(v.evidence_refs, vec!["ev_1".to_string()]);
        assert!(v.missing.is_empty());
    }

    #[test]
    fn parses_partial_with_missing() {
        let v = parse_verdict(
            r#"{"status":"partial","evidence_refs":[],"missing":["未验证边界","  补测试  "]}"#,
        )
        .expect("parse");
        assert_eq!(v.status, CompletionStatus::Partial);
        assert_eq!(
            v.missing,
            vec!["未验证边界".to_string(), "补测试".to_string()]
        );
    }

    #[test]
    fn tolerates_markdown_fence() {
        let v = parse_verdict("```json\n{\"status\":\"unsatisfied\"}\n```").expect("parse");
        assert_eq!(v.status, CompletionStatus::Unsatisfied);
    }

    #[test]
    fn tolerates_leading_prose() {
        let v = parse_verdict(
            "判定如下：{\"status\":\"satisfied\",\"evidence_refs\":[\"ev_2\"]} 以上。",
        )
        .expect("parse");
        assert_eq!(v.status, CompletionStatus::Satisfied);
    }

    #[test]
    fn unknown_status_is_none() {
        // 未知 status → None，调用方据此 fail-open（维持第 ① 层结论）。
        assert!(parse_verdict(r#"{"status":"done","evidence_refs":["ev_1"]}"#).is_none());
    }

    #[test]
    fn unparsable_is_none() {
        assert!(parse_verdict("我觉得完成了").is_none());
    }

    #[test]
    fn render_includes_ids_and_marks_overflow() {
        let mut evidence: Vec<JudgedEvidence> = (0..MAX_EVIDENCE_ITEMS)
            .map(|i| ev(&format!("ev_{i}"), "verify", true))
            .collect();
        evidence.push(ev("ev_extra", "write", true));
        let body = render_user_body(&CompletionJudgeInput {
            task: "跑通测试".to_string(),
            evidence,
        });
        assert!(body.contains("[ev_0]"));
        assert!(body.contains("另有 1 条证据未列出"));
        // 溢出项本身不应被渲染
        assert!(!body.contains("[ev_extra]"));
    }

    #[test]
    fn render_marks_empty_evidence() {
        let body = render_user_body(&CompletionJudgeInput {
            task: "分析项目".to_string(),
            evidence: Vec::new(),
        });
        assert!(body.contains("（无证据）"));
    }

    #[test]
    fn render_truncates_long_task_and_detail() {
        let body = render_user_body(&CompletionJudgeInput {
            task: "任".repeat(5000),
            evidence: vec![JudgedEvidence {
                detail: "细".repeat(1000),
                ..ev("ev_1", "verify", true)
            }],
        });
        assert!(body.contains('…'));
        assert!(body.chars().count() < 3000);
    }

    #[test]
    fn long_missing_is_bounded_by_verdict_ctor() {
        // 渲染层不裁剪 missing，交由 CompletionVerdict::new 统一施加上限。
        let raw = format!(
            r#"{{"status":"partial","missing":["{}"]}}"#,
            "字".repeat(300)
        );
        let v = parse_verdict(&raw).expect("parse");
        assert_eq!(
            v.missing[0].chars().count(),
            CompletionVerdict::MAX_MISSING_CHARS + 1
        );
    }
}
