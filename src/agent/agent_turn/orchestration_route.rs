//! 编排路由决议：tracing + SSE **`timeline_log`**（`kind=orchestration_route`）。

use crabmate_agent::agent_turn::{TurnRouteDecisionV1, log_turn_route_decision};

use crate::sse::{self, SsePayload};

use super::params::RunLoopParams;

pub(crate) async fn record_and_emit_turn_route_decision(
    p: &RunLoopParams<'_>,
    decision: &TurnRouteDecisionV1,
) {
    log_turn_route_decision(decision);
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_route_decision_version = decision.version,
        turn_route_top_level = decision.top_level.as_str(),
        turn_route_orchestration_mode = decision.orchestration_mode.as_str(),
        turn_route_turn_phase = decision.turn_phase.as_str(),
        turn_route_freeform_because = decision.freeform_because.as_deref().unwrap_or(""),
        turn_route_hierarchical_post_intent_route =
            decision.hierarchical_post_intent_route.as_deref().unwrap_or(""),
        "turn_route_decision"
    );
    let detail = decision.to_json().ok();
    let title = format!("编排路由：{}", decision.orchestration_mode);
    crate::turn_replay_dump::append_turn_replay_event_if_configured(
        "orchestration_route",
        title.as_str(),
        detail.as_deref(),
    );
    if let Some(out) = p.ctx.io.out {
        let payload = SsePayload::TimelineLog {
            log: sse::protocol::TimelineLogBody {
                kind: "orchestration_route".to_string(),
                title,
                detail,
            },
        };
        let _ =
            sse::send_string_logged(out, sse::encode_message(payload), "orchestration_route").await;
    }
}
