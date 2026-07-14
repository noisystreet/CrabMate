#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[cfg(not(target_os = "linux"))]
use tauri::webview::WebviewBuilder;
use tauri::{
    LogicalPosition, LogicalSize, Manager, Position, Rect, RunEvent, Size, Theme, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
    webview::{NewWindowFeatures, NewWindowResponse},
};
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use url::Url;

#[derive(Debug)]
struct BackendHandle {
    child: Arc<Mutex<Option<Child>>>,
}

impl BackendHandle {
    fn kill(&self) {
        let mut guard = self.child.lock().expect("backend mutex poisoned");
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *guard = None;
    }
}

fn backend_binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "crabmate.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "crabmate"
    }
}

const INSTALLED_FRONTEND_DIST: &str = "/usr/share/crabmate/frontend/dist";

fn installed_frontend_dist_path() -> Option<PathBuf> {
    let path = PathBuf::from(INSTALLED_FRONTEND_DIST);
    path.join("index.html").is_file().then_some(path)
}

fn user_home_workdir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// `CM_E2E_FIXTURES=1` 时隐藏 splash/main，避免 Wayland 桌面在 xvfb 外仍弹窗。
fn e2e_hide_app_windows() -> bool {
    std::env::var("CM_E2E_FIXTURES").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn dev_repo_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent()?.parent()?;
    if repo_root.join("frontend/Trunk.toml").is_file() || repo_root.join("Cargo.toml").is_file() {
        Some(repo_root.to_path_buf())
    } else {
        None
    }
}

fn resolve_backend_workdir() -> PathBuf {
    if let Ok(dir) = std::env::var("CM_DESKTOP_WORKDIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir.trim());
    }
    dev_repo_root().unwrap_or_else(user_home_workdir)
}

fn apply_backend_install_env(command: &mut Command) {
    if let Some(repo) = dev_repo_root() {
        let dist = repo.join("frontend/dist");
        if dist.join("index.html").is_file() {
            command.env("CM_WEB_STATIC_DIR", &dist);
        } else {
            command.env_remove("CM_WEB_STATIC_DIR");
        }
        return;
    }
    if let Some(dist) = installed_frontend_dist_path() {
        command.env("CM_WEB_STATIC_DIR", dist);
    }
}

fn sidecar_backend_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let bin = backend_binary_name();
    if let Ok(current_exe) = std::env::current_exe()
        && let Some(exe_dir) = current_exe.parent()
    {
        candidates.push(exe_dir.join(bin));
        candidates.push(exe_dir.join("sidecar").join(bin));
        candidates.push(exe_dir.join("resources").join("sidecar").join(bin));
    }
    candidates
}

fn resolve_backend_config_path() -> Option<PathBuf> {
    let candidate = PathBuf::from("/etc/crabmate/config.toml");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn configure_backend_serve_command(command: &mut Command, backend_config_path: &Option<PathBuf>) {
    command
        .arg("serve")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("0")
        .arg("--desktop-ready-json");
    if let Some(config_path) = backend_config_path.as_ref() {
        command.arg("--config").arg(config_path);
    }
}

fn parse_web_ready_url(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if v.get("event").and_then(|e| e.as_str()) != Some("web_ready") {
        return None;
    }
    v.get("url").and_then(|u| u.as_str()).map(str::to_string)
}

fn try_spawn_backend(backend_workdir: &std::path::Path) -> Result<Child, String> {
    let mut attempted = Vec::new();
    let mut last_err = String::new();
    let backend_config_path = resolve_backend_config_path();

    if let Ok(explicit) = std::env::var("CM_DESKTOP_BACKEND_BIN")
        && !explicit.trim().is_empty()
    {
        attempted.push(format!("env: {explicit}"));
        let mut command = Command::new(explicit.trim());
        configure_backend_serve_command(&mut command, &backend_config_path);
        apply_backend_install_env(&mut command);
        command
            .current_dir(backend_workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(e) => {
                last_err = format!("env backend spawn failed: {e}");
            }
        }
    }

    for candidate in sidecar_backend_candidates() {
        attempted.push(format!("sidecar: {}", candidate.display()));
        let mut command = Command::new(&candidate);
        configure_backend_serve_command(&mut command, &backend_config_path);
        apply_backend_install_env(&mut command);
        command
            .current_dir(backend_workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(e) => {
                last_err = format!(
                    "sidecar backend spawn failed ({}): {e}",
                    candidate.display()
                );
            }
        }
    }

    let path_bin = backend_binary_name();
    attempted.push(format!("PATH: {path_bin}"));
    let mut command = Command::new(path_bin);
    configure_backend_serve_command(&mut command, &backend_config_path);
    apply_backend_install_env(&mut command);
    command
        .current_dir(backend_workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match command.spawn() {
        Ok(child) => Ok(child),
        Err(e) => {
            if last_err.is_empty() {
                last_err = format!("PATH backend spawn failed: {e}");
            }
            Err(format!(
                "{last_err}; attempted backends: {}",
                attempted.join(" | ")
            ))
        }
    }
}

fn spawn_backend_and_wait_ready() -> Result<(Child, String), String> {
    let backend_workdir = resolve_backend_workdir();

    let mut child = try_spawn_backend(&backend_workdir).map_err(|e| {
        format!(
            "failed to spawn backend in `{}`: {e}",
            backend_workdir.display()
        )
    })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "backend stderr pipe unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "backend stdout pipe unavailable".to_string())?;

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[backend] {line}");
        }
    });

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = ready_tx.send(Err(
                        "backend stdout closed before web_ready JSON".to_string()
                    ));
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        println!("[backend] {trimmed}");
                    }
                    if let Some(url) = parse_web_ready_url(trimmed) {
                        let _ = ready_tx.send(Ok(url));
                        for rest in reader.lines().map_while(Result::ok) {
                            println!("[backend] {rest}");
                        }
                        break;
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("backend stdout read failed: {e}")));
                    break;
                }
            }
            if Instant::now() >= deadline {
                let _ = ready_tx.send(Err(
                    "timed out waiting for backend web_ready JSON".to_string()
                ));
                break;
            }
        }
    });

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("backend wait failed: {e}"))?
        {
            return Err(format!(
                "backend exited before web_ready (status: {status}); rebuild crabmate and ensure frontend/dist exists"
            ));
        }
        match ready_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(Ok(url)) => return Ok((child, url)),
            Ok(Err(e)) => return Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("backend stdout reader thread exited unexpectedly".to_string());
            }
        }
    }
}

#[tauri::command]
async fn save_text_file_via_dialog(
    app: tauri::AppHandle,
    default_name: String,
    content: String,
) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });

    let picked = rx
        .await
        .map_err(|e| format!("save dialog channel failed: {e}"))?;
    let Some(file_path) = picked else {
        return Ok(false);
    };

    let path = match file_path {
        FilePath::Path(p) => p,
        FilePath::Url(url) => url
            .to_file_path()
            .map_err(|_| "save dialog returned a non-file URL".to_string())?,
    };
    std::fs::write(&path, content).map_err(|e| format!("write file failed: {e}"))?;
    Ok(true)
}

#[tauri::command]
async fn pick_workspace_folder_via_dialog(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });

    let picked = rx
        .await
        .map_err(|e| format!("pick folder dialog channel failed: {e}"))?;

    Ok(match picked {
        None => None,
        Some(FilePath::Path(p)) => Some(p.to_string_lossy().into_owned()),
        Some(FilePath::Url(url)) => Some(
            url.to_file_path()
                .map_err(|_| "pick folder returned a non-file URL".to_string())?
                .to_string_lossy()
                .into_owned(),
        ),
    })
}

/// 是否在系统默认浏览器中打开（不留在 WebView 内导航）。
fn should_open_link_externally(app_origin: &url::Origin, target: &Url) -> bool {
    match target.scheme() {
        "http" | "https" | "mailto" => {}
        _ => return false,
    }
    target.origin() != *app_origin
}

fn is_github_host(url: &Url) -> bool {
    url.host_str().is_some_and(|h| {
        h == "github.com"
            || h.ends_with(".github.com")
            || h.ends_with(".githubusercontent.com")
            || h.ends_with(".githubassets.com")
    })
}

/// GitHub 专用 WebView 允许 http(s) 导航（含 OAuth 登录跳转）；其它 scheme 拒绝。
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn github_webview_allows_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") || url.as_str() == "about:blank"
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn github_webview_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("github-webview");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn create_github_webview_window(
    app: &tauri::AppHandle,
    parsed: Url,
    title: Option<String>,
    window_features: Option<NewWindowFeatures>,
) -> Result<WebviewWindow, String> {
    if !github_webview_allows_navigation(&parsed) {
        return Err("仅支持 http(s) URL".to_string());
    }

    let label = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        parsed.as_str().hash(&mut hasher);
        format!("github-{:016x}", hasher.finish())
    };
    if let Some(existing) = app.get_webview_window(&label) {
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(existing);
    }

    let data_dir = github_webview_data_dir(app)?;
    let window_title = title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            parsed
                .host_str()
                .map(|h| format!("GitHub — {h}"))
                .unwrap_or_else(|| "GitHub".to_string())
        });
    let app_for_handlers = app.clone();

    #[cfg(target_os = "linux")]
    let initial_url = Url::parse("about:blank").map_err(|e| format!("invalid blank url: {e}"))?;
    #[cfg(not(target_os = "linux"))]
    let initial_url = parsed.clone();

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(initial_url))
        .title(window_title)
        .inner_size(1120.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .resizable(true)
        .decorations(true)
        .visible(!e2e_hide_app_windows())
        .center()
        .data_directory(data_dir)
        .on_navigation(github_webview_allows_navigation)
        .on_new_window({
            let app = app_for_handlers.clone();
            move |url, features| {
                if !github_webview_allows_navigation(&url) {
                    return NewWindowResponse::Deny;
                }
                match create_github_webview_window(&app, url.clone(), None, Some(features)) {
                    Ok(window) => NewWindowResponse::Create { window },
                    Err(_) => NewWindowResponse::Deny,
                }
            }
        });

    if let Some(features) = window_features {
        builder = builder.window_features(features);
    }

    let window = builder
        .build()
        .map_err(|e| format!("create webview window failed: {e}"))?;

    // WebKitGTK 偶发会取消以 External URL 创建的新窗口首轮导航；
    // 先创建空页再显式导航更稳定，且不影响其它平台的嵌入路径。
    #[cfg(target_os = "linux")]
    window
        .navigate(parsed)
        .map_err(|e| format!("navigate webview window failed: {e}"))?;

    Ok(window)
}

#[tauri::command]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

fn main_webview_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window not found".into())
}

#[cfg(not(target_os = "linux"))]
fn main_tauri_window(app: &tauri::AppHandle) -> Result<tauri::Window, String> {
    app.get_window("main")
        .ok_or_else(|| "main window not found".into())
}

const GITHUB_EMBED_WEBVIEW_LABEL: &str = "github-embed";

static GITHUB_EMBED_LAST_URL: Mutex<Option<String>> = Mutex::new(None);
static GITHUB_EMBED_OP: Mutex<()> = Mutex::new(());

fn unmount_github_embed_webview_inner(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview(GITHUB_EMBED_WEBVIEW_LABEL) {
        let _ = existing.hide();
        let _ = existing.set_bounds(Rect {
            position: Position::Logical(LogicalPosition::new(0.0, 0.0)),
            size: Size::Logical(LogicalSize::new(0.0, 0.0)),
        });
        existing.close().map_err(|e| e.to_string())?;
    }
    if let Ok(mut last) = GITHUB_EMBED_LAST_URL.lock() {
        *last = None;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn github_embed_webview_builder(
    app: &tauri::AppHandle,
    parsed: Url,
) -> Result<WebviewBuilder<tauri::Wry>, String> {
    let data_dir = github_webview_data_dir(app)?;
    let app_for_handlers = app.clone();
    Ok(
        WebviewBuilder::new(GITHUB_EMBED_WEBVIEW_LABEL, WebviewUrl::External(parsed))
            .data_directory(data_dir)
            .on_navigation(github_webview_allows_navigation)
            .on_new_window(move |url, features| {
                if !github_webview_allows_navigation(&url) {
                    return NewWindowResponse::Deny;
                }
                match create_github_webview_window(
                    &app_for_handlers,
                    url.clone(),
                    None,
                    Some(features),
                ) {
                    Ok(window) => NewWindowResponse::Create { window },
                    Err(_) => NewWindowResponse::Deny,
                }
            }),
    )
}

#[tauri::command]
fn sync_github_embed_webview(
    app: tauri::AppHandle,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<bool, String> {
    let _guard = GITHUB_EMBED_OP
        .lock()
        .map_err(|e| format!("github embed op lock: {e}"))?;

    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("仅支持 https URL".to_string());
    }
    if !is_github_host(&parsed) {
        return Err("GitHub 嵌入仅支持 GitHub 域名".to_string());
    }
    if width < 1.0 || height < 1.0 {
        return Ok(true);
    }

    // Linux：独立 WebViewWindow 受限于 WebKitGTK 兼容性，易出现导航被取消；
    // 直接通过系统默认浏览器打开，确保可靠。
    #[cfg(target_os = "linux")]
    {
        let _ = (x, y);
        app.opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|e| e.to_string())?;
        Ok(false)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let bounds = Rect {
            position: Position::Logical(LogicalPosition::new(x, y)),
            size: Size::Logical(LogicalSize::new(width, height)),
        };

        let same_url = GITHUB_EMBED_LAST_URL
            .lock()
            .map_err(|e| e.to_string())?
            .as_deref()
            == Some(url.as_str());

        if let Some(existing) = app.get_webview(GITHUB_EMBED_WEBVIEW_LABEL) {
            if same_url {
                let _ = existing.set_auto_resize(false);
                existing.set_bounds(bounds).map_err(|e| e.to_string())?;
                let _ = existing.show();
                return Ok(true);
            }
            // URL 变更：关闭后重建，避免 navigate() 取消进行中的加载（WebKit「Operation was cancelled」）
            unmount_github_embed_webview_inner(&app)?;
        }

        let window = main_tauri_window(&app)?;
        let builder = github_embed_webview_builder(&app, parsed)?;
        let webview = window
            .add_child(
                builder,
                LogicalPosition::new(x, y),
                LogicalSize::new(width, height),
            )
            .map_err(|e| e.to_string())?;
        let _ = webview.set_auto_resize(false);
        webview.set_bounds(bounds).map_err(|e| e.to_string())?;
        let _ = webview.show();
        if let Ok(mut last) = GITHUB_EMBED_LAST_URL.lock() {
            *last = Some(url);
        }
        Ok(true)
    }
}

#[tauri::command]
fn unmount_github_embed_webview(app: tauri::AppHandle) -> Result<(), String> {
    let _guard = GITHUB_EMBED_OP
        .lock()
        .map_err(|e| format!("github embed op lock: {e}"))?;
    unmount_github_embed_webview_inner(&app)
}

#[tauri::command]
fn set_main_window_decorations(app: tauri::AppHandle, decorations: bool) -> Result<(), String> {
    main_webview_window(&app)?
        .set_decorations(decorations)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn main_window_minimize(app: tauri::AppHandle) -> Result<(), String> {
    main_webview_window(&app)?
        .minimize()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn main_window_toggle_maximize(app: tauri::AppHandle) -> Result<(), String> {
    let win = main_webview_window(&app)?;
    if win.is_maximized().map_err(|e| e.to_string())? {
        win.unmaximize().map_err(|e| e.to_string())
    } else {
        win.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn main_window_close(app: tauri::AppHandle) -> Result<(), String> {
    main_webview_window(&app)?
        .close()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn confirm_delete_session_via_dialog(
    app: tauri::AppHandle,
    message: String,
) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    app.dialog()
        .message(message)
        .title("确认删除会话")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "删除".to_string(),
            "取消".to_string(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await
        .map_err(|e| format!("confirm dialog channel failed: {e}"))
}

fn main() {
    let backend_state = Arc::new(Mutex::new(None::<Child>));
    let backend_state_for_exit = Arc::clone(&backend_state);

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(feature = "victauri")]
    {
        builder = builder.plugin(
            victauri_plugin::VictauriBuilder::new()
                .auth_disabled()
                .build()
                .unwrap(),
        );
    }
    builder
        .invoke_handler(tauri::generate_handler![
            save_text_file_via_dialog,
            pick_workspace_folder_via_dialog,
            confirm_delete_session_via_dialog,
            open_external_url,
            sync_github_embed_webview,
            unmount_github_embed_webview,
            set_main_window_decorations,
            main_window_minimize,
            main_window_toggle_maximize,
            main_window_close
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // 启动画面先显示，后台启后端（E2E 下 visible(false) 防弹窗）
            let show_window = !e2e_hide_app_windows();
            let _splash =
                WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
                    .title("CrabMate")
                    .inner_size(400.0, 300.0)
                    .resizable(false)
                    .decorations(false)
                    .visible(show_window)
                    .center()
                    .build()
                    .map_err(|e| format!("failed to create splash window: {e}"))?;

            std::thread::spawn(move || {
                let outcome = spawn_backend_and_wait_ready();
                let handle = app_handle.clone();
                // Tauri v2: 通过 evaluate_script 或事件在主线程处理
                match outcome {
                    Ok((child, ready_url)) => {
                        match create_main_window_from_url(
                            &handle,
                            ready_url,
                            child,
                            Arc::clone(&backend_state),
                        ) {
                            Ok(()) => {
                                if let Some(splash_win) = handle.get_webview_window("splash") {
                                    let _ = splash_win.close();
                                }
                            }
                            Err(e) => {
                                if let Some(splash_win) = handle.get_webview_window("splash") {
                                    let _ = splash_win.close();
                                }
                                handle
                                    .dialog()
                                    .message(e.clone())
                                    .title("CrabMate Desktop 启动失败")
                                    .kind(MessageDialogKind::Error)
                                    .blocking_show();
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(splash_win) = handle.get_webview_window("splash") {
                            let _ = splash_win.close();
                        }
                        handle
                            .dialog()
                            .message(e.clone())
                            .title("CrabMate Desktop 启动失败")
                            .kind(MessageDialogKind::Error)
                            .blocking_show();
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build tauri app")
        .run(move |_app_handle, event| {
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                let handle = BackendHandle {
                    child: Arc::clone(&backend_state_for_exit),
                };
                handle.kill();
            }
        });
}

fn create_main_window_from_url(
    app_handle: &tauri::AppHandle,
    ready_url: String,
    child: std::process::Child,
    backend_state: Arc<Mutex<Option<std::process::Child>>>,
) -> Result<(), String> {
    {
        let mut guard = backend_state.lock().expect("backend mutex poisoned");
        *guard = Some(child);
    }
    app_handle.manage(BackendHandle {
        child: backend_state,
    });

    let parsed_url: Url = ready_url
        .parse()
        .map_err(|e| format!("invalid backend ready url `{ready_url}`: {e}"))?;
    let app_origin = parsed_url.origin();
    let app_handle_clone = app_handle.clone();

    WebviewWindowBuilder::new(app_handle, "main", WebviewUrl::External(parsed_url.clone()))
        .title("CrabMate Desktop")
        .inner_size(1280.0, 840.0)
        .resizable(true)
        .decorations(false)
        .visible(!e2e_hide_app_windows())
        .theme(Some(Theme::Light))
        .on_navigation(move |url| {
            if should_open_link_externally(&app_origin, url) {
                let _ = app_handle_clone
                    .opener()
                    .open_url(url.as_str(), None::<&str>);
                return false;
            }
            true
        })
        .build()
        .map_err(|e| format!("failed to create main window: {e}"))?;

    Ok(())
}
