use std::sync::atomic::Ordering;

use crate::cm_sse_protocol::StreamEndReason;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::per_turn_flight::PerTurnFlight;
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
// 2) 本回合已发过时不应重复补发（fallback_skips_when_flag_already_set）
// 3) 同一回合二次触发时保持幂等（fallback_is_idempotent_for_same_turn）
#[tokio::test]
async fn fallback_emits_final_response_when_missing() {
    let flight = PerTurnFlight::default();
    let job_id = 42_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    assert!(emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await);

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
async fn fallback_skips_when_flag_already_set() {
    let flight = PerTurnFlight::default();
    // 模拟「本回合已有别的生产者发过终答时间线」：幂等闸已置位。
    flight.final_response_emitted.store(true, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    assert!(
        !emit_missing_final_response_fallback_if_needed(&flight, &tx, 43, &messages).await,
        "已发过终答时间线时兜底应返回 false"
    );

    let no_frame = timeout(Duration::from_millis(120), rx.recv()).await;
    assert!(no_frame.is_err(), "已有 final_response 时不应再补发");
}

#[tokio::test]
async fn fallback_is_idempotent_for_same_turn() {
    let flight = PerTurnFlight::default();
    let job_id = 44_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![Message::assistant_only("最终总结内容")];

    assert!(emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await);
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

    // 第 5 帧是 text_message_end：一次兜底固定发 5 帧（reasoning_end / text_start /
    // final_response / answer_phase / text_end），读空它才能断言「之后没有新帧」。
    let _text_end = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv text_end")
        .expect("text_end payload");

    // `final_response_emitted` 已由首次调用占位：二次调用应直接返回 false，不再写任何帧。
    let emitted_again =
        emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await;
    assert!(
        !emitted_again,
        "二次 fallback 应返回 false（本回合幂等闸已置位）"
    );
    let no_frame = timeout(Duration::from_millis(120), rx.recv()).await;
    assert!(no_frame.is_err(), "二次 fallback 不应再写出任何帧");
}

#[tokio::test]
async fn fallback_skips_when_turn_has_no_new_assistant() {
    let flight = PerTurnFlight::default();
    let job_id = 45_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let messages = vec![
        Message::system_only("sys"),
        Message::user_only("上一轮提问"),
        Message::assistant_only("上一轮回答"),
        Message::user_only("本轮提问"),
    ];

    assert!(!emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await);

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
    let flight = PerTurnFlight::default();
    let job_id = 46_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);

    let mut reasoning_only = Message::assistant_only("");
    reasoning_only.content = None;
    reasoning_only.reasoning_content = Some("推理：先读取目标文件再作答。".to_string());
    let messages = vec![Message::user_only("帮我看看"), reasoning_only];

    assert!(
        emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await,
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
    let flight = PerTurnFlight::default();
    let job_id = 47_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);

    let long_reasoning = "思".repeat(2500);
    let mut reasoning_only = Message::assistant_only("");
    reasoning_only.content = None;
    reasoning_only.reasoning_content = Some(long_reasoning.clone());
    let messages = vec![Message::user_only("帮我看看"), reasoning_only];

    assert!(emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await);

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

// 层 4b（#1414）：末条助手无正文、也无可回填思维链时，用**最后一次工具结果摘要**兜底终答。
// 场景即早停门的终答纠偏预算耗尽后仍以 tool-only 收尾，必须让消费者拿到「跑了什么、结果如何」。
// 摘要须单行化并截断，避免把整段工具输出搬进终答。
#[tokio::test]
async fn fallback_uses_last_tool_result_when_no_body_or_reasoning() {
    let flight = PerTurnFlight::default();
    let job_id = 48_u64;
    let (tx, mut rx) = mpsc::channel::<String>(8);

    let mut tool_only = Message::assistant_only("");
    tool_only.content = None;
    tool_only.tool_calls = Some(vec![ToolCall {
        id: "call_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: "cargo_test".to_string(),
            arguments: "{}".to_string(),
        },
    }]);
    let mut long_output = String::from("退出码：0\n标准输出：\ntest result: ok. 12 passed; 0 failed\n");
    long_output.push_str(&"日志".repeat(1200));
    let messages = vec![
        Message::user_only("跑通全部测试，然后回报结果"),
        tool_only,
        Message {
            role: "tool".to_string(),
            content: Some(long_output.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("cargo_test".to_string()),
            tool_call_id: Some("call_1".to_string()),
        },
    ];

    assert!(emit_missing_final_response_fallback_if_needed(&flight, &tx, job_id, &messages).await);

    let _reasoning_end = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv reasoning_end")
        .expect("reasoning_end payload");
    let _text_start = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv text_start")
        .expect("text_start payload");
    let frame = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("recv final_response frame")
        .expect("final_response payload");

    assert!(
        frame.contains("（本轮模型未输出终答，以下为最后一次工具执行的结果摘要）"),
        "工具摘要兜底须带来源前缀: {frame}"
    );
    assert!(
        frame.contains("cargo_test"),
        "工具摘要兜底须标明工具名: {frame}"
    );
    assert!(
        frame.contains("退出码：0 标准输出： test result"),
        "多行工具输出应压成单行: {frame}"
    );
    assert!(
        !frame.contains(&"日志".repeat(1000)),
        "超长工具输出不得整段搬进终答: {frame}"
    );
    assert!(frame.contains('…'), "截断后应追加省略号: {frame}");
}

// 层 2 收尾终态判定：`completed_turn_end_reason` 的单一事实来源行为固化。
#[test]
fn turn_end_reason_fallback_wins_over_visible_output() {
    let messages = vec![
        Message::user_only("问"),
        Message::assistant_only("有正文"),
    ];
    assert_eq!(
        completed_turn_end_reason(true, false, &messages),
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
        completed_turn_end_reason(false, false, &messages),
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
        completed_turn_end_reason(false, false, &messages),
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
        completed_turn_end_reason(false, false, &messages),
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
        completed_turn_end_reason(false, false, &messages),
        StreamEndReason::NoOutput,
        "无任何输出时应返回 NoOutput"
    );
}

// 层 1 预期固化：截断优先于「有可见输出」与「兜底」，终态须为 Truncated。
#[test]
fn turn_end_reason_truncated_wins_over_fallback_and_visible_output() {
    let messages = vec![
        Message::user_only("问"),
        Message::assistant_only("被截断的半句正文"),
    ];
    assert_eq!(
        completed_turn_end_reason(true, true, &messages),
        StreamEndReason::Truncated,
        "截断标志应优先于兜底与可见输出，返回 Truncated"
    );
    assert_eq!(
        completed_turn_end_reason(false, true, &messages),
        StreamEndReason::Truncated,
        "无兜底但被截断时也应返回 Truncated"
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

/// #1380：per-flight 观测镜像改由所有权决定在册与否。
///
/// 回归点：`stream_job_setup` 曾把 RAII guard 声明在装配函数内，drop 得比回合早，
/// 于是流式回合期间 `GET /status` 的 `per_active_jobs` 恒为空。改成 `Weak` 后
/// 「还有强引用」与「在册」是同一件事，无法再被作用域悄悄改变。
#[tokio::test]
async fn per_flight_listed_while_owner_alive() {
    let q = ChatJobQueue::new(1, 1);
    let flight = std::sync::Arc::new(PerTurnFlight::default());
    q.attach_per_flight(7, &flight);

    let listed = q.active_per_jobs();
    assert_eq!(listed.len(), 1, "driver 持有 flight 时应可见");
    assert_eq!(listed[0].job_id, 7);

    // 模拟回合收尾：最后一个强引用消失。
    drop(flight);
    assert!(q.active_per_jobs().is_empty(), "强引用消失后应自动摘除");
}

/// #1380：`active_per_jobs` 的惰性清扫要顺手摘掉失效键，不能随进程寿命累积。
#[tokio::test]
async fn stale_per_flight_keys_are_swept_on_read() {
    let q = ChatJobQueue::new(1, 1);
    let first = std::sync::Arc::new(PerTurnFlight::default());
    q.attach_per_flight(1, &first);
    drop(first);

    // 第二个回合登记时，1 号的 flight 已无人持有 —— 读取只应看到 2 号。
    let second = std::sync::Arc::new(PerTurnFlight::default());
    q.attach_per_flight(2, &second);

    let listed = q.active_per_jobs();
    assert_eq!(listed.len(), 1, "旧键应在读取时被清扫");
    assert_eq!(listed[0].job_id, 2);
}
