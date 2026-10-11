use crate::cm_sse_protocol::StreamEndReason;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::sse::SseStreamHub;
use crate::types::{FunctionCall, Message, ToolCall};

use super::ChatJobQueue;
use super::stream_finish::{
    completed_turn_end_reason, emit_missing_final_response_fallback_if_needed,
    emit_stream_draining, emit_stream_ended_once, sse_payload_has_final_response_timeline,
};

#[tokio::test]
async fn queue_accepts_config_bounds() {
    let q = ChatJobQueue::new(2, 4);
    assert_eq!(q.max_concurrent(), 2);
    assert_eq!(q.max_pending(), 4);
}

#[tokio::test]
async fn queue_stream_cancel_flag_roundtrip() {
    let q = ChatJobQueue::new(1, 1);
    let flag = q.register_stream_cancel(42);
    assert!(q.request_stream_cancel(42));
    assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    q.unregister_stream_cancel(42);
    assert!(!q.request_stream_cancel(42));
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

// 层 4 回归：末条助手仅有思维链（无正文）时，兜底须用思维链补发 `final_response` + `assistant_answer_phase`，
// 杜绝「`completed` + 无 `assistant_answer_phase` + 无 `final_response`」矛盾收尾。
#[tokio::test]
async fn fallback_uses_reasoning_when_last_assistant_has_no_content() {
    let hub = SseStreamHub::new();
    let job_id = 46_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);

    let mut reasoning_only = Message::assistant_only("");
    reasoning_only.content = None;
    reasoning_only.reasoning_content = Some("推理：先读取目标文件再作答。".to_string());
    let messages = vec![Message::user_only("帮我看看"), reasoning_only];

    assert!(
        emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await,
        "仅有思维链的终态应触发兜底并返回 true"
    );

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
    assert!(
        sse_payload_has_final_response_timeline(&first),
        "应以思维链补发 final_response 时间线: {first}"
    );
    assert!(
        first.contains("（本轮模型未输出正文，以下为从思考内容回填的兜底内容）"),
        "兜底终答应带来源前缀，避免与已流式下发的思维链混淆: {first}"
    );
    assert!(
        first.contains("推理：先读取目标文件再作答。"),
        "兜底终答应携带思维链文本: {first}"
    );

    let second = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv answer_phase frame")
        .expect("answer_phase payload");
    assert!(
        second.contains("\"customType\":\"assistant_answer_phase\""),
        "第二帧应为 answer_phase: {second}"
    );
}

// 层 4 兜底须截断超长思维链（上限 2000 字符 + 省略号），避免整段 CoT 外泄。
#[tokio::test]
async fn fallback_truncates_overlong_reasoning() {
    let hub = SseStreamHub::new();
    let job_id = 47_u64;
    hub.register_job(job_id);
    let (tx, mut rx) = mpsc::channel::<String>(8);

    let long_reasoning = "思".repeat(2500);
    let mut reasoning_only = Message::assistant_only("");
    reasoning_only.content = None;
    reasoning_only.reasoning_content = Some(long_reasoning.clone());
    let messages = vec![Message::user_only("帮我看看"), reasoning_only];

    assert!(emit_missing_final_response_fallback_if_needed(&hub, &tx, job_id, &messages).await);

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

    assert!(
        first.contains("（本轮模型未输出正文，以下为从思考内容回填的兜底内容）"),
        "截断后的兜底终答仍应带来源前缀: {first}"
    );
    assert!(
        !first.contains(&long_reasoning),
        "超长思维链不应被整段回填: {first}"
    );
    assert!(
        first.contains('…'),
        "超长思维链截断后应追加省略号: {first}"
    );
}

// 层 2 收尾终态判定：`completed_turn_end_reason` 的单一事实来源行为固化。
#[test]
fn turn_end_reason_fallback_wins_over_visible_output() {
    let messages = vec![
        Message::user_only("问"),
        Message::assistant_only("有正文"),
    ];
    assert_eq!(
        completed_turn_end_reason(true, &messages),
        StreamEndReason::Fallback,
        "层 4 兜底已补发终答时应返回 Fallback"
    );
}

#[test]
fn turn_end_reason_completed_for_assistant_answer() {
    let messages = vec![
        Message::user_only("问"),
        Message::assistant_only("这是正文回答。"),
    ];
    assert_eq!(
        completed_turn_end_reason(false, &messages),
        StreamEndReason::Completed,
        "本轮有正文终答应返回 Completed"
    );
}

// 发现 1 预期固化：仅工具调用（无正文）也算可见输出 → Completed。
#[test]
fn turn_end_reason_completed_for_tool_calls_only() {
    let mut tool_only = Message::assistant_only("");
    tool_only.content = None;
    tool_only.tool_calls = Some(vec![ToolCall {
        id: "call_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: "read_file".to_string(),
            arguments: "{}".to_string(),
        },
    }]);
    let messages = vec![Message::user_only("问"), tool_only];
    assert_eq!(
        completed_turn_end_reason(false, &messages),
        StreamEndReason::Completed,
        "仅工具调用也算可见输出，应返回 Completed"
    );
}

// 发现 2 收敛：仅思维链且兜底未发出时为 NoOutput（不再误判为 completed）。
#[test]
fn turn_end_reason_no_output_for_reasoning_only_without_fallback() {
    let mut reasoning_only = Message::assistant_only("");
    reasoning_only.content = None;
    reasoning_only.reasoning_content = Some("只思考未作答".to_string());
    let messages = vec![Message::user_only("问"), reasoning_only];
    assert_eq!(
        completed_turn_end_reason(false, &messages),
        StreamEndReason::NoOutput,
        "仅思维链且未兜底时不得判为 completed"
    );
}

#[test]
fn turn_end_reason_no_output_for_empty_assistant() {
    let mut empty = Message::assistant_only("");
    empty.content = None;
    let messages = vec![Message::user_only("问"), empty];
    assert_eq!(
        completed_turn_end_reason(false, &messages),
        StreamEndReason::NoOutput,
        "无任何输出时应返回 NoOutput"
    );
}

#[tokio::test]
async fn conversation_saved_emits_before_stream_ended() {
    let (tx, mut rx) = mpsc::channel::<String>(8);
    emit_stream_draining(&tx, 99).await;
    let saved = crate::sse::encode_message(crate::sse::SsePayload::ConversationSaved {
        saved: crate::sse::ConversationSavedBody {
            revision: 7,
            tiktoken_prompt_tokens: None,
        },
    });
    let _ = crate::sse::send_string_logged(
        &tx,
        saved,
        "chat_job_queue::tests conversation_saved_before_end",
    )
    .await;
    let mut sent = false;
    emit_stream_ended_once(
        &tx,
        99,
        StreamEndReason::Completed,
        &mut sent,
        "chat_job_queue::tests stream_ended_last",
        None,
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
    let third = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv third")
        .expect("third payload");

    assert!(
        first.contains("\"customType\":\"stream_draining\"")
            || first.contains("\"stream_draining\"")
    );
    assert!(
        second.contains("\"customType\":\"conversation_saved\"")
            || second.contains("\"conversation_saved\"")
    );
    assert!(third.contains("\"type\":\"RUN_FINISHED\"") || third.contains("\"stream_ended\""));
}
