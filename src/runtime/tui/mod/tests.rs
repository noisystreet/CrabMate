use super::*;
use crate::types::Message;
use tokio::sync::mpsc;

fn assistant(content: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

#[test]
fn trailing_content_returns_last_assistant_plain_content() {
    let msgs = vec![Message::user_only("q"), assistant("partial answer")];
    let got = trailing_streaming_assistant_content(&msgs);
    assert_eq!(got, "partial answer");
}

#[test]
fn trailing_content_ignores_tool_or_non_assistant_tail() {
    let msgs = vec![
        Message::user_only("q"),
        assistant("answer"),
        Message {
            role: "tool".to_string(),
            content: Some("{}".to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: Some("c1".to_string()),
        },
    ];
    let got = trailing_streaming_assistant_content(&msgs);
    assert_eq!(got, "");
}

#[test]
fn trailing_content_ignores_assistant_with_tool_calls() {
    let msgs = vec![Message {
        role: "assistant".to_string(),
        content: Some("calling".to_string()),
        tool_calls: Some(vec![]),
        name: None,
        tool_call_id: None,
    }];
    let got = trailing_streaming_assistant_content(&msgs);
    assert_eq!(got, "");
}

#[tokio::test]
async fn event_forwarder_prefers_stream_when_both_pending() {
    let (stream_tx, stream_rx) = mpsc::channel::<String>(4);
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<Arc<[Message]>>(4);
    let (event_tx, mut event_rx) = mpsc::channel::<TuiAgentEvent>(8);

    stream_tx
        .send("delta-a".to_string())
        .await
        .expect("stream send should work");
    snapshot_tx
        .send(vec![Message::user_only("q")].into())
        .await
        .expect("snapshot send should work");
    drop(stream_tx);
    drop(snapshot_tx);

    let forwarder = spawn_tui_event_forwarder(stream_rx, snapshot_rx, event_tx);
    let first = event_rx.recv().await.expect("first event");
    let second = event_rx.recv().await.expect("second event");

    assert!(matches!(first, TuiAgentEvent::StreamLine(ref s) if s == "delta-a"));
    assert!(matches!(second, TuiAgentEvent::MessagesSnapshot(_)));

    forwarder.await.expect("forwarder join");
}

#[tokio::test]
async fn event_forwarder_continues_after_stream_channel_closed() {
    let (stream_tx, stream_rx) = mpsc::channel::<String>(4);
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<Arc<[Message]>>(4);
    let (event_tx, mut event_rx) = mpsc::channel::<TuiAgentEvent>(8);

    drop(stream_tx);
    snapshot_tx
        .send(vec![Message::user_only("still works")].into())
        .await
        .expect("snapshot send should work");
    drop(snapshot_tx);

    let forwarder = spawn_tui_event_forwarder(stream_rx, snapshot_rx, event_tx);
    let ev = event_rx.recv().await.expect("event should arrive");
    assert!(matches!(ev, TuiAgentEvent::MessagesSnapshot(_)));
    forwarder.await.expect("forwarder join");
}

#[tokio::test]
async fn event_forwarder_coalesces_snapshots_to_latest() {
    let (stream_tx, stream_rx) = mpsc::channel::<String>(1);
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<Arc<[Message]>>(8);
    let (event_tx, mut event_rx) = mpsc::channel::<TuiAgentEvent>(8);

    drop(stream_tx);
    snapshot_tx
        .send(vec![Message::user_only("old")].into())
        .await
        .expect("snapshot old send should work");
    snapshot_tx
        .send(vec![Message::user_only("new")].into())
        .await
        .expect("snapshot new send should work");
    drop(snapshot_tx);

    let forwarder = spawn_tui_event_forwarder(stream_rx, snapshot_rx, event_tx);
    let ev = event_rx.recv().await.expect("snapshot event should arrive");
    match ev {
        TuiAgentEvent::MessagesSnapshot(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].content.as_deref(), Some("new"));
        }
        TuiAgentEvent::StreamLine(_) => panic!("expected snapshot event"),
    }
    forwarder.await.expect("forwarder join");
}

#[tokio::test]
async fn event_forwarder_flushes_snapshot_after_all_queued_stream_lines() {
    let n_stream = 70usize;
    let (stream_tx, stream_rx) = mpsc::channel::<String>(n_stream + 8);
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<Arc<[Message]>>(8);
    let (event_tx, mut event_rx) = mpsc::channel::<TuiAgentEvent>(n_stream + 16);

    for i in 0..n_stream {
        stream_tx
            .send(format!("s-{i}"))
            .await
            .expect("stream send should work");
    }
    snapshot_tx
        .send(vec![Message::user_only("snap")].into())
        .await
        .expect("snapshot send should work");
    drop(stream_tx);
    drop(snapshot_tx);

    let forwarder = spawn_tui_event_forwarder(stream_rx, snapshot_rx, event_tx);

    // 快照排在所有流式行之后入队：应先收齐全部 StreamLine，再收到 MessagesSnapshot（分步工具结果同理）。
    for i in 0..n_stream {
        let ev = event_rx.recv().await.expect("stream event should arrive");
        assert!(matches!(ev, TuiAgentEvent::StreamLine(ref s) if s == &format!("s-{i}")));
    }
    let ev = event_rx
        .recv()
        .await
        .expect("snapshot after stream drain should arrive");
    assert!(matches!(ev, TuiAgentEvent::MessagesSnapshot(_)));
    forwarder.await.expect("forwarder join");
}
