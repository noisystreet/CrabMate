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
/// 本回合的权限等级 `persisted` 档（设计 §6.1 的 min 链）：请求显式传的等级优先，否则用会话持久值。
///
/// 两者都是「意图」而非生效值——真正的钳制（上限 / 角色 / 会话模式）在
/// `cm_agent::permission::resolve_permission_level_for_turn` 里做，这里只负责选档。
///
/// 解析失败按 `None` 处理并留日志：历史库里的坏串不该让整个回合 500，
/// 回落配置默认（与 `session_mode` 解析失败回落 `act` 同一取向）。
pub(super) fn resolve_job_permission_persisted(
    requested: Option<&str>,
    persisted_active: Option<&str>,
) -> Option<crate::cm_types::PermissionLevel> {
    let raw = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| persisted_active.map(str::trim).filter(|s| !s.is_empty()))?;
    match crate::cm_types::parse_optional_permission_level(Some(raw)) {
        Ok(level) => level,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "permission_level 解析失败，回落配置默认：{e}"
            );
            None
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

#[cfg(test)]
mod tests {
    use super::resolve_job_permission_persisted;
    use crate::cm_types::PermissionLevel;

    #[test]
    fn request_level_wins_over_persisted() {
        assert_eq!(
            resolve_job_permission_persisted(Some("workspace"), Some("chat")),
            Some(PermissionLevel::Workspace)
        );
    }

    #[test]
    fn falls_back_to_persisted_when_request_absent() {
        assert_eq!(
            resolve_job_permission_persisted(None, Some("read")),
            Some(PermissionLevel::Read)
        );
        assert_eq!(
            resolve_job_permission_persisted(Some("  "), Some("read")),
            Some(PermissionLevel::Read)
        );
    }

    #[test]
    fn absent_and_invalid_both_mean_unset() {
        assert_eq!(resolve_job_permission_persisted(None, None), None);
        assert_eq!(resolve_job_permission_persisted(Some(""), Some("  ")), None);
        // 坏串回落「未指定」，不 panic、不默认成某个具体档。
        assert_eq!(
            resolve_job_permission_persisted(Some("root"), None),
            None,
            "非法等级应回落配置默认，而不是静默提权/降级"
        );
    }
}
