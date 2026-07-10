//! API 与对话相关类型

use serde::Serialize;

/// 拼接在 `api_base` 后的 OpenAI 兼容 chat 路径（无前导斜杠）。
pub const OPENAI_CHAT_COMPLETIONS_REL_PATH: &str = "chat/completions";

/// 拼接在 `api_base` 后的 OpenAI 兼容模型列表路径（`GET`，无前导斜杠）；部分网关可能未实现。
pub const OPENAI_MODELS_REL_PATH: &str = "models";

/// 单次 `run_agent_turn` / HTTP 请求对 `chat/completions` 的 **`seed`** 覆盖（OpenAI 兼容字段；供应商不支持时通常会忽略）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub enum LlmSeedOverride {
    /// 使用 [`AgentConfig::llm_seed`]（未配置则请求体不带 `seed`）。
    #[default]
    FromConfig,
    /// 强制在请求 JSON 中写入该整数 `seed`。
    Fixed(i64),
    /// 本回合请求体**不**含 `seed`（即使配置里设置了默认 seed）。
    OmitFromRequest,
}

/// 合并配置中的默认 seed 与单次回合覆盖，得到写入 `ChatRequest.seed` 的值。
#[inline]
pub fn resolved_llm_seed(base: Option<i64>, override_: LlmSeedOverride) -> Option<i64> {
    match override_ {
        LlmSeedOverride::FromConfig => base,
        LlmSeedOverride::Fixed(n) => Some(n),
        LlmSeedOverride::OmitFromRequest => None,
    }
}

#[cfg(test)]
mod llm_seed_tests {
    use super::{LlmSeedOverride, resolved_llm_seed};

    #[test]
    fn resolved_seed_respects_override() {
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::FromConfig),
            Some(1)
        );
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::Fixed(42)),
            Some(42)
        );
        assert_eq!(
            resolved_llm_seed(Some(1), LlmSeedOverride::OmitFromRequest),
            None
        );
        assert_eq!(resolved_llm_seed(None, LlmSeedOverride::FromConfig), None);
    }
}

mod chat_api;
mod message;
mod message_lineage;
mod real_user_message;
pub mod server_injected_user;
mod staged_step_window;
mod tiktoken_snapshot;

pub use chat_api::*;
pub use message::*;
pub use real_user_message::{
    first_real_user_task_content, is_real_user_task_message, last_real_user_message_index,
    last_real_user_task_content, messages_slice_since_last_real_user,
};
pub use server_injected_user::{
    is_ephemeral_staged_coach_user_message, is_server_injected_user_message,
    strip_orchestration_injected_users_for_conversation_store,
};
pub use staged_step_window::{
    is_staged_step_injection_user_message, is_staged_step_window_boundary_user,
    last_staged_step_injection_index, staged_step_episode_end_exclusive,
    staged_step_window_end_exclusive, tool_messages_in_staged_step_episode,
    tool_messages_in_staged_step_window,
};
pub use tiktoken_snapshot::TiktokenPromptTokensSnapshot;
// 供宿主/调试引用 [`message_lineage`]；库内尚未全覆盖调用点，`cargo check` 下会呈现未使用。
#[allow(unused_imports)]
pub use message_lineage::{ContextInjectionKind, MessageLineage, message_lineage};
#[cfg(test)]
mod server_injected_user_store_tests {
    use super::*;

    #[test]
    fn strip_orchestration_injected_users_keeps_real_user_and_workspace_profile() {
        let mut v = vec![
            Message::user_only("真实用户"),
            Message::user_staged_step_injection("### 分步 1/1\n- id: a\n- 描述: b"),
            Message::user_plan_rewrite_injection("你的最终回答缺少**结构化规划**"),
            Message::user_first_turn_workspace_context("工作区画像"),
            Message::assistant_only("ok"),
        ];
        strip_orchestration_injected_users_for_conversation_store(&mut v);
        assert_eq!(v.len(), 3);
        assert!(v.iter().any(|m| {
            message_content_as_str(&m.content).is_some_and(|c| c.contains("真实用户"))
        }));
        assert!(
            v.iter()
                .any(crate::is_first_turn_workspace_context_injection)
        );
    }
}

#[cfg(test)]
mod api_messages_strip_tests {
    use super::*;

    #[test]
    fn filter_web_client_snapshot_drops_system_prompt_and_injections() {
        let inj_mem = Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text("mem".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_LONG_TERM_MEMORY_NAME.to_string()),
            tool_call_id: None,
        };
        let inj_cl = Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text("cl".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_WORKSPACE_CHANGELIST_NAME.to_string()),
            tool_call_id: None,
        };
        let inj_ctx = Message::user_first_turn_workspace_context("profile");
        let sys = Message::system_only("do not leak to web list");
        let plain = Message::user_only("hi");
        let reject = Message::user_planner_tool_call_reject_injection(format!(
            "{STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX}\n请重写"
        ));
        let v = vec![sys, inj_mem, inj_cl, inj_ctx, plain.clone(), reject];
        let out = filter_messages_for_web_client_snapshot(&v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], plain);
    }

    #[test]
    fn web_snapshot_hides_registered_server_injected_users() {
        let plain = Message::user_only("真实用户提问");
        let cases: Vec<(Message, bool)> = vec![
            (plain.clone(), true),
            (
                Message::user_staged_step_injection("### 分步 1/2\n- id: s1\n- 描述: 运行检查"),
                false,
            ),
            (
                Message::user_staged_orchestration_injection(format!(
                    "{}\n请优化",
                    crabmate_display_rules::STAGED_PLAN_OPTIMIZER_COACH_MARK
                )),
                false,
            ),
            (
                Message::user_plan_rewrite_injection(
                    "你的最终回答缺少**结构化规划**。请加入 agent_reply_plan JSON",
                ),
                false,
            ),
            (
                Message::user_server_injection(
                    CRABMATE_STAGED_PATCH_FEEDBACK_NAME,
                    "### 分阶段规划 · 步级反馈（plan_id=x）\n补丁",
                ),
                false,
            ),
        ];
        for (msg, expect_visible) in cases {
            let out = filter_messages_for_web_client_snapshot(std::slice::from_ref(&msg));
            assert_eq!(
                out.len() == 1,
                expect_visible,
                "visible={expect_visible} name={:?}",
                msg.name
            );
            if expect_visible {
                assert_eq!(out[0], msg);
            }
        }
    }

    #[test]
    fn filter_web_client_snapshot_keeps_timeline_system_markers() {
        let tl = Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(r#"{"kind":"x"}"#.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("crabmate_timeline".to_string()),
            tool_call_id: None,
        };
        let u = Message::user_only("hi");
        let v = vec![Message::system_only("sys"), tl.clone(), u.clone()];
        let out = filter_messages_for_web_client_snapshot(&v);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], tl);
        assert_eq!(out[1], u);
    }

    #[test]
    fn skip_ui_separator_and_strip_reasoning_one_pass() {
        let sep = Message::chat_ui_separator(true);
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("body".to_string())),
            reasoning_content: Some("chain".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::user_only("u"), sep, assistant];
        let out = messages_for_api_stripping_reasoning_skip_ui_separators(&v, false, false);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(message_content_as_str(&out[1].content), Some("body"));
        assert!(out[1].reasoning_content.is_none());
    }

    #[test]
    fn strip_reasoning_only_matches_composing_without_separators() {
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("x".to_string())),
            reasoning_content: Some("r".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::user_only("u"), assistant];
        let a = messages_stripping_reasoning_for_api_request(&v, false, false);
        let b = messages_for_api_stripping_reasoning_skip_ui_separators(&v, false, false);
        assert_eq!(a, b);
    }

    #[test]
    fn preserve_reasoning_for_assistant_tool_calls_when_requested() {
        let tc = ToolCall {
            id: "x".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "f".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let asst = Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: Some("think".to_string()),
            reasoning_details: None,
            tool_calls: Some(vec![tc.clone()]),
            name: None,
            tool_call_id: None,
        };
        let kept = message_clone_stripping_reasoning_for_api(&asst, true, false);
        assert_eq!(kept.reasoning_content.as_deref(), Some("think"));
        assert!(kept.reasoning_details.is_none());
        let gone = message_clone_stripping_reasoning_for_api(&asst, false, false);
        assert!(gone.reasoning_content.is_none());
    }

    #[test]
    fn preserve_inserts_empty_reasoning_when_tool_calls_but_missing() {
        let tc = ToolCall {
            id: "x".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "f".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let asst = Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![tc]),
            name: None,
            tool_call_id: None,
        };
        let out = message_clone_stripping_reasoning_for_api(&asst, true, false);
        assert_eq!(out.reasoning_content.as_deref(), Some(""));
    }

    #[test]
    fn deepseek_thinking_strips_reasoning_when_no_tool_calls_per_vendor_doc() {
        // https://api-docs.deepseek.com/zh-cn/guides/thinking_mode — 未工具调用时 reasoning 无需参与拼接
        let asst = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("hi".to_string())),
            reasoning_content: Some("think only".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let out = message_clone_stripping_reasoning_for_api(&asst, false, true);
        assert!(out.reasoning_content.is_none());
    }

    #[test]
    fn deepseek_thinking_keeps_reasoning_when_tool_calls_present() {
        let tc = ToolCall {
            id: "t".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "f".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let asst = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("call".to_string())),
            reasoning_content: Some("chain".to_string()),
            reasoning_details: None,
            tool_calls: Some(vec![tc]),
            name: None,
            tool_call_id: None,
        };
        let out = message_clone_stripping_reasoning_for_api(&asst, false, true);
        assert_eq!(out.reasoning_content.as_deref(), Some("chain"));
    }
}

#[cfg(test)]
mod normalize_messages_tests {
    use super::*;

    fn asst(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(content.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn asst_with_tc(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(content.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: "tc1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "noop".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn merges_adjacent_assistant_placeholder_after_prior_assistant() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("prior"),
            asst(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        assert_eq!(message_content_as_str(&n[2].content), Some("prior"));
    }

    #[test]
    fn drops_trailing_empty_assistant() {
        let v = vec![Message::system_only("s"), Message::user_only("u"), asst("")];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].role, "user");
    }

    #[test]
    fn merges_streaming_partial_then_full_assistant() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("hel"),
            asst("hello"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(message_content_as_str(&n[2].content), Some("hello"));
    }

    /// 裁剪掉 tool 后常见：带 tool_calls 的 assistant 紧挨下一条助手正文。
    #[test]
    fn strips_orphan_tool_calls_when_followed_by_assistant_reply() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc("calling tool"),
            asst("final answer"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        assert!(n[2].tool_calls.is_none());
        assert!(
            message_content_as_str(&n[2].content)
                .unwrap()
                .contains("calling tool")
        );
        assert!(
            message_content_as_str(&n[2].content)
                .unwrap()
                .contains("final answer")
        );
    }

    #[test]
    fn strips_tool_calls_when_followed_by_empty_assistant_only() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc("x"),
            asst(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(message_content_as_str(&n[2].content), Some("x"));
        assert!(n[2].tool_calls.is_none());
    }

    /// 正文助手 + 带 tool_calls 的助手后仍有 tool 消息：必须保留 tool_calls（不得被末尾孤儿清理误伤）。
    #[test]
    fn preserves_merged_tool_calls_when_tool_follows() {
        let tool = Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text(r#"{"ok":true}"#.to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("tc1".to_string()),
        };
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("reasoning"),
            asst_with_tc(""),
            tool,
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 4);
        assert_eq!(n[2].role, "assistant");
        assert!(n[2].tool_calls.as_ref().is_some_and(|c| !c.is_empty()));
        assert_eq!(n[3].role, "tool");
    }

    #[test]
    fn collapses_three_consecutive_assistants() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("a"),
            asst("b"),
            asst("c"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
        let c = message_content_as_str(&n[2].content).unwrap();
        assert!(c.contains('a') && c.contains('c'));
    }

    #[test]
    fn merges_when_assistant_role_has_whitespace() {
        let mut odd = asst("x");
        odd.role = " Assistant ".to_string();
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            odd,
            asst("y"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[2].role, "assistant");
    }

    /// 前一条仅正文、后一条带 tool_calls：合并为一条；末尾无 `tool` 时孤儿 `tool_calls` 再被清掉（否则仍非法）。
    #[test]
    fn merges_assistant_then_assistant_with_tool_calls() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst("partial"),
            asst_with_tc(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert!(n[2].tool_calls.is_none());
        assert!(
            message_content_as_str(&n[2].content)
                .unwrap()
                .contains("partial")
        );
    }

    /// 末尾仅 `tool_calls`、正文为空且无后续 `tool`：清空 `tool_calls` 后须整条删除，避免 API 400。
    #[test]
    fn drops_trailing_assistant_when_orphan_tool_calls_cleared_and_content_empty() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u"),
            asst_with_tc(""),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 2);
        assert_eq!(n[1].role, "user");
    }

    /// strip `reasoning_content` 后正文为空、也无 `tool_calls` 的助手若在**中间**，仍会导致 DeepSeek HTTP 400，须整表剔除。
    #[test]
    fn drops_infix_empty_assistant_without_tool_calls() {
        let v = vec![
            Message::system_only("s"),
            Message::user_only("u1"),
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message::user_only("u2"),
        ];
        let n = normalize_messages_for_openai_compatible_request(v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0].role, "system");
        assert_eq!(n[1].role, "user");
        assert_eq!(n[2].role, "user");
    }
}

#[cfg(test)]
mod fold_system_messages_tests {
    use super::*;

    #[test]
    fn merges_system_into_following_user() {
        let v = vec![Message::system_only("sys"), Message::user_only("hi")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].role, "user");
        assert_eq!(message_content_as_str(&o[0].content), Some("sys\n\nhi"));
    }

    #[test]
    fn joins_multiple_system_blocks() {
        let v = vec![
            Message::system_only("a"),
            Message::system_only("b"),
            Message::user_only("u"),
        ];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(message_content_as_str(&o[0].content), Some("a\n\nb\n\nu"));
    }

    #[test]
    fn system_before_assistant_inserts_user_carrier() {
        let a = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("reply".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let v = vec![Message::system_only("instr"), a];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].role, "user");
        assert_eq!(message_content_as_str(&o[0].content), Some("instr"));
        assert_eq!(o[1].role, "assistant");
    }

    #[test]
    fn trailing_system_only_becomes_user() {
        let v = vec![Message::system_only("orphan")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].role, "user");
        assert_eq!(message_content_as_str(&o[0].content), Some("orphan"));
    }

    #[test]
    fn trims_system_role_case_and_whitespace() {
        let mut s = Message::system_only("x");
        s.role = " SYSTEM ".to_string();
        let v = vec![s, Message::user_only("y")];
        let o = fold_system_messages_into_following_user(v);
        assert_eq!(o.len(), 1);
        assert!(
            message_content_as_str(&o[0].content)
                .unwrap()
                .starts_with("x")
        );
    }
}

#[cfg(test)]
mod sanitize_tool_call_arguments_tests {
    use super::sanitize_tool_call_arguments_for_openai_compat;

    #[test]
    fn empty_and_whitespace_become_empty_object() {
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat(""), "{}");
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat("   "), "{}");
    }

    #[test]
    fn valid_json_round_trips_compact() {
        assert_eq!(
            sanitize_tool_call_arguments_for_openai_compat(r#"{"path":"a"}"#),
            r#"{"path":"a"}"#
        );
    }

    #[test]
    fn invalid_json_becomes_empty_object() {
        assert_eq!(sanitize_tool_call_arguments_for_openai_compat("{"), "{}");
        assert_eq!(
            sanitize_tool_call_arguments_for_openai_compat("not json"),
            "{}"
        );
    }

    #[test]
    fn escapes_literal_newline_inside_json_string() {
        let raw = concat!("{\"code\": \"def f():", "\n", "    pass\"}");
        let out = sanitize_tool_call_arguments_for_openai_compat(raw);
        let v: serde_json::Value = serde_json::from_str(&out).expect("sanitized must parse");
        assert_eq!(v["code"], "def f():\n    pass");
    }

    #[test]
    fn repairs_truncated_string_and_object() {
        let raw = r#"{"code": "partial"#;
        let out = sanitize_tool_call_arguments_for_openai_compat(raw);
        let v: serde_json::Value = serde_json::from_str(&out).expect("sanitized must parse");
        assert_eq!(v["code"], "partial");
    }
}

#[cfg(test)]
mod message_lineage_pub_smoke {
    use crate::Message;
    use crate::message_lineage::{MessageLineage, message_lineage};

    #[test]
    fn pub_exports_resolve_for_embedding_hosts() {
        assert!(matches!(
            message_lineage(&Message::user_only("x")),
            MessageLineage::UserNatural
        ));
    }
}
