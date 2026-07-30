#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop_lifecycle;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
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
    let seed = crabmate_config::ensure_user_config_seeded_from_system();
    let user = crabmate_config::user_config_toml_path();
    if user.is_file() {
        return Some(user);
    }
    // 种子失败且尚无用户副本时，只读回退系统模板（日常路径仍以 XDG 用户副本为准）。
    if let Err(e) = seed {
        eprintln!("[crabmate-desktop] seed XDG config from /etc/crabmate: {e}");
        let system = crabmate_config::system_config_toml_path();
        if system.is_file() {
            eprintln!(
                "[crabmate-desktop] falling back to read-only {}",
                system.display()
            );
            return Some(system);
        }
    }
    None
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

/// 通过 `eval` 更新启动页文案（须在主线程调用）。
fn splash_eval_status(splash: &WebviewWindow, status: &str, detail: &str) {
    let status_js = serde_json::to_string(status).unwrap_or_else(|_| "\"\"".into());
    let detail_js = serde_json::to_string(detail).unwrap_or_else(|_| "\"\"".into());
    let _ = splash.eval(format!(
        "window.setSplashStatus && window.setSplashStatus({status_js}, {detail_js});"
    ));
}

fn splash_eval_error(splash: &WebviewWindow, message: &str) {
    let msg_js = serde_json::to_string(message).unwrap_or_else(|_| "\"启动失败\"".into());
    let _ = splash.eval(format!(
        "window.setSplashError && window.setSplashError({msg_js});"
    ));
}

fn update_splash_status(app: &tauri::AppHandle, status: &str, detail: &str) {
    let status = status.to_string();
    let detail = detail.to_string();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(splash) = handle.get_webview_window("splash") {
            splash_eval_status(&splash, &status, &detail);
        }
    });
}

fn show_splash_error(app: &tauri::AppHandle, message: String) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(splash) = handle.get_webview_window("splash") {
            splash_eval_error(&splash, &message);
            let _ = splash.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 480.0,
                height: 420.0,
            }));
            let _ = splash.center();
        }
    });
}

fn close_splash_window(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(splash) = handle.get_webview_window("splash") {
            let _ = splash.close();
        }
    });
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

fn spawn_backend_and_wait_ready(
    on_progress: impl Fn(&str, &str),
) -> Result<(Child, String), String> {
    let backend_workdir = resolve_backend_workdir();
    on_progress("正在拉起本地后端…", "解析可执行文件与配置");

    let mut child = try_spawn_backend(&backend_workdir).map_err(|e| {
        format!(
            "failed to spawn backend in `{}`: {e}",
            backend_workdir.display()
        )
    })?;
    on_progress("等待服务就绪…", "监听 web_ready（最长约 30 秒）");
    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("backend stderr pipe unavailable".to_string());
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("backend stdout pipe unavailable".to_string());
        }
    };

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
        if let Some(status) = child.try_wait().map_err(|e| {
            let _ = child.kill();
            let _ = child.wait();
            format!("backend wait failed: {e}")
        })? {
            return Err(format!(
                "backend exited before web_ready (status: {status}); rebuild crabmate and ensure frontend/dist exists"
            ));
        }
        match ready_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(Ok(url)) => return Ok((child, url)),
            Ok(Err(e)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                let _ = child.wait();
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

#[tauri::command]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 托盘「退出」与 splash/前端显式退出共用：先 kill 后端再结束壳进程。
pub(crate) fn request_desktop_quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<BackendHandle>() {
        state.kill();
    }
    app.exit(0);
}

#[tauri::command]
fn quit_desktop_app(app: tauri::AppHandle) {
    request_desktop_quit(&app);
}

fn main_webview_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
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
    let window = main_webview_window(&app)?;
    if desktop_lifecycle::tray_available(&app) {
        window.hide().map_err(|e| e.to_string())
    } else {
        window.minimize().map_err(|e| e.to_string())
    }
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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            desktop_lifecycle::focus_existing_instance(app);
        }))
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
            set_main_window_decorations,
            main_window_minimize,
            main_window_toggle_maximize,
            main_window_close,
            quit_desktop_app
        ])
        .setup(move |app| {
            desktop_lifecycle::setup_tray(app);
            let app_handle = app.handle().clone();

            // 启动画面先显示，后台启后端（E2E 下 visible(false) 防弹窗）
            let show_window = !e2e_hide_app_windows();
            let _splash =
                WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
                    .title("CrabMate")
                    .inner_size(440.0, 340.0)
                    .resizable(false)
                    .decorations(false)
                    .visible(show_window)
                    .center()
                    .build()
                    .map_err(|e| format!("failed to create splash window: {e}"))?;

            update_splash_status(&app_handle, "正在启动…", "准备本地后端服务");

            std::thread::spawn(move || {
                let handle = app_handle.clone();
                let progress = |status: &str, detail: &str| {
                    let status = status.to_string();
                    let detail = detail.to_string();
                    let h = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Some(splash) = h.get_webview_window("splash") {
                            splash_eval_status(&splash, &status, &detail);
                        }
                    });
                };
                let outcome = spawn_backend_and_wait_ready(progress);
                match outcome {
                    Ok((child, ready_url)) => {
                        update_splash_status(&handle, "正在打开界面…", "后端已就绪");
                        let create_result = {
                            let h = handle.clone();
                            let state = Arc::clone(&backend_state);
                            let (tx, rx) = std::sync::mpsc::channel();
                            let _ = handle.run_on_main_thread(move || {
                                let r = create_main_window_from_url(&h, ready_url, child, state);
                                let _ = tx.send(r);
                            });
                            rx.recv()
                                .unwrap_or_else(|_| Err("main window create channel failed".into()))
                        };
                        match create_result {
                            Ok(()) => close_splash_window(&handle),
                            Err(e) => {
                                show_splash_error(&handle, e);
                                if !e2e_hide_app_windows() {
                                    handle
                                        .dialog()
                                        .message(
                                            "主窗口未能创建。详情见启动页；可点击「退出」关闭。",
                                        )
                                        .title("CrabMate Desktop")
                                        .kind(MessageDialogKind::Error)
                                        .blocking_show();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[crabmate-desktop] startup failed: {e}");
                        show_splash_error(&handle, e);
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

#[cfg(test)]
mod tests {
    use super::parse_web_ready_url;

    #[test]
    fn parse_web_ready_url_extracts_url() {
        let line =
            r#"{"event":"web_ready","host":"127.0.0.1","port":9,"url":"http://127.0.0.1:9/"}"#;
        assert_eq!(
            parse_web_ready_url(line).as_deref(),
            Some("http://127.0.0.1:9/")
        );
        assert!(parse_web_ready_url(r#"{"event":"other","url":"http://x/"}"#).is_none());
        assert!(parse_web_ready_url("not json").is_none());
    }
}
