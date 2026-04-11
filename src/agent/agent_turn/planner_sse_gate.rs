//! Web 分阶段「无工具规划轮」SSE 门控：与 `AGENT_WEB_RAW_ASSISTANT_OUTPUT` 对齐。
//!
//! - 规划 JSON（合并 `reasoning_content` + 正文）解析为 `no_task: true` 时：不向浏览器下发规划轮正文（SSE 缓冲清空）。
//! - 否则：丢弃 CrabMate 信封前出现的纯文本增量（`reasoning_*`），保留信封与 `assistant_answer_phase` 之后的正文增量。

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

const CHANNEL_CAP: usize = 512;

struct GateState {
    seen_answer_phase: bool,
    post_phase: Vec<String>,
}

pub(crate) struct PlannerSseGate {
    pub(crate) inner_tx: mpsc::Sender<String>,
    state: Arc<Mutex<GateState>>,
    join: tokio::task::JoinHandle<()>,
    real_out: mpsc::Sender<String>,
}

impl PlannerSseGate {
    pub(crate) fn spawn(real_out: mpsc::Sender<String>) -> Self {
        let (inner_tx, mut inner_rx) = mpsc::channel::<String>(CHANNEL_CAP);
        let state = Arc::new(Mutex::new(GateState {
            seen_answer_phase: false,
            post_phase: Vec::new(),
        }));
        let st = Arc::clone(&state);
        let ro = real_out.clone();
        let join = tokio::spawn(async move {
            while let Some(line) = inner_rx.recv().await {
                let trimmed = line.trim_start();
                let json_val: Option<Value> = if trimmed.starts_with('{') {
                    serde_json::from_str(trimmed).ok()
                } else {
                    None
                };
                let is_envelope = json_val
                    .as_ref()
                    .and_then(|v| v.get("v"))
                    .map(|x| x.is_number())
                    .unwrap_or(false);
                if is_envelope {
                    let phase = json_val
                        .as_ref()
                        .and_then(|v| v.get("assistant_answer_phase"))
                        .and_then(|x| x.as_bool())
                        == Some(true);
                    let _ = crate::sse::send_string_logged(&ro, line, "planner_sse_gate:envelope")
                        .await;
                    if phase {
                        let mut g = st.lock().await;
                        g.seen_answer_phase = true;
                    }
                    continue;
                }
                let mut g = st.lock().await;
                if !g.seen_answer_phase {
                    continue;
                }
                g.post_phase.push(line);
            }
        });
        Self {
            inner_tx,
            state,
            join,
            real_out,
        }
    }

    /// `inner_tx` 须在调用前随 `CompleteChatRetryingParams` 一并释放（drop）。
    pub(crate) async fn finish(self, assistant_msg: &crate::types::Message) {
        let Self {
            inner_tx,
            state,
            join,
            real_out,
        } = self;
        drop(inner_tx);
        let _ = join.await;
        let no_task =
            crate::agent::plan_artifact::parse_agent_reply_plan_v1_from_assistant_message(
                assistant_msg,
            )
            .ok()
            .is_some_and(|p| p.no_task);
        let mut g = state.lock().await;
        if no_task {
            g.post_phase.clear();
            return;
        }
        for line in g.post_phase.drain(..) {
            let _ =
                crate::sse::send_string_logged(&real_out, line, "planner_sse_gate:flush_content")
                    .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sse_envelope_with_v() {
        let s = crate::sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        let v: Value = serde_json::from_str(s.trim()).unwrap();
        assert!(v.get("v").map(|x| x.is_number()).unwrap_or(false));
        assert_eq!(
            v.get("assistant_answer_phase").and_then(|x| x.as_bool()),
            Some(true)
        );
    }
}
