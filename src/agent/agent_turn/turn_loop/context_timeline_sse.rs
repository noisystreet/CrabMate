//! 上下文时间线 SSE（`timeline_log`）；不经过 `execute/tools`。

use crate::agent::agent_turn::host::turn_sink::TurnControlSink;
use crate::cm_agent::context_timeline::ContextTimelineEvent;
use crate::sse::{SsePayload, TimelineLogBody, send_sse_control_payload_optional};

pub(crate) async fn emit_context_timeline_sse(
    control: &TurnControlSink<'_>,
    events: &[ContextTimelineEvent],
) {
    for ev in events {
        crate::turn_replay_dump::append_turn_replay_event_if_configured(
            ev.kind,
            ev.title.as_str(),
            Some(ev.detail.as_str()),
        );
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
