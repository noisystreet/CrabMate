#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop_lifecycle;
mod desktop_main_window;
mod os_theme;

use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::StateFlags;
use url::Url;

pub(crate) fn desktop_window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

/// 连接页默认建议地址（本机常见 `serve` 端口；用户可改）。
const DEFAULT_SUGGESTED_SERVE_URL: &str = "http://127.0.0.1:8080/";

/// `CM_E2E_FIXTURES=1` 时隐藏 splash/main，避免 Wayland 桌面在 xvfb 外仍弹窗。
pub(crate) fn e2e_hide_app_windows() -> bool {
    std::env::var("CM_E2E_FIXTURES").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// E2E 或显式跳过时：不展示连接页，直接打开 **`CM_DESKTOP_SERVE_URL`**（须已有 `serve`）。
pub(crate) fn skip_connect_page() -> bool {
    e2e_hide_app_windows()
        || std::env::var("CM_DESKTOP_SKIP_CONNECT").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "0")
}

/// 连接页预填：`CM_DESKTOP_SUGGESTED_URL` → 默认本机 8080。
fn suggested_serve_url() -> String {
    env_nonempty("CM_DESKTOP_SUGGESTED_URL")
        .unwrap_or_else(|| DEFAULT_SUGGESTED_SERVE_URL.to_string())
}

/// 跳过连接页时必填的已运行 `serve` URL。
fn direct_serve_url() -> Result<String, String> {
    env_nonempty("CM_DESKTOP_SERVE_URL").ok_or_else(|| {
        "跳过连接页时须设置 CM_DESKTOP_SERVE_URL（指向已运行的 crabmate serve，例如 http://127.0.0.1:8080/）。桌面壳不再拉起后端。"
            .to_string()
    })
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
                width: desktop_main_window::BOOT_SHELL_WIDTH,
                height: desktop_main_window::BOOT_SHELL_HEIGHT + 80.0,
            }));
            let _ = splash.center();
        }
    });
}

pub(crate) fn close_splash_window(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(splash) = handle.get_webview_window("splash") {
            let _ = splash.close();
        }
    });
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

#[tauri::command]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 托盘「退出」与 splash/前端显式退出共用。
pub(crate) fn request_desktop_quit(app: &tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn quit_desktop_app(app: tauri::AppHandle) {
    request_desktop_quit(&app);
}

/// 与建窗逻辑一致：Linux 读 gsettings；其它平台 `None`（前端用 matchMedia）。
#[tauri::command]
fn os_prefers_dark_theme() -> Option<bool> {
    os_theme::os_prefers_dark_theme()
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

fn open_main_after_splash(app: &tauri::AppHandle) -> Result<(), String> {
    if skip_connect_page() {
        let url = direct_serve_url()?;
        desktop_main_window::create_main_window_from_url(app, url)
    } else {
        desktop_main_window::create_main_window_connect_page(app, Some(suggested_serve_url()))
    }
}

fn main() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            desktop_lifecycle::focus_existing_instance(app);
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["splash"])
                // 主窗口在闪屏后才建，几何恢复由 `create_main_window_*`
                // 在 `show()` 前同步触发，避免插件异步 restore 造成默认尺寸闪一下。
                .skip_initial_state("main")
                .with_state_flags(desktop_window_state_flags())
                .build(),
        )
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
            quit_desktop_app,
            os_prefers_dark_theme,
            crabmate_connect::connect_remote,
            crabmate_connect::disconnect_remote,
            crabmate_connect::get_suggested_server_url,
            crabmate_connect::get_connect_bearer,
        ])
        .setup(move |app| {
            desktop_lifecycle::setup_tray(app);
            os_theme::spawn_linux_color_scheme_watcher(app.handle().clone());
            app.manage(crabmate_connect::SuggestedServerUrl::default());
            app.manage(crabmate_connect::AllowedServeOrigin::default());
            let app_handle = app.handle().clone();

            let show_window = !e2e_hide_app_windows();
            let splash =
                WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
                    .title("CrabMate")
                    .inner_size(
                        desktop_main_window::BOOT_SHELL_WIDTH,
                        desktop_main_window::BOOT_SHELL_HEIGHT,
                    )
                    .resizable(false)
                    .decorations(false)
                    .background_color(desktop_main_window::BOOT_SHELL_BG)
                    .visible(false)
                    .center()
                    .build()
                    .map_err(|e| format!("failed to create splash window: {e}"))?;
            if show_window {
                let _ = splash.show();
                let _ = splash.center();
                let _ = splash.set_focus();
            }

            update_splash_status(
                &app_handle,
                "正在启动…",
                "打开连接页（请先自行启动 crabmate serve）",
            );

            std::thread::spawn(move || {
                let handle = app_handle.clone();
                update_splash_status(&handle, "正在打开界面…", "加载连接页（请稍候）");
                let create_result = {
                    let h = handle.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    let _ = handle.run_on_main_thread(move || {
                        let r = open_main_after_splash(&h);
                        let _ = tx.send(r);
                    });
                    rx.recv()
                        .unwrap_or_else(|_| Err("main window create channel failed".into()))
                };
                match create_result {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("[crabmate-desktop] startup failed: {e}");
                        show_splash_error(&handle, e.clone());
                        if !e2e_hide_app_windows() {
                            handle
                                .dialog()
                                .message(format!(
                                    "{e}\n\n请先在本机或远程启动 crabmate serve，再在连接页填写地址。"
                                ))
                                .title("CrabMate Desktop")
                                .kind(MessageDialogKind::Error)
                                .blocking_show();
                        }
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build tauri app")
        .run(|_app_handle, _event| {
            // 壳不再持有后端子进程；Exit 无需额外回收。
        });
}

#[cfg(test)]
mod tests {
    use super::desktop_window_state_flags;
    use tauri_plugin_window_state::StateFlags;

    #[test]
    fn window_state_flags_include_geometry() {
        let flags = desktop_window_state_flags();
        assert!(flags.contains(StateFlags::SIZE));
        assert!(flags.contains(StateFlags::POSITION));
        assert!(flags.contains(StateFlags::MAXIMIZED));
    }
}
