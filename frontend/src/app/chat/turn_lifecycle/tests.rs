use super::reducer::{
    StreamSubPhase, TurnLifecycleEvent, TurnLifecycleState, TurnPhase, apply_turn_lifecycle,
    turn_lifecycle_coarse_busy, turn_lifecycle_ui_inflight,
};
use crate::app::chat::composer_stream::StreamControlEvent;

fn apply_seq(s: &mut TurnLifecycleState, evs: &[TurnLifecycleEvent]) {
    for e in evs {
        apply_turn_lifecycle(s, *e);
    }
}

#[test]
fn attach_open_delta_drains_and_shell_release_idle() {
    let mut s = TurnLifecycleState::default();
    apply_seq(
        &mut s,
        &[
            TurnLifecycleEvent::AttachPrepared {
                attach_generation: 1,
            },
            TurnLifecycleEvent::HttpStreamOpened {
                attach_generation: 1,
            },
            TurnLifecycleEvent::SseControl(StreamControlEvent::ModelTextDelta),
            TurnLifecycleEvent::SseControl(StreamControlEvent::StreamEnded),
            TurnLifecycleEvent::SseControl(StreamControlEvent::StreamDone),
            TurnLifecycleEvent::ShellReleased {
                attach_generation: 1,
            },
        ],
    );
    assert_eq!(s.phase, TurnPhase::Idle);
    assert!(!turn_lifecycle_coarse_busy(s));
}

#[test]
fn tool_call_enters_tool_subphase() {
    let mut s = TurnLifecycleState::default();
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::AttachPrepared {
            attach_generation: 2,
        },
    );
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::SseControl(StreamControlEvent::ToolCallDeclared),
    );
    assert!(matches!(
        s.phase,
        TurnPhase::Streaming {
            attach_generation: 2,
            sub: StreamSubPhase::ToolUiBusy,
        }
    ));
}

#[test]
fn stale_generation_http_open_is_noop() {
    let mut s = TurnLifecycleState::default();
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::AttachPrepared {
            attach_generation: 3,
        },
    );
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::AttachPrepared {
            attach_generation: 4,
        },
    );
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::HttpStreamOpened {
            attach_generation: 3,
        },
    );
    assert!(matches!(
        s.phase,
        TurnPhase::Attaching {
            attach_generation: 4
        }
    ));
}

#[test]
fn shell_release_only_when_generation_matches() {
    let mut s = TurnLifecycleState::default();
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::AttachPrepared {
            attach_generation: 5,
        },
    );
    apply_turn_lifecycle(
        &mut s,
        TurnLifecycleEvent::ShellReleased {
            attach_generation: 99,
        },
    );
    assert!(matches!(
        s.phase,
        TurnPhase::Attaching {
            attach_generation: 5
        }
    ));
    assert!(turn_lifecycle_ui_inflight(s));
}
