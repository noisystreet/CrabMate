use crabmate_sse_protocol::StreamEndReason;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::sse::SseStreamHub;
use crate::types::Message;

use super::ChatJobQueue;
use super::stream_finish::{
    emit_missing_final_response_fallback_if_needed, emit_stream_ended_once,
    sse_payload_has_final_response_timeline,
};

#[tokio::test]
async fn queue_accepts_config_bounds() {
    let q = ChatJobQueue::new(2, 4);
    assert_eq!(q.max_concurrent(), 2);
    assert_eq!(q.max_pending(), 4);
}

// `final_response` 兜底回归覆盖：
// 1) 缺失时应补发（fallback_emits_final_response_when_missing）
// 2) 已存在且稍后可见时不应误补（fallback_skips_when_final_response_arrives_with_small_delay）
// 3) 同一 job 二次触发时保持幂等（fallback_is_idempotent_for_same_job）
#[tokio::test]
async fn fallback_emits_final_response_when_missing() {
    let hub = SseStreamHub::new();
    let job_id = 42_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await;

    // 跳过 reasoning_end / text_start，读取 timeline + answer_phase
    let _reasoning_end = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv reasoning_end")
        .expect("reasoning_end payload");
    let _text_start = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv text_start")
        .expect("text_start payload");
    let first = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv final_response frame")
        .expect("final_response payload");
    assert!(sse_payload_has_final_response_timeline(&first));

    let second = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv answer_phase frame")
        .expect("answer_phase payload");
    assert!(
        second.contains("\"customType\":\"assistant_answer_phase\""),
        "second frame should be answer_phase CUSTOM: {second}"
    );
}

#[tokio::test]
async fn fallback_skips_when_final_response_arrives_with_small_delay() {
    let hub = SseStreamHub::new();
    let job_id = 43_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    let delayed_payload = crate::sse::encode_message(crate::sse::SsePayload::TimelineLog {
        log: crate::sse::protocol::TimelineLogBody {
            kind: "final_response".to_string(),
            title: "已存在总结".to_string(),
            detail: None,
        },
    });
    let hub_for_task = hub.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = hub_for_task.publish(job_id, delayed_payload);
    });

    emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await;

    let no_frame = timeout(Duration::from_millis(120), rx.recv()).await;
    assert!(no_frame.is_err(), "已有 final_response 时不应再补发");
}

#[tokio::test]
async fn fallback_is_idempotent_for_same_job() {
    let hub = SseStreamHub::new();
    let job_id = 44_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await;
    // 跳过 reasoning_end / text_start
    let _r = rx.recv().await;
    let _t = rx.recv().await;
    let first = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv first frame (timeline)")
        .expect("first frame payload");
    let second = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv second frame (answer_phase)")
        .expect("second frame payload");
    assert!(sse_payload_has_final_response_timeline(&first));
    assert!(
        second.contains("\"customType\":\"assistant_answer_phase\""),
        "second frame should be answer_phase CUSTOM: {second}"
    );

    // 首次 fallback 通过桥接后，hub 已可见 final_response；二次调用应无输出。
    hub.publish(job_id, first);
    // eventual 内部有 5×20ms 轮询，等待足够久确保 publish 被读到
    tokio::time::sleep(Duration::from_millis(200)).await;
    // 二次调用应直接返回 false，不向 tx 写入任何数据
    let emitted_again =
        emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await;
    assert!(
        !emitted_again,
        "二次 fallback 应返回 false（hub 中已有 final_response）"
    );
}

#[tokio::test]
async fn fallback_skips_when_turn_has_no_new_assistant() {
    let hub = SseStreamHub::new();
    let job_id = 45_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![
        Message::system_only("sys"),
        Message::user_only("上一轮提问"),
        Message::assistant_only("上一轮回答"),
        Message::user_only("本轮提问"),
    ];

    emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await;

    let no_frame = timeout(Duration::from_millis(120), rx.recv()).await;
    assert!(
        no_frame.is_err(),
        "本轮无 assistant 输出时不应复用上一轮回答做 fallback"
    );
}

#[tokio::test]
async fn stream_ended_emits_before_followup_saved_event() {
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let mut sent = false;
    emit_stream_ended_once(
        &tx,
        99,
        StreamEndReason::Completed,
        &mut sent,
        "chat_job_queue::tests stream_ended_first",
        None,
    )
    .await;
    let saved = crate::sse::encode_message(crate::sse::SsePayload::ConversationSaved {
        saved: crate::sse::ConversationSavedBody {
            revision: 7,
            tiktoken_prompt_tokens: None,
        },
    });
    let _ = crate::sse::send_string_logged(
        &tx,
        saved,
        "chat_job_queue::tests conversation_saved_after_end",
    )
    .await;

    let first = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv first")
        .expect("first payload");
    let second = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv second")
        .expect("second payload");

    assert!(first.contains("\"type\":\"RUN_FINISHED\"") || first.contains("\"stream_ended\""));
    assert!(
        second.contains("\"customType\":\"conversation_saved\"")
            || second.contains("\"conversation_saved\"")
    );
}
