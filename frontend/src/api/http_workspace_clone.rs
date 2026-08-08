//! `POST /workspace/clone/stream`：消费 SSE 进度事件。

use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;

use super::browser::{api_url, auth_headers, window};

#[derive(Debug, Clone)]
pub struct WorkspaceCloneRequest {
    pub url: String,
    pub name: String,
    pub depth: Option<u32>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone)]
pub enum WorkspaceCloneSseEvent {
    Phase(String),
    Log(String),
    Progress {
        percent: u8,
        #[allow(dead_code)]
        label: String,
    },
    Done {
        name: String,
        path: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Deserialize)]
struct SseJson {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    line: Option<String>,
    #[serde(default)]
    percent: Option<u8>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn parse_sse_data_line(data: &str) -> Option<WorkspaceCloneSseEvent> {
    let v: SseJson = serde_json::from_str(data.trim()).ok()?;
    match v.kind.as_str() {
        "phase" => Some(WorkspaceCloneSseEvent::Phase(v.phase.unwrap_or_default())),
        "log" => Some(WorkspaceCloneSseEvent::Log(v.line.unwrap_or_default())),
        "progress" => Some(WorkspaceCloneSseEvent::Progress {
            percent: v.percent.unwrap_or(0),
            label: v.label.unwrap_or_default(),
        }),
        "done" => Some(WorkspaceCloneSseEvent::Done {
            name: v.name.unwrap_or_default(),
            path: v.path.unwrap_or_default(),
        }),
        "error" => Some(WorkspaceCloneSseEvent::Error {
            code: v.code.unwrap_or_else(|| "CLONE_ERROR".into()),
            message: v.message.unwrap_or_default(),
        }),
        _ => None,
    }
}

fn drain_sse_events(buf: &mut String) -> Vec<WorkspaceCloneSseEvent> {
    let mut out = Vec::new();
    while let Some(idx) = buf.find("\n\n") {
        let block = buf[..idx].to_string();
        buf.drain(..idx + 2);
        for line in block.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(data) = line.strip_prefix("data:") {
                if let Some(ev) = parse_sse_data_line(data) {
                    out.push(ev);
                }
            }
        }
    }
    out
}

fn http_error_from_json_body(status: u16, s: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        let code = v
            .get("code")
            .and_then(|x| x.as_str())
            .unwrap_or("CLONE_HTTP")
            .to_string();
        let message = v
            .get("error")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("message").and_then(|x| x.as_str()))
            .unwrap_or(s)
            .to_string();
        return format!("{code}: {message} (HTTP {status})");
    }
    format!("HTTP {status}: {s}")
}

fn append_utf8_chunk(raw: &mut Vec<u8>, text_buf: &mut String, chunk: &[u8]) {
    raw.extend_from_slice(chunk);
    if let Ok(s) = std::str::from_utf8(raw) {
        text_buf.push_str(s);
        raw.clear();
        return;
    }
    let Some(n) = std::str::from_utf8(raw)
        .err()
        .map(|e| e.valid_up_to())
        .filter(|n| *n > 0)
    else {
        return;
    };
    text_buf.push_str(std::str::from_utf8(&raw[..n]).unwrap_or(""));
    raw.drain(..n);
}

fn take_stream_chunk_bytes(chunk_val: &JsValue) -> (bool, Option<Vec<u8>>) {
    let obj = js_sys::Object::from(chunk_val.clone());
    let done = js_sys::Reflect::get(&obj, &JsValue::from_str("done"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let bytes = js_sys::Reflect::get(&obj, &JsValue::from_str("value"))
        .ok()
        .and_then(|value| {
            if value.is_undefined() || value.is_null() {
                return None;
            }
            value.dyn_into::<js_sys::Uint8Array>().ok().map(|arr| {
                let mut chunk = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut chunk);
                chunk
            })
        });
    (done, bytes)
}

fn apply_sse_event(
    ev: &WorkspaceCloneSseEvent,
    done_name: &mut Option<String>,
    done_path: &mut Option<String>,
    stream_err: &mut Option<String>,
) {
    match ev {
        WorkspaceCloneSseEvent::Done { name, path } => {
            *done_name = Some(name.clone());
            *done_path = Some(path.clone());
        }
        WorkspaceCloneSseEvent::Error { code, message } => {
            *stream_err = Some(format!("{code}: {message}"));
        }
        _ => {}
    }
}

async fn open_clone_sse_response(
    req: &WorkspaceCloneRequest,
    loc: Locale,
) -> Result<Response, String> {
    let body = json!({
        "url": req.url,
        "name": req.name,
        "depth": req.depth,
        "branch": req.branch,
    });
    let body_s = serde_json::to_string(&body).map_err(|e| e.to_string())?;

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    let _ = h.set("Accept", "text/event-stream");
    init.set_headers(&h);
    init.set_body(&JsValue::from_str(&body_s));

    let request = Request::new_with_str_and_init(&api_url("/workspace/clone/stream"), &init)
        .map_err(|e| format!("request: {:?}", e))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;

    let status = resp.status();
    if status != 200 {
        let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
            .await
            .map_err(|e| format!("read body: {:?}", e))?;
        let s = text.as_string().unwrap_or_default();
        return Err(http_error_from_json_body(status, &s));
    }
    Ok(resp)
}

/// 流式 clone；`on_event` 每条 SSE 业务事件回调一次。成功返回 `done` 的 path。
pub async fn post_workspace_clone_stream<F>(
    req: WorkspaceCloneRequest,
    loc: Locale,
    mut on_event: F,
) -> Result<(String, String), String>
where
    F: FnMut(WorkspaceCloneSseEvent),
{
    let resp = open_clone_sse_response(&req, loc).await?;
    let body = resp
        .body()
        .ok_or_else(|| "clone stream: empty body".to_string())?;
    let reader = body
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|_| "clone stream: reader".to_string())?;

    let mut text_buf = String::new();
    let mut raw: Vec<u8> = Vec::new();
    let mut done_name = None::<String>;
    let mut done_path = None::<String>;
    let mut stream_err = None::<String>;

    loop {
        let chunk_val = JsFuture::from(reader.read())
            .await
            .map_err(|e| format!("stream read: {:?}", e))?;
        let (done, bytes) = take_stream_chunk_bytes(&chunk_val);
        if let Some(chunk) = bytes {
            append_utf8_chunk(&mut raw, &mut text_buf, &chunk);
        }
        for ev in drain_sse_events(&mut text_buf) {
            apply_sse_event(&ev, &mut done_name, &mut done_path, &mut stream_err);
            on_event(ev);
        }
        if done {
            break;
        }
        if done_path.is_some() || stream_err.is_some() {
            reader.release_lock();
            break;
        }
    }

    if let Some(err) = stream_err {
        return Err(err);
    }
    match (done_name, done_path) {
        (Some(n), Some(p)) if !p.is_empty() => Ok((n, p)),
        _ => Err("clone stream ended without done".to_string()),
    }
}

/// 从仓库 URL 推断默认项目名（去 `.git`）。
pub fn infer_project_name_from_clone_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let last = u
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(".git");
    let mut out = String::new();
    for (i, c) in last.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            if i == 0 && !c.is_ascii_alphanumeric() {
                continue;
            }
            out.push(c);
        }
    }
    if out.is_empty() {
        "project".into()
    } else {
        out.chars().take(64).collect()
    }
}
