//! 队列 worker：在独立 task 中经 [`crate::TurnRunner`] 执行回合（流式与 JSON 模式）。

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

impl JobOutcome {
    pub(super) fn kind_label(&self) -> &'static str {
        match self {
            Self::Stream { .. } => "stream",
            Self::Json { .. } => "json",
        }
    }

    pub(super) fn fields(self) -> (bool, bool, Option<String>) {
        match self {
            Self::Stream { ok, cancelled, err } | Self::Json { ok, cancelled, err } => {
                (ok, cancelled, err)
            }
        }
    }
}

pub(super) async fn run_queued_job(job: QueuedChatJob) -> JobOutcome {
    match job {
        QueuedChatJob::Stream {
            envelope,
            stream_event_tx,
            web_approval_session,
            cancel,
        } => {
            let github_token = envelope.github_token.clone();
            crate::github_token::with_request_github_token(github_token, async move {
                stream_job::run_stream_queued_job(stream_job::StreamQueuedJobParams {
                    envelope,
                    stream_event_tx,
                    web_approval_session,
                    cancel,
                })
                .await
            })
            .await
        }
        QueuedChatJob::Json { envelope, reply_tx } => {
            let github_token = envelope.github_token.clone();
            crate::github_token::with_request_github_token(github_token, async move {
                json_job::run_json_queued_job(json_job::JsonQueuedJobParams { envelope, reply_tx })
                    .await
            })
            .await
        }
    }
}
