#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent, Theme, WebviewUrl, WebviewWindowBuilder};
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
    if let Ok(dir) = std::env::var("CM_DESKTOP_WORKDIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir.trim());
        }
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
    v.get("url")
        .and_then(|u| u.as_str())
        .map(str::to_string)
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
                last_err = format!("sidecar backend spawn failed ({}): {e}", candidate.display());
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
                        "backend stdout closed before web_ready JSON".to_string(),
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
                    "timed out waiting for backend web_ready JSON".to_string(),
                ));
                break;
            }
        }
    });

    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("backend wait failed: {e}"))? {
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
async fn pick_workspace_folder_via_dialog(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
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

#[tauri::command]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

fn main_webview_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window not found".into())
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(victauri_plugin::VictauriBuilder::new().auth_disabled().build().unwrap())
        .invoke_handler(tauri::generate_handler![
            save_text_file_via_dialog,
            pick_workspace_folder_via_dialog,
            confirm_delete_session_via_dialog,
            open_external_url,
            set_main_window_decorations,
            main_window_minimize,
            main_window_toggle_maximize,
            main_window_close
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // 启动画面先显示，后台启后端
            let _splash = WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
                .title("CrabMate")
                .inner_size(400.0, 300.0)
                .resizable(false)
                .decorations(false)
                .center()
                .build()
                .map_err(|e| format!("failed to create splash window: {e}"))?;

            std::thread::spawn(move || {
                let outcome = spawn_backend_and_wait_ready();
                let handle = app_handle.clone();
                // Tauri v2: 通过 evaluate_script 或事件在主线程处理
                match outcome {
                    Ok((child, ready_url)) => {
                        // 关闭启动画面
                        if let Some(splash_win) = handle.get_webview_window("splash") {
                            let _ = splash_win.close();
                        }
                        // 尝试创建主窗口
                        match create_main_window_from_url(
                            &handle,
                            ready_url,
                            child,
                            Arc::clone(&backend_state),
                        ) {
                            Ok(()) => {}
                            Err(e) => {
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
        let mut guard = backend_state
            .lock()
            .expect("backend mutex poisoned");
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

    WebviewWindowBuilder::new(
        app_handle,
        "main",
        WebviewUrl::External(parsed_url.clone()),
    )
    .title("CrabMate Desktop")
    .inner_size(1280.0, 840.0)
    .resizable(true)
    .decorations(false)
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
