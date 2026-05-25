//! [`super::send_chat_stream`] 用的抽取逻辑（降低 CCN）。

use web_sys::Response;

use crabmate_sse_protocol::StreamEndReason;

use crate::i18n::Locale;

use super::{ChatStreamCallbacks, body_reader, http_request};

pub(super) fn chat_stream_fetch_retry_exhausted(
    stream_resume_job_id: Option<u64>,
    attempt: u32,
) -> bool {
    stream_resume_job_id.is_none() || attempt >= 6
}

pub(super) async fn chat_stream_http_error_message(
    resp: &Response,
    loc: Locale,
) -> Result<String, String> {
    let msg = http_request::chat_stream_read_error_body(resp, loc).await?;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg)
        && let Some(m) = v.get("message").and_then(|x| x.as_str())
        && !m.trim().is_empty()
    {
        return Ok(m.to_string());
    }
    Ok(msg)
}

pub(super) enum ChatStreamConsumeOutcome {
    Finished { saw_stream_ended: bool },
    ResumeReconnect,
}

pub(super) async fn consume_chat_stream_body_phase(
    resp: Response,
    signal: &web_sys::AbortSignal,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
    stream_resume_job_id: Option<u64>,
) -> Result<ChatStreamConsumeOutcome, String> {
    let Some(rb) = resp.body() else {
        return Err(crate::i18n::api_err_no_response_body(loc).to_string());
    };
    let (stream_finished_normally, saw_stream_ended) =
        body_reader::consume_chat_stream_response_body(
            rb,
            signal,
            last_event_id,
            cbs,
            loc,
            stream_resume_job_id,
        )
        .await?;
    Ok(if stream_finished_normally {
        ChatStreamConsumeOutcome::Finished { saw_stream_ended }
    } else {
        ChatStreamConsumeOutcome::ResumeReconnect
    })
}

pub(super) enum ChatStreamRoundOutcome {
    Completed,
    ResumeReconnect,
}

/// 单轮 HTTP 响应：`410` / 错误体 / SSE 体消费与正常收尾回调。
pub(super) async fn run_chat_stream_http_round(
    resp: Response,
    cbs: &ChatStreamCallbacks,
    stream_resume_job_id: &mut Option<u64>,
    signal: &web_sys::AbortSignal,
    last_event_id: &mut u64,
    loc: Locale,
) -> Result<ChatStreamRoundOutcome, String> {
    http_request::apply_chat_stream_response_headers(&resp, cbs, stream_resume_job_id);
    if resp.status() == 410 {
        return Err(crate::i18n::api_err_stream_gone(loc).to_string());
    }
    if !resp.ok() {
        return Err(chat_stream_http_error_message(&resp, loc).await?);
    }
    match consume_chat_stream_body_phase(
        resp,
        signal,
        last_event_id,
        cbs,
        loc,
        *stream_resume_job_id,
    )
    .await?
    {
        ChatStreamConsumeOutcome::Finished { saw_stream_ended } => {
            if !saw_stream_ended {
                // 某些后端/网络尾部场景可能未显式下发 `stream_ended`，前端按正常完结补齐。
                (cbs.on_stream_ended)(StreamEndReason::Completed.to_string(), None);
            }
            (cbs.on_done)();
            Ok(ChatStreamRoundOutcome::Completed)
        }
        ChatStreamConsumeOutcome::ResumeReconnect => Ok(ChatStreamRoundOutcome::ResumeReconnect),
    }
}
