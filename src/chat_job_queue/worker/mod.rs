//! 队列 worker：在独立 task 中执行 `run_agent_turn`（流式与 JSON 模式）。

mod json_job;
mod stream_job;
mod stream_job_setup;

use super::QueuedChatJob;

pub(super) enum JobOutcome {
    Stream {
        ok: bool,
        cancelled: bool,
        err: Option<String>,
    },
    Json {
        ok: bool,
        cancelled: bool,
        err: Option<String>,
    },
}

pub(super) async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            envelope,
            stream_event_tx,
            web_approval_session,
        } => {
            stream_job::run_stream_queued_job(stream_job::StreamQueuedJobParams {
                envelope,
                stream_event_tx,
                web_approval_session,
            })
            .await
        }
        QueuedChatJob::Json { envelope, reply_tx } => {
            json_job::run_json_queued_job(json_job::JsonQueuedJobParams { envelope, reply_tx })
                .await
        }
    }
}
