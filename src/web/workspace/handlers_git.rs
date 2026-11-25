//! `GET /workspace/git`：只读 git 状态投影的 HTTP 薄壳。
//!
//! 解析、探测与状态映射都在 [`super::git_projection`]；这里只负责工作区根解析、
//! 缓存读写（TTL + 最小 single-flight）与响应组装。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};

use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::workspace::{
    WorkspaceGitFileList, WorkspaceGitQuery, WorkspaceGitSource, WorkspaceGitState,
    WorkspaceGitStatusResponse, WorkspaceGitUntrackedMode,
};
use crate::web::app_state::{AppStateHttpCore, CachedWorkspaceGitStatus, WorkspaceGitCache};
use crate::web::workspace::git_projection::{
    self, CACHE_TTL, PROBE_TIMEOUT, ProbeOutcome, UntrackedMode, response_from_outcome,
};
use crate::web::workspace::handlers::effective_workspace_base_canonical;

/// 返回当前工作区的只读 git 状态投影。
///
/// 一律 200：可用性靠 `state` 表达（`ok` / `not_a_repo` / `git_unavailable` / `failed`）。
/// `?refresh=true` 只跳过缓存**读**，仍回写缓存 —— 因此该 GET 保持幂等。
/// `?files=true` 才随响应返回文件清单；缓存里**永远**是全量，见 [`with_files_visibility`]。
pub async fn workspace_git_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<WorkspaceGitQuery>,
) -> Json<WorkspaceGitStatusResponse> {
    let want_files = query.files.unwrap_or(false);
    // 口径目前只有折叠一种；放在这里是为了让「谁取的数」一路可见（回显在 `untracked_mode`）。
    let mode = UntrackedMode::Folded;
    let raw_root = http.effective_workspace_path().await;
    if raw_root.trim().is_empty() {
        return Json(workspace_level_failure(
            error_codes::WORKSPACE_NOT_SET,
            "尚未设置工作区",
        ));
    }
    let root = match effective_workspace_base_canonical(&http).await {
        Ok(p) => p,
        // **不透传 `user_message()`**：`EffectiveRootOutsideAllowed` / `OutsideAllowedRoots` 的
        // Display 会把允许根的**绝对路径**拼进文案，既泄露服务器目录结构，也违反本路由
        // 「message 不含绝对路径」的契约（设计文档 §2.1）。按 `kind()` 映射成固定文案。
        Err(e) => {
            tracing::debug!(kind = e.kind(), "workspace git: 工作区根不可用");
            return Json(workspace_level_failure(
                error_codes::WORKSPACE_OUT_OF_ROOTS,
                workspace_root_failure_message(e.kind()),
            ));
        }
    };
    let root_key = root.to_string_lossy().to_string();
    let cache = http.workspace_git_cache.clone();
    let force = query.refresh.unwrap_or(false);

    if !force && let Some(hit) = read_fresh(&cache, &root_key, want_files) {
        return Json(hit);
    }

    // 最小 single-flight：TTL 到期瞬间的并发请求不应各自 fork 两条 git 命令。
    // 本仓没有通用 single-flight 设施（`per_turn_flight` 是单回合状态镜像），锁 + 双重检查足够。
    let _probe_guard = cache.probe_lock.lock().await;
    if !force && let Some(hit) = read_fresh(&cache, &root_key, want_files) {
        return Json(hit);
    }

    let outcome = probe_with_timeout(root, mode).await;
    match outcome {
        outcome @ (ProbeOutcome::Ok(_) | ProbeOutcome::NotARepo | ProbeOutcome::GitUnavailable) => {
            let response = response_from_outcome(
                outcome,
                git_projection::now_unix(),
                WorkspaceGitSource::Probe,
                mode,
            );
            // 明确结论（含 not_a_repo / git_unavailable）写缓存，省掉 TTL 内的重复 fork。
            // 写缓存时清单被**搬进** `Arc`（不复制），出站再按 `?files` 决定是否复制一份。
            let (skeleton, files) = write_cache(&cache, &root_key, response);
            Json(attach_files(skeleton, files.as_deref(), want_files))
        }
        ProbeOutcome::Failed { .. } => {
            // 重探失败时优先沿用旧值，避免一次超时把侧栏打成 failed。
            if let Some(stale) = read_stale(&cache, &root_key, want_files) {
                return Json(stale);
            }
            // 无旧值可兜底：不写缓存，避免把一次瞬时失败固化一整个 TTL。
            // 不可用态本来就没有清单，`None` 即「没有列表可给」。
            Json(attach_files(
                response_from_outcome(
                    outcome,
                    git_projection::now_unix(),
                    WorkspaceGitSource::Probe,
                    mode,
                ),
                None,
                want_files,
            ))
        }
    }
}

/// 按 `?files` 把清单挂到出站响应上：`false` 时置 `None`（字段缺失 = 本次没要列表）。
///
/// 清单以 `&WorkspaceGitFileList` 传入，只有真要返回时才 `.cloned()` 一次 ——
/// 无参请求因此**一次都不复制**清单（#1507）。
fn attach_files(
    mut response: WorkspaceGitStatusResponse,
    files: Option<&WorkspaceGitFileList>,
    want_files: bool,
) -> WorkspaceGitStatusResponse {
    response.files = if want_files { files.cloned() } else { None };
    response
}

/// 从缓存条目装配出站响应：改 `source`，按 `?files` 决定是否挂上清单。
///
/// 无参命中只克隆骨架（`files` 本来就是 `None`），不碰 `Arc` 里的清单。
fn materialize(
    entry: &CachedWorkspaceGitStatus,
    want_files: bool,
    source: WorkspaceGitSource,
) -> WorkspaceGitStatusResponse {
    let mut response = entry.skeleton.clone();
    response.source = source;
    if want_files {
        response.files = entry.files.as_deref().cloned();
    }
    response
}

/// 工作区根被拒时的固定文案（**不含**绝对路径）。
///
/// `WorkspacePathError` 的 `Display` 对 `EffectiveRootOutsideAllowed` 会带上允许根全路径，
/// 所以这里只按 `kind()` 分类，不透传原文。
fn workspace_root_failure_message(kind: &str) -> &'static str {
    match kind {
        "effective_root_sensitive" | "sensitive_path_denied" => "工作区根命中敏感目录黑名单",
        "effective_root_outside_allowed" | "outside_allowed_roots" => "工作区根不在允许范围内",
        _ => "工作区根无法解析",
    }
}

/// 工作区根层面的失败（未设置 / 越界），不经 probe。
fn workspace_level_failure(code: &str, message: &str) -> WorkspaceGitStatusResponse {
    WorkspaceGitStatusResponse {
        state: WorkspaceGitState::Failed,
        reason_code: Some(code.to_string()),
        message: Some(message.to_string()),
        head: None,
        upstream: None,
        ahead: None,
        behind: None,
        dirty: None,
        counts: None,
        files: None,
        // 未进 probe 就没取过数，口径仍是默认的折叠（客户端只看 `state`）。
        untracked_mode: WorkspaceGitUntrackedMode::Folded,
        computed_at_unix: git_projection::now_unix(),
        source: WorkspaceGitSource::Probe,
    }
}

/// 带墙钟上限的探测。
///
/// `spawn_blocking` 无法被取消，超时后那条线程仍会跑完再丢弃结果 —— 这是可接受的：
/// 单次探测上限 2s，且 single-flight 已把并发压成一次。
async fn probe_with_timeout(root: std::path::PathBuf, mode: UntrackedMode) -> ProbeOutcome {
    let handle = tokio::task::spawn_blocking(move || git_projection::probe(&root, mode));
    match tokio::time::timeout(PROBE_TIMEOUT, handle).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(join_error)) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_FAILED,
            message: format!("探测任务失败：{join_error}"),
        },
        Err(_) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_TIMEOUT,
            message: "git 探测超时".to_string(),
        },
    }
}

/// TTL 内的缓存命中（`source` 改写为 `cache`，`computed_at_unix` 保持探测时刻）。
fn read_fresh(
    cache: &WorkspaceGitCache,
    root_key: &str,
    want_files: bool,
) -> Option<WorkspaceGitStatusResponse> {
    let guard = cache.entry.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.workspace_root != root_key || entry.computed_at.elapsed() >= CACHE_TTL {
        return None;
    }
    Some(materialize(entry, want_files, WorkspaceGitSource::Cache))
}

/// 任意时长的旧值（供重探失败时兜底），`source` 改写为 `stale_cache`。
fn read_stale(
    cache: &WorkspaceGitCache,
    root_key: &str,
    want_files: bool,
) -> Option<WorkspaceGitStatusResponse> {
    let guard = cache.entry.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.workspace_root != root_key {
        return None;
    }
    Some(materialize(
        entry,
        want_files,
        WorkspaceGitSource::StaleCache,
    ))
}

/// 写入缓存，并把清单**搬进** `Arc`（不复制）。
///
/// 返回「骨架 + 那份 `Arc`」：调用方按 `?files` 决定要不要为出站再复制一次清单——
/// 无参请求因此一次都不复制（#1507）。锁中毒时静默跳过：缓存坏掉不该让只读端点失败，
/// 此时返回的 `files` 仍可用于本次出站。
fn write_cache(
    cache: &WorkspaceGitCache,
    root_key: &str,
    mut response: WorkspaceGitStatusResponse,
) -> (
    WorkspaceGitStatusResponse,
    Option<Arc<WorkspaceGitFileList>>,
) {
    // 骨架里 `files` 恒为 `None`，清单只留在 `Arc` 里，避免每次命中都深拷贝。
    let files = response.files.take().map(Arc::new);
    if let Ok(mut guard) = cache.entry.lock() {
        *guard = Some(CachedWorkspaceGitStatus {
            workspace_root: root_key.to_string(),
            computed_at: std::time::Instant::now(),
            skeleton: response.clone(),
            files: files.clone(),
        });
    }
    (response, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cm_api_contract::workspace::{
        WorkspaceGitFileEntry, WorkspaceGitFileKind, WorkspaceGitFileList,
    };

    fn sample() -> WorkspaceGitStatusResponse {
        WorkspaceGitStatusResponse {
            state: WorkspaceGitState::Ok,
            reason_code: None,
            message: None,
            head: None,
            upstream: None,
            ahead: None,
            behind: None,
            dirty: Some(false),
            counts: None,
            // 缓存条目**恒**带全量列表：`?files` 只影响出站，不影响缓存内容。
            files: Some(WorkspaceGitFileList {
                items: vec![WorkspaceGitFileEntry {
                    path: "a.txt".to_string(),
                    kind: WorkspaceGitFileKind::File,
                    staged: false,
                    unstaged: false,
                    untracked: true,
                    conflicted: false,
                }],
                truncated: false,
                limit: 500,
            }),
            untracked_mode: WorkspaceGitUntrackedMode::Folded,
            computed_at_unix: 1234,
            source: WorkspaceGitSource::Probe,
        }
    }

    /// 写一条缓存条目，形态与生产路径一致：骨架里 `files = None`，清单在 `Arc` 里。
    fn store(cache: &WorkspaceGitCache, root: &str, computed_at: std::time::Instant) {
        let mut skeleton = sample();
        let files = skeleton.files.take().map(Arc::new);
        let mut guard = cache.entry.lock().expect("cache lock");
        *guard = Some(CachedWorkspaceGitStatus {
            workspace_root: root.to_string(),
            computed_at,
            skeleton,
            files,
        });
    }

    /// 结构约定（#1507）：骨架**永不**携带清单，清单只存在于 `Arc` 里。
    ///
    /// 这条锁住「无参命中不深拷贝清单」的前提——一旦有人把 `files` 塞回骨架，
    /// 每次命中又会恢复成整份复制的旧行为。
    #[test]
    fn cache_skeleton_never_carries_files() {
        let cache = WorkspaceGitCache::default();
        store(&cache, "/w", std::time::Instant::now());

        let guard = cache.entry.lock().expect("cache lock");
        let entry = guard.as_ref().expect("entry");
        assert!(entry.skeleton.files.is_none(), "骨架不得携带清单");
        assert!(entry.files.is_some(), "清单应放在 Arc 里");
    }

    #[test]
    fn fresh_hit_within_ttl_keeps_computed_at_and_marks_cache() {
        let cache = WorkspaceGitCache::default();
        store(&cache, "/w", std::time::Instant::now());

        let hit = read_fresh(&cache, "/w", false).expect("fresh hit");
        assert_eq!(hit.source, WorkspaceGitSource::Cache);
        assert_eq!(hit.computed_at_unix, 1234, "命中缓存不应推进探测时刻");
        assert!(hit.files.is_none(), "无参命中不带清单");
        assert!(
            read_fresh(&cache, "/other", false).is_none(),
            "换工作区不应命中"
        );
    }

    #[test]
    fn expired_entry_is_not_fresh_but_serves_as_stale_cache() {
        let cache = WorkspaceGitCache::default();
        let old = std::time::Instant::now()
            .checked_sub(CACHE_TTL * 2)
            .expect("单调钟当前时刻必然大于 CACHE_TTL*2");
        store(&cache, "/w", old);

        assert!(
            read_fresh(&cache, "/w", false).is_none(),
            "过期条目不算命中"
        );

        let stale = read_stale(&cache, "/w", false).expect("stale fallback");
        assert_eq!(stale.source, WorkspaceGitSource::StaleCache);
        assert_eq!(stale.computed_at_unix, 1234, "兜底沿用旧探测时刻");
        assert!(
            read_stale(&cache, "/other", false).is_none(),
            "换工作区不应兜底"
        );
    }

    #[test]
    fn empty_cache_never_hits() {
        let cache = WorkspaceGitCache::default();
        assert!(read_fresh(&cache, "/w", true).is_none());
        assert!(read_stale(&cache, "/w", true).is_none());
    }

    /// #1498 回归点：缓存里始终留有**全量**清单，`?files` 只在出站剥离。
    ///
    /// 如果哪天有人按 `files` 布尔把响应写进缓存（或缓存写入用了已剥离的响应），
    /// 「先无参请求 → 再 `?files=true`」就会拿到 `counts` 有、列表空 的响应。
    #[test]
    fn plain_request_must_not_poison_cache_for_files_request() {
        let cache = WorkspaceGitCache::default();
        store(&cache, "/w", std::time::Instant::now());

        // 第一次：无参请求，出站不带清单。
        let plain = read_fresh(&cache, "/w", false).expect("fresh hit");
        assert!(plain.files.is_none(), "无参请求不应带列表");

        // 第二次：`?files=true` 命中同一缓存，必须仍能给出列表。
        let with_files = read_fresh(&cache, "/w", true).expect("fresh hit");
        assert!(with_files.files.is_some(), "缓存不得被前一次无参请求污染");
        assert_eq!(with_files.source, WorkspaceGitSource::Cache);
        assert_eq!(
            with_files.files.expect("files").items.len(),
            1,
            "列表内容应与写入缓存时一致"
        );
    }

    /// `stale_cache` 兜底路径同样按 `?files` 出站：重探失败时旧值里的列表必须完整保留。
    #[test]
    fn stale_cache_entry_still_serves_files_when_requested() {
        let cache = WorkspaceGitCache::default();
        let old = std::time::Instant::now()
            .checked_sub(CACHE_TTL * 2)
            .expect("单调钟当前时刻必然大于 CACHE_TTL*2");
        store(&cache, "/w", old);

        let stale = read_stale(&cache, "/w", true).expect("stale");
        assert_eq!(stale.source, WorkspaceGitSource::StaleCache);
        assert_eq!(
            stale.files.expect("stale 也要带列表").items.len(),
            1,
            "旧值里的列表必须完整保留"
        );

        // 无参请求同样不带清单。
        let plain = read_stale(&cache, "/w", false).expect("stale");
        assert!(plain.files.is_none());
        assert_eq!(plain.source, WorkspaceGitSource::StaleCache);
    }
}
