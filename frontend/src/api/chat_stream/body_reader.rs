use futures_util::future::{Either, select};
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::i18n::Locale;

use super::ChatStreamCallbacks;
use super::sse_frame::{flush_sse_tail, process_sse_buffer};

/// 块边界可能截断 UTF-8：只把从开头起「完整码点」前缀解码进 `text`，余字节留在 `raw`。
fn append_chunk_to_text_buffer(raw: &mut Vec<u8>, chunk: &[u8], text: &mut String) {
    raw.extend_from_slice(chunk);
    loop {
        if raw.is_empty() {
            break;
        }
        match std::str::from_utf8(raw.as_slice()) {
            Ok(s) => {
                text.push_str(s);
                raw.clear();
                break;
            }
            Err(e) => {
                let n = e.valid_up_to();
                if n == 0 {
                    break;
                }
                text.push_str(std::str::from_utf8(&raw[..n]).expect("valid_up_to"));
                raw.drain(..n);
            }
        }
    }
}

/// 已收到 `stream_ended` 后，部分浏览器/代理可能长期不结束 body；超时则 `releaseLock` 结束挂起。
const POST_STREAM_ENDED_READ_TIMEOUT_MS: u32 = 25_000;

/// 尚未收到 `stream_ended` 时，单次 `read()` 若长期无字节（断流、掉帧、代理挂起），会永远阻塞；设上限以便回落 busy。
/// 长思考无 SSE 的网关较少见；若仍误判可调大或做配置。
const PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS: u32 = 300_000;

/// 两次「含 `data:` 的有效负载」之间的最大间隔（毫秒）。代理可能周期性下发不含 `data:` 的注释帧，
/// 使 `read()` 频繁返回，从而永远不触发 [`PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS`]；此上限仍可结束悬挂流。
/// 断线重连路径亦依赖此项（该路径不设单次 read 超时）。
const SSE_MEANINGFUL_PAYLOAD_IDLE_TIMEOUT_MS: f64 = 180_000.0;

/// 消费 `/chat/stream` 响应体：UTF-8 重组、SSE 分帧与尾部 flush（与断线重连时的读失败语义一致）。
pub(super) async fn consume_chat_stream_response_body(
    rb: web_sys::ReadableStream,
    signal: &web_sys::AbortSignal,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
    stream_resume_job_id: Option<u64>,
) -> Result<(bool, bool), String> {
    let reader: web_sys::ReadableStreamDefaultReader = rb
        .get_reader()
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_stream_reader(loc).to_string())?;

    let mut raw: Vec<u8> = Vec::new();
    let mut buffer = String::new();
    let mut stream_finished_normally = false;
    let mut saw_stream_ended = false;
    let mut last_meaningful_payload_ms = js_sys::Date::now();
    loop {
        if signal.aborted() {
            return Ok((true, saw_stream_ended));
        }
        if !saw_stream_ended {
            let now = js_sys::Date::now();
            if now - last_meaningful_payload_ms > SSE_MEANINGFUL_PAYLOAD_IDLE_TIMEOUT_MS {
                reader.release_lock();
                stream_finished_normally = true;
                break;
            }
        }
        let chunk: wasm_bindgen::JsValue = if saw_stream_ended {
            match select(
                JsFuture::from(reader.read()),
                TimeoutFuture::new(POST_STREAM_ENDED_READ_TIMEOUT_MS),
            )
            .await
            {
                Either::Left((Ok(c), _)) => c,
                Either::Left((Err(e), _)) => {
                    if stream_resume_job_id.is_none() {
                        return Err(crate::i18n::api_err_stream_read(&e));
                    }
                    break;
                }
                Either::Right(((), _pending_read)) => {
                    reader.release_lock();
                    stream_finished_normally = true;
                    break;
                }
            }
        } else if stream_resume_job_id.is_none() {
            match select(
                JsFuture::from(reader.read()),
                TimeoutFuture::new(PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS),
            )
            .await
            {
                Either::Left((Ok(c), _)) => c,
                Either::Left((Err(e), _)) => {
                    return Err(crate::i18n::api_err_stream_read(&e));
                }
                Either::Right(((), _)) => {
                    reader.release_lock();
                    stream_finished_normally = true;
                    break;
                }
            }
        } else {
            match JsFuture::from(reader.read()).await {
                Ok(c) => c,
                Err(e) => {
                    if stream_resume_job_id.is_none() {
                        return Err(crate::i18n::api_err_stream_read(&e));
                    }
                    break;
                }
            }
        };
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if done {
            stream_finished_normally = true;
            break;
        }
        let value =
            js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).unwrap_or(JsValue::NULL);
        if let Some(u8) = value.dyn_ref::<js_sys::Uint8Array>() {
            append_chunk_to_text_buffer(&mut raw, &u8.to_vec(), &mut buffer);
        }
        let meaningful =
            process_sse_buffer(&mut buffer, last_event_id, &mut saw_stream_ended, cbs, loc)?;
        if meaningful > 0 {
            last_meaningful_payload_ms = js_sys::Date::now();
        }
        // 不在此处因 `stream_ended` 提前 break：提前结束 ReadableStream 消费可能导致部分环境下
        // `fetch` 身未完成、外层 `send_chat_stream` 永久 await，状态栏卡「模型生成中」。
    }
    if !raw.is_empty() {
        buffer.push_str(&String::from_utf8_lossy(&raw));
        raw.clear();
    }
    let _tail_meaningful =
        flush_sse_tail(&mut buffer, last_event_id, &mut saw_stream_ended, cbs, loc)?;
    if saw_stream_ended {
        stream_finished_normally = true;
    }
    Ok((stream_finished_normally, saw_stream_ended))
}

#[cfg(test)]
mod tests {
    use super::append_chunk_to_text_buffer;

    /// 完整 ASCII 块：直接追加到 text，raw 清空。
    #[test]
    fn append_ascii_chunk() {
        let mut raw = Vec::new();
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, b"hello", &mut text);
        assert_eq!(text, "hello");
        assert!(raw.is_empty());
    }

    /// 多字节 UTF-8 完整块（中文）。
    #[test]
    fn append_utf8_chunk() {
        let mut raw = Vec::new();
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, "你好世界".as_bytes(), &mut text);
        assert_eq!(text, "你好世界");
        assert!(raw.is_empty());
    }

    /// 块边界截断 UTF-8：3 字节中文字符只给了前 2 字节，应留在 raw 中。
    #[test]
    fn append_truncated_utf8_keeps_in_raw() {
        let mut raw = Vec::new();
        let mut text = String::new();
        // "你" = [0xE4, 0xBD, 0xA0]，只给前 2 字节
        append_chunk_to_text_buffer(&mut raw, &[0xE4, 0xBD], &mut text);
        assert!(text.is_empty());
        assert_eq!(raw, vec![0xE4, 0xBD]);
    }

    /// 前一个块留下不完整字节，后一个块补全。
    #[test]
    fn append_completes_partial_from_previous_chunk() {
        let mut raw = vec![0xE4, 0xBD]; // "你" 的前 2 字节
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, &[0xA0], &mut text); // "你" 的最后一字节
        assert_eq!(text, "你");
        assert!(raw.is_empty());
    }

    /// 前一个块留下不完整字节，后一个块仍不完整：应继续留在 raw。
    #[test]
    fn append_partial_then_still_partial() {
        let mut raw = vec![0xE4]; // 某 3 字节字符的第 1 字节
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, &[0xBD], &mut text); // 第 2 字节，仍不完整
        assert!(text.is_empty());
        assert_eq!(raw, vec![0xE4, 0xBD]);
    }

    /// 空 chunk：不应改变任何状态。
    #[test]
    fn append_empty_chunk_noop() {
        let mut raw = Vec::new();
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, b"", &mut text);
        assert!(text.is_empty());
        assert!(raw.is_empty());
    }

    /// 混合：前一段完整 ASCII + 后一段不完整 UTF-8 前缀。
    #[test]
    fn append_ascii_then_truncated_utf8() {
        let mut raw = Vec::new();
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, b"abc", &mut text);
        assert_eq!(text, "abc");
        assert!(raw.is_empty());

        append_chunk_to_text_buffer(&mut raw, &[0xE4, 0xBD], &mut text);
        assert_eq!(text, "abc");
        assert_eq!(raw, vec![0xE4, 0xBD]);
    }

    /// raw 中已有不完整字节，追加新 chunk 后仍不完整。
    #[test]
    fn append_to_non_empty_raw() {
        let mut raw = vec![0xE4, 0xBD]; // 不完整
        let mut text = String::new();
        // 追加完整 ASCII
        append_chunk_to_text_buffer(&mut raw, b"!", &mut text);
        // raw 仍不完整（0xE4 0xBD 0x21 不是合法 UTF-8 前缀序列）
        assert!(text.is_empty());
        assert_eq!(raw, vec![0xE4, 0xBD, 0x21]);
    }

    /// 连续追加多个完整块。
    #[test]
    fn append_multiple_full_chunks() {
        let mut raw = Vec::new();
        let mut text = String::new();
        append_chunk_to_text_buffer(&mut raw, b"ab", &mut text);
        append_chunk_to_text_buffer(&mut raw, b"cd", &mut text);
        assert_eq!(text, "abcd");
        assert!(raw.is_empty());
    }
}
