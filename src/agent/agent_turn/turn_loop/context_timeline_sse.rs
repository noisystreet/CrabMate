//! 上下文时间线 SSE（`timeline_log`）；不经过 `execute/tools`。

use crate::agent::agent_turn::host::turn_sink::TurnControlSink;
use crate::cm_agent::context_timeline::ContextTimelineEvent;
use crate::sse::{SsePayload, TimelineLogBody, send_sse_control_payload_optional};

pub(crate) async fn emit_context_timeline_sse(
    control: &TurnControlSink<'_>,
    events: &[ContextTimelineEvent],
) {
    for ev in events {
        append_context_timeline_replay_event(ev);
        let payload = SsePayload::TimelineLog {
            log: TimelineLogBody {
                kind: ev.kind.to_string(),
                title: ev.title.clone(),
                detail: Some(ev.detail.clone()),
            },
        };
        let _ = send_sse_control_payload_optional(
            control.out,
            control.sse_control_mirror.as_ref(),
            payload,
            "context_timeline",
            control.sse_encoder.as_ref(),
        )
        .await;
    }
}

/// 时间线旁注的 `detail` 本身就是 JSON 字符串：解析成容器就按结构化写入 replay，
/// 让 `turn-replay-events.jsonl` 可以直接 `jq '.detail.before_tokens'`。
///
/// 解析失败或结果不是容器（历史事件、纯文本 detail）时回退到 `{"text": …}` 包装，
/// 与其它 replay 事件的形状保持一致。
fn append_context_timeline_replay_event(ev: &ContextTimelineEvent) {
    // replay 未启用时 `append_*` 本就立刻返回，但那时 `detail` 已经被解析过一遍 —— 先判再解析。
    if !crate::turn_replay_dump::replay_dump_enabled() {
        return;
    }
    let detail = replay_detail_value(ev.detail.as_str());
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        ev.kind,
        ev.title.as_str(),
        Some(&detail),
    );
}

/// 见 [`append_context_timeline_replay_event`]。
fn replay_detail_value(detail: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(detail) {
        Ok(value) if value.is_object() || value.is_array() => value,
        _ => serde_json::json!({ "text": detail, "phase": "general" }),
    }
}

#[cfg(test)]
mod tests {
    use super::replay_detail_value;

    #[test]
    fn structured_detail_is_written_as_object() {
        let detail = r#"{"count_hit":true,"before_tokens":12000,"after_tokens":9000}"#;
        let value = replay_detail_value(detail);
        assert_eq!(value["before_tokens"], 12000);
        assert!(
            value.get("text").is_none(),
            "结构化 detail 不应再被包一层 text"
        );
    }

    #[test]
    fn plain_text_detail_falls_back_to_text_wrapper() {
        let value = replay_detail_value("已裁剪历史");
        assert_eq!(value["text"], "已裁剪历史");
        assert_eq!(value["phase"], "general");
    }

    #[test]
    fn json_scalar_detail_falls_back_to_text_wrapper() {
        // `"abc"` / `123` 是合法 JSON 但不是容器：保持 text 包装，避免下游按 key 取值踩空。
        let value = replay_detail_value("\"abc\"");
        assert_eq!(value["text"], "\"abc\"");
    }
}
