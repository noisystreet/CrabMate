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
/// 本请求显式申请的等级（设计 §6.1 的 `requested` 档）。
///
/// 与 [`resolve_job_permission_persisted`] 分开而不是先合并成一个值：`requested` **优先于**
/// `persisted` 的选档由 `resolve_permission_level_for_turn` 的 `requested.or(persisted)` 做，
/// 两档各自保留时 `ResolvedPermission.requested` 才有真实值可回显——合并后它会恒为 `None`，
/// 于是「我明明切了 L4」这类排查依据就丢了来源。
pub(super) fn resolve_job_permission_requested(
    requested: Option<&str>,
) -> Option<crate::cm_types::PermissionLevel> {
    parse_job_permission_level(requested, "请求体")
}

/// 会话持久化的等级**意图**（设计 §6.1 的 `persisted` 档）；`None` = 该会话从未指定。
pub(super) fn resolve_job_permission_persisted(
    persisted_active: Option<&str>,
) -> Option<crate::cm_types::PermissionLevel> {
    parse_job_permission_level(persisted_active, "会话持久值")
}

/// 两档共用的解析：空白与坏串都按「未指定」处理并留日志。
///
/// `POST /chat` 已在解析请求体时返回 400，这里只防御历史库里的脏数据与内部入口——
/// 一行坏串不该让整个回合 500，回落配置默认（与 `session_mode` 解析失败回落 `act` 同取向）。
fn parse_job_permission_level(
    raw: Option<&str>,
    source: &str,
) -> Option<crate::cm_types::PermissionLevel> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty())?;
    match crate::cm_types::parse_optional_permission_level(Some(raw)) {
        Ok(level) => level,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "permission_level（{source}）解析失败，按未指定处理：{e}"
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
    use super::{resolve_job_permission_persisted, resolve_job_permission_requested};
    use crate::cm_types::PermissionLevel;

    #[test]
    fn requested_and_persisted_stay_separate_stages() {
        // 两档各归各：选档（`requested.or(persisted)`）留给
        // `resolve_permission_level_for_turn`。合并成单值会让
        // `ResolvedPermission.requested` 恒为 `None`，排查时看不出等级来自
        // 「本次请求」还是「会话遗留」。
        assert_eq!(
            resolve_job_permission_requested(Some("workspace")),
            Some(PermissionLevel::Workspace)
        );
        assert_eq!(
            resolve_job_permission_persisted(Some("chat")),
            Some(PermissionLevel::Chat)
        );
    }

    #[test]
    fn blank_and_absent_mean_unset() {
        assert_eq!(resolve_job_permission_requested(None), None);
        assert_eq!(resolve_job_permission_requested(Some("  ")), None);
        assert_eq!(resolve_job_permission_persisted(None), None);
        assert_eq!(resolve_job_permission_persisted(Some("")), None);
    }

    #[test]
    fn invalid_string_falls_back_to_unset() {
        // 坏串回落「未指定」，不 panic、不默认成某个具体档（`POST /chat` 已前置 400）。
        assert_eq!(resolve_job_permission_requested(Some("root")), None);
        assert_eq!(resolve_job_permission_persisted(Some("root")), None);
    }
}
