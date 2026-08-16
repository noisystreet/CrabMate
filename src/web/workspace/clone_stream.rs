//! `POST /workspace/clone/stream`：项目池内 `git clone --progress` 并以 SSE 推送进度，成功后切换工作区。

use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use crate::cm_web_host::http_types::workspace::WorkspaceCloneStreamBody;
use futures_util::Stream;
use serde_json::json;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;

use crate::web::app_state::AppStateHttpCore;
use crate::workspace::path::{is_sensitive_workspace_path, validate_workspace_set_path};
use crate::workspace::project::workspace_project_dir;

use super::clone_validate::{
    WORKSPACE_CLONE_TIMEOUT, parse_clone_progress_percent, redact_clone_log_line,
    split_progress_chunks, validate_clone_branch, validate_clone_repo_url,
};
use super::handlers::apply_workspace_override;

/// 同时进行的项目池 clone 上限（防磁盘/带宽打满）。
const MAX_CONCURRENT_CLONES: usize = 2;

static CLONE_SLOTS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_CLONES)));

type ClonePrecheck = (
    PathBuf,
    PathBuf,
    String,
    String,
    Option<u32>,
    Option<String>,
);

#[derive(Debug)]
enum CloneTargetError {
    Name(crate::workspace::project::WorkspaceProjectNameError),
    Sensitive,
    OutsidePool,
    Exists,
    Io(io::Error),
}

fn clone_http_err(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "ok": false,
            "code": code,
            "error": message,
        })),
    )
}

/// 解析目标路径并做策略预检（**不**创建目录；独占创建在流内 `claim_clone_target_dir`）。
fn resolve_clone_target(pool: &Path, name: &str) -> Result<PathBuf, CloneTargetError> {
    let dir = workspace_project_dir(pool, name).map_err(CloneTargetError::Name)?;
    if is_sensitive_workspace_path(&dir) {
        return Err(CloneTargetError::Sensitive);
    }
    if !dir.starts_with(pool) {
        return Err(CloneTargetError::OutsidePool);
    }
    if dir.exists() {
        return Err(CloneTargetError::Exists);
    }
    Ok(dir)
}

/// 用 `create_dir` 独占抢占目标目录，避免同名并发误删对方仓库。
fn claim_clone_target_dir(dir: &Path) -> Result<(), CloneTargetError> {
    match std::fs::create_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(CloneTargetError::Exists),
        Err(e) => Err(CloneTargetError::Io(e)),
    }
}

fn map_target_err(e: CloneTargetError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        CloneTargetError::Name(n) => {
            clone_http_err(StatusCode::BAD_REQUEST, "CLONE_BAD_NAME", &n.to_string())
        }
        CloneTargetError::Sensitive => clone_http_err(
            StatusCode::FORBIDDEN,
            "CLONE_FORBIDDEN",
            "项目路径不在允许范围（敏感路径）",
        ),
        CloneTargetError::OutsidePool => clone_http_err(
            StatusCode::FORBIDDEN,
            "CLONE_FORBIDDEN",
            "项目路径不在项目池内",
        ),
        CloneTargetError::Exists => clone_http_err(
            StatusCode::CONFLICT,
            "CLONE_DIR_EXISTS",
            "项目目录已存在，请换名称或先删除",
        ),
        CloneTargetError::Io(e) => clone_http_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CLONE_IO",
            &format!("无法创建项目目录: {e}"),
        ),
    }
}

fn map_target_err_sse(e: CloneTargetError) -> (String, String) {
    match e {
        CloneTargetError::Exists => (
            "CLONE_DIR_EXISTS".into(),
            "项目目录已存在，请换名称或先删除".into(),
        ),
        CloneTargetError::Io(err) => ("CLONE_IO".into(), format!("无法创建项目目录: {err}")),
        other => {
            let (_s, Json(v)) = map_target_err(other);
            (
                v.get("code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("CLONE_ERROR")
                    .to_string(),
                v.get("error")
                    .and_then(|c| c.as_str())
                    .unwrap_or("clone target error")
                    .to_string(),
            )
        }
    }
}

fn sse_data(v: serde_json::Value) -> Result<Event, axum::Error> {
    Ok(Event::default().data(v.to_string()))
}

/// `dest` 为 `.`（已独占空目录）或相对池根的目录名。
fn build_git_clone_args(
    url: &str,
    dest: &str,
    depth: Option<u32>,
    branch: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["clone".to_string(), "--progress".to_string()];
    if let Some(d) = depth.filter(|d| *d >= 1) {
        args.push("--depth".into());
        args.push(d.to_string());
    }
    if let Some(b) = branch.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--branch".into());
        args.push(b.to_string());
        args.push("--single-branch".into());
    }
    args.push(url.to_string());
    args.push(dest.to_string());
    args
}

async fn cleanup_partial_clone(pool: &Path, dir: &Path) {
    if !dir.starts_with(pool) || !dir.exists() {
        return;
    }
    if let Err(e) = tokio::fs::remove_dir_all(dir).await {
        warn!(error = %e, path = %dir.display(), "clone 失败后清理半成品目录失败");
    }
}

async fn pump_git_stderr_lines(
    stderr: tokio::process::ChildStderr,
    tx: mpsc::Sender<Result<Event, axum::Error>>,
    auth_hint: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stderr);
    let mut raw_buf = [0u8; 4096];
    let mut line_buf = String::new();
    loop {
        match reader.read(&mut raw_buf).await {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&raw_buf[..n]);
                for line in split_progress_chunks(&mut line_buf, &chunk) {
                    if line_suggests_git_auth_failure(&line) {
                        auth_hint.store(true, Ordering::Relaxed);
                    }
                    let redacted = redact_clone_log_line(&line);
                    if redacted.is_empty() {
                        continue;
                    }
                    if let Some((percent, label)) = parse_clone_progress_percent(&redacted) {
                        let _ = tx
                            .send(sse_data(json!({
                                "type": "progress",
                                "percent": percent,
                                "label": label,
                            })))
                            .await;
                    }
                    let _ = tx
                        .send(sse_data(json!({
                            "type": "log",
                            "line": redacted,
                        })))
                        .await;
                }
            }
            Err(_) => break,
        }
    }
    if !line_buf.trim().is_empty() {
        if line_suggests_git_auth_failure(&line_buf) {
            auth_hint.store(true, Ordering::Relaxed);
        }
        let redacted = redact_clone_log_line(&line_buf);
        let _ = tx
            .send(sse_data(json!({ "type": "log", "line": redacted })))
            .await;
    }
}

/// 较强的鉴权失败信号（已有 token 时才用于 `CLONE_AUTH_REQUIRED`）。
/// 不含「could not read from remote repository」等泛化 fatal（网络/DNS 也会出现）。
fn line_suggests_git_auth_failure(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("authentication failed")
        || l.contains("could not read username")
        || l.contains("invalid username or password")
        || l.contains("support for password authentication was removed")
        || l.contains("the requested url returned error: 401")
        || l.contains("the requested url returned error: 403")
}

fn clone_fail_code_and_message(
    url: &str,
    auth_hint: bool,
    exit_code: Option<i32>,
) -> (&'static str, String) {
    let github = crate::github_token::is_github_https_url(url);
    let has_token = crate::github_token::resolve_token_plaintext().is_some();
    // 无 token：GitHub HTTPS 失败一律引导连接；有 token：仅强鉴权信号才引导重连。
    if github && (!has_token || auth_hint) {
        (
            "CLONE_AUTH_REQUIRED",
            "git clone 失败：GitHub HTTPS 需要有效凭据。请在「设置 → GitHub」连接，或配置服务端 GH_TOKEN；SSH remote 不使用 OAuth token。"
                .into(),
        )
    } else {
        (
            "CLONE_GIT_FAILED",
            format!("git clone 失败（退出码 {exit_code:?}）"),
        )
    }
}

/// 客户端断开 SSE 时发取消信号。
struct CancelOnDropStream<S> {
    inner: S,
    cancel: Option<oneshot::Sender<()>>,
}

impl<S> Drop for CancelOnDropStream<S> {
    fn drop(&mut self) {
        if let Some(c) = self.cancel.take() {
            let _ = c.send(());
        }
    }
}

impl<S: Stream + Unpin> Stream for CancelOnDropStream<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

struct CloneJob {
    http: AppStateHttpCore,
    pool: PathBuf,
    target: PathBuf,
    name: String,
    url: String,
    depth: Option<u32>,
    branch: Option<String>,
    tx: mpsc::Sender<Result<Event, axum::Error>>,
    cancel: oneshot::Receiver<()>,
    /// 持有至任务结束，释放并发槽。
    _slot: tokio::sync::OwnedSemaphorePermit,
}

async fn send_err(tx: &mpsc::Sender<Result<Event, axum::Error>>, code: &str, message: &str) {
    let _ = tx
        .send(sse_data(json!({
            "type": "error",
            "code": code,
            "message": message,
        })))
        .await;
}

async fn run_clone_and_stream(job: CloneJob) {
    let CloneJob {
        http,
        pool,
        target,
        name,
        url,
        depth,
        branch,
        tx,
        mut cancel,
        _slot,
    } = job;

    if let Err(e) = claim_clone_target_dir(&target) {
        let (code, msg) = map_target_err_sse(e);
        send_err(&tx, &code, &msg).await;
        return;
    }

    let _ = tx
        .send(sse_data(json!({ "type": "phase", "phase": "clone" })))
        .await;

    if which_git().is_none() {
        cleanup_partial_clone(&pool, &target).await;
        send_err(&tx, "CLONE_NO_GIT", "服务端未找到 git 可执行文件").await;
        return;
    }

    // 已独占空目录：在目标目录内 `git clone <url> .`
    let args = build_git_clone_args(&url, ".", depth, branch.as_deref());
    let mut cmd = Command::new("git");
    cmd.args(&args)
        .current_dir(&target)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let _ = crate::github_token::apply_github_https_auth_pairs(&url, |k, v| {
        cmd.env(k, v);
    });

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            cleanup_partial_clone(&pool, &target).await;
            send_err(&tx, "CLONE_SPAWN_FAILED", &format!("无法启动 git: {e}")).await;
            return;
        }
    };

    let auth_hint = Arc::new(AtomicBool::new(false));
    let stderr = child.stderr.take();
    let stdout = child.stdout.take();
    let tx_err = tx.clone();
    let auth_hint_pump = Arc::clone(&auth_hint);
    let stderr_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            pump_git_stderr_lines(err, tx_err, auth_hint_pump).await;
        }
    });
    let stdout_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            let mut r = BufReader::new(out);
            let mut buf = String::new();
            let _ = r.read_to_string(&mut buf).await;
        }
    });

    let wait_fut = async { tokio::time::timeout(WORKSPACE_CLONE_TIMEOUT, child.wait()).await };

    let status = tokio::select! {
        _ = &mut cancel => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_task.await;
            let _ = stdout_task.await;
            cleanup_partial_clone(&pool, &target).await;
            send_err(&tx, "CLONE_CANCELLED", "克隆已取消（客户端断开）").await;
            return;
        }
        timed = wait_fut => match timed {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                let _ = stderr_task.await;
                let _ = stdout_task.await;
                cleanup_partial_clone(&pool, &target).await;
                send_err(&tx, "CLONE_GIT_FAILED", &format!("等待 git 失败: {e}")).await;
                return;
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stderr_task.await;
                let _ = stdout_task.await;
                cleanup_partial_clone(&pool, &target).await;
                send_err(&tx, "CLONE_TIMEOUT", "克隆超时（超过 20 分钟）").await;
                return;
            }
        }
    };
    let _ = stderr_task.await;
    let _ = stdout_task.await;

    if !status.success() {
        cleanup_partial_clone(&pool, &target).await;
        let (code, msg) =
            clone_fail_code_and_message(&url, auth_hint.load(Ordering::Relaxed), status.code());
        send_err(&tx, code, &msg).await;
        return;
    }

    if cancel.try_recv().is_ok() {
        // 仓已完整：保留目录（勿清理），由用户稍后用「选择工作区」打开。
        send_err(
            &tx,
            "CLONE_CANCELLED",
            "克隆完成但客户端已断开，未切换工作区；可用「选择工作区」打开",
        )
        .await;
        return;
    }

    let _ = tx
        .send(sse_data(json!({ "type": "phase", "phase": "activate" })))
        .await;

    let cfg = http.cfg.read().await;
    let canon = match validate_workspace_set_path(&cfg, &target.display().to_string()) {
        Ok(p) => p,
        Err(e) => {
            drop(cfg);
            // 完整仓保留，便于改名后用「选择工作区」打开。
            send_err(
                &tx,
                "CLONE_ACTIVATE_FAILED",
                &format!(
                    "仓库已克隆到 {}，但切换工作区失败: {}",
                    target.display(),
                    e.user_message()
                ),
            )
            .await;
            return;
        }
    };
    drop(cfg);
    let path_str = canon.display().to_string();
    apply_workspace_override(&http, &path_str).await;
    let _ = tx
        .send(sse_data(json!({
            "type": "done",
            "name": name,
            "path": path_str,
        })))
        .await;
}

fn which_git() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("git");
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let exe = dir.join("git.exe");
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
        None
    })
}

async fn precheck_clone_request(
    http: &AppStateHttpCore,
    body: &WorkspaceCloneStreamBody,
) -> Result<ClonePrecheck, (StatusCode, Json<serde_json::Value>)> {
    let url = validate_clone_repo_url(&body.url)
        .map_err(|e| clone_http_err(StatusCode::BAD_REQUEST, "CLONE_BAD_URL", e.user_message()))?;
    if body.depth == Some(0) {
        return Err(clone_http_err(
            StatusCode::BAD_REQUEST,
            "CLONE_BAD_DEPTH",
            "depth 须 >= 1",
        ));
    }
    let branch = match body.branch.as_ref() {
        None => None,
        Some(b) => {
            let t = b.trim();
            if t.is_empty() {
                None
            } else {
                Some(
                    validate_clone_branch(t)
                        .map_err(|m| {
                            clone_http_err(StatusCode::BAD_REQUEST, "CLONE_BAD_BRANCH", m)
                        })?
                        .to_string(),
                )
            }
        }
    };
    let cfg = http.cfg.read().await;
    let Some(pool) = cfg.workspace_roots.web_workspace_pool.clone() else {
        return Err(clone_http_err(
            StatusCode::NOT_FOUND,
            "CLONE_NO_POOL",
            "未配置 web_workspace_pool",
        ));
    };
    drop(cfg);
    if which_git().is_none() {
        return Err(clone_http_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "CLONE_NO_GIT",
            "服务端未找到 git 可执行文件",
        ));
    }
    let target = resolve_clone_target(pool.as_path(), body.name.trim()).map_err(map_target_err)?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| body.name.trim().to_string());
    Ok((pool, target, name, url.to_string(), body.depth, branch))
}

/// `POST /workspace/clone/stream`：SSE 进度；开流前参数错误返回 JSON。
pub async fn workspace_clone_stream_handler(
    State(http): State<AppStateHttpCore>,
    Json(body): Json<WorkspaceCloneStreamBody>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let slot = Arc::clone(&CLONE_SLOTS).try_acquire_owned().map_err(|_| {
        clone_http_err(
            StatusCode::TOO_MANY_REQUESTS,
            "CLONE_BUSY",
            "已有克隆任务进行中，请稍后再试",
        )
    })?;
    let (pool, target, name, url, depth, branch) = match precheck_clone_request(&http, &body).await
    {
        Ok(v) => v,
        Err(e) => {
            drop(slot);
            return Err(e);
        }
    };

    let (tx, rx) = mpsc::channel::<Result<Event, axum::Error>>(64);
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let http2 = http.clone();
    // `tokio::spawn` 不会继承 HTTP 中间件的 task-local；入队前捕获再挂回作用域。
    let github_token = crate::github_token::resolve_token_plaintext();
    tokio::spawn(async move {
        crate::github_token::with_request_github_token(github_token, async move {
            let _ = tx
                .send(sse_data(json!({ "type": "phase", "phase": "validate" })))
                .await;
            run_clone_and_stream(CloneJob {
                http: http2,
                pool,
                target,
                name,
                url,
                depth,
                branch,
                tx,
                cancel: cancel_rx,
                _slot: slot,
            })
            .await;
        })
        .await;
    });

    let stream = CancelOnDropStream {
        inner: ReceiverStream::new(rx),
        cancel: Some(cancel_tx),
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    #[test]
    fn git_clone_args_include_progress_and_depth() {
        let args = build_git_clone_args("https://example.com/a.git", ".", Some(1), Some("main"));
        assert!(args.windows(2).any(|w| w == ["--depth", "1"]));
        assert!(args.iter().any(|a| a == "--progress"));
        assert!(args.windows(2).any(|w| w == ["--branch", "main"]));
        assert!(args.iter().any(|a| a == "--single-branch"));
        assert_eq!(args.last().map(String::as_str), Some("."));
    }

    #[test]
    fn claim_dir_is_exclusive() {
        let root = tempfile::tempdir().expect("temp");
        let dir = root.path().join("demo");
        claim_clone_target_dir(&dir).expect("first claim");
        match claim_clone_target_dir(&dir) {
            Err(CloneTargetError::Exists) => {}
            other => panic!("expected Exists, got {other:?}"),
        }
    }

    #[test]
    fn clone_into_claimed_dir_from_bare() {
        let Some(_) = which_git() else {
            eprintln!("skip: no git");
            return;
        };
        let root = tempfile::tempdir().expect("temp");
        let bare = root.path().join("bare.git");
        let pool = root.path().join("pool");
        std::fs::create_dir_all(&pool).unwrap();
        let st = StdCommand::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .status()
            .expect("git init");
        assert!(st.success());
        let seed = root.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        assert!(
            StdCommand::new("git")
                .args(["clone", bare.to_str().unwrap(), "."])
                .current_dir(&seed)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            StdCommand::new("git")
                .args([
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "user.name=t",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "i"
                ])
                .current_dir(&seed)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            StdCommand::new("git")
                .args(["push", "origin", "HEAD:refs/heads/main"])
                .current_dir(&seed)
                .status()
                .unwrap()
                .success()
        );

        let target = pool.join("demo");
        claim_clone_target_dir(&target).expect("claim");
        let args = build_git_clone_args(bare.to_str().unwrap(), ".", Some(1), None);
        let st = StdCommand::new("git")
            .args(&args)
            .current_dir(&target)
            .env("LC_ALL", "C")
            .status()
            .unwrap();
        assert!(st.success(), "clone failed");
        assert!(target.join(".git").exists());
    }
}
