//! 主窗口创建：打开共用连接页，或（E2E）直连已运行的 `serve` URL。
//!
//! 桌面壳**不**拉起 `crabmate serve`；用户须自行启动后端或连接远程。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::webview::{Color, PageLoadEvent};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::{StateFlags, WindowExt};
use url::Url;

use crate::os_theme;
use crate::{close_splash_window, desktop_window_state_flags, e2e_hide_app_windows};

/// 主 UI（连上 `serve` 后）默认逻辑尺寸。
const MAIN_UI_WIDTH: f64 = 1280.0;
const MAIN_UI_HEIGHT: f64 = 840.0;

/// 启动闪屏与连接页共用逻辑尺寸（居中小窗，视觉一致）。
pub(crate) const BOOT_SHELL_WIDTH: f64 = 480.0;
pub(crate) const BOOT_SHELL_HEIGHT: f64 = 420.0;

/// WebView / 闪屏底色（与前端深色壳一致）。
pub(crate) const BOOT_SHELL_BG: Color = Color(0x0A, 0x0D, 0x12, 0xFF);

#[derive(Clone, Copy)]
enum MainWindowMode {
    /// 先打开连接页（与闪屏同尺寸小窗）；导航到 `serve` 后再放大并恢复几何。
    ConnectPage,
    /// E2E / 跳过连接页：直接全尺寸 UI（须已有可连的 `serve`）。
    DirectUi,
}

/// E2E / `CM_DESKTOP_SKIP_CONNECT`：直接打开已运行的 `serve` URL。
pub(crate) fn create_main_window_from_url(
    app_handle: &tauri::AppHandle,
    ready_url: String,
) -> Result<(), String> {
    let parsed_url: Url = ready_url
        .parse()
        .map_err(|e| format!("invalid serve url `{ready_url}`: {e}"))?;
    let backend_origin = parsed_url.origin();
    if let Some(allowed) = app_handle.try_state::<crabmate_connect::AllowedServeOrigin>() {
        allowed.set_from_url(&parsed_url);
    }
    finish_create_main_window(
        app_handle,
        WebviewUrl::External(parsed_url),
        backend_origin,
        MainWindowMode::DirectUi,
    )
    .map(|_| ())
}

/// 展示共用连接页；可选预填建议服务器 URL（经 `get_suggested_server_url`）。
///
/// 初始导航白名单不含任何 `serve` Origin；探测成功后由 `connect_remote` 写入
/// [`AllowedServeOrigin`](crabmate_connect::AllowedServeOrigin)。
pub(crate) fn create_main_window_connect_page(
    app_handle: &tauri::AppHandle,
    suggested_url: Option<String>,
) -> Result<(), String> {
    if let Some(url) = suggested_url.filter(|s| !s.trim().is_empty()) {
        app_handle
            .state::<crabmate_connect::SuggestedServerUrl>()
            .set(Some(url.trim().to_string()));
    }
    let placeholder = Url::parse("http://tauri.localhost/connect.html")
        .map_err(|e| format!("invalid connect placeholder url: {e}"))?;
    let backend_origin = placeholder.origin();
    let window = finish_create_main_window(
        app_handle,
        WebviewUrl::App("connect.html".into()),
        backend_origin,
        MainWindowMode::ConnectPage,
    )?;
    if let Ok(current) = window.url() {
        crabmate_connect::seed_connect_home(&current);
    } else {
        crabmate_connect::seed_connect_home(&placeholder);
    }
    Ok(())
}

fn is_connect_page_url(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    matches!(url.scheme(), "tauri" | "asset")
        || host.eq_ignore_ascii_case("tauri.localhost")
        || url.path().ends_with("connect.html")
}

fn apply_connect_page_geometry(window: &WebviewWindow) {
    // 会话窗可能处于最大化；不先取消则 set_size 无效，连接页会仍占满屏。
    let _ = window.unmaximize();
    let _ = window.set_resizable(false);
    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
        width: BOOT_SHELL_WIDTH,
        height: BOOT_SHELL_HEIGHT,
    }));
    let _ = window.center();
}

fn apply_main_ui_geometry(window: &WebviewWindow) {
    // 连接页小尺寸可能已写入 window-state；连上后恢复显示器位置，再最大化会话窗。
    // 不 restore SIZE/MAXIMIZED（连接页会把「未最大化」写脏）。
    let _ = window.set_resizable(true);
    let _ = window.restore_state(StateFlags::POSITION);
    if let Err(e) = window.maximize() {
        eprintln!("[crabmate-desktop] maximize after connect failed: {e}");
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: MAIN_UI_WIDTH,
            height: MAIN_UI_HEIGHT,
        }));
    }
}

fn finish_create_main_window(
    app_handle: &tauri::AppHandle,
    webview_url: WebviewUrl,
    backend_origin: url::Origin,
    mode: MainWindowMode,
) -> Result<WebviewWindow, String> {
    let app_handle_clone = app_handle.clone();
    let revealed = Arc::new(AtomicBool::new(false));
    let revealed_on_load = Arc::clone(&revealed);
    let app_on_load = app_handle.clone();
    let (width, height) = match mode {
        MainWindowMode::ConnectPage => (BOOT_SHELL_WIDTH, BOOT_SHELL_HEIGHT),
        MainWindowMode::DirectUi => (MAIN_UI_WIDTH, MAIN_UI_HEIGHT),
    };

    // 连接页：builder 即 `.center()`；Wayland 上未映射窗口的 `center()` 常被忽略，
    // 故在 `show()` 后再居中一次（见 `reveal_main_window_once`）。
    let mut builder = WebviewWindowBuilder::new(app_handle, "main", webview_url)
        .title("CrabMate Desktop")
        .inner_size(width, height)
        .resizable(matches!(mode, MainWindowMode::DirectUi))
        .decorations(false)
        .background_color(BOOT_SHELL_BG)
        .visible(false)
        .theme(os_theme::initial_window_theme());
    if matches!(mode, MainWindowMode::ConnectPage) {
        builder = builder.center();
    }
    let window = builder
        .on_navigation(move |url| {
            // 连接页 → 仅已探测通过的 AllowedServeOrigin；会话内跨源外开。
            match url.scheme() {
                "tauri" | "asset" => true,
                "mailto" => {
                    let _ = app_handle_clone
                        .opener()
                        .open_url(url.as_str(), None::<&str>);
                    false
                }
                "http" | "https" => {
                    let host = url.host_str().unwrap_or("");
                    if host.eq_ignore_ascii_case("tauri.localhost") {
                        return true;
                    }
                    let allowed = app_handle_clone
                        .try_state::<crabmate_connect::AllowedServeOrigin>()
                        .is_some_and(|s| s.matches_url(url));
                    let is_backend = url.origin() == backend_origin;
                    if let Some(w) = app_handle_clone.get_webview_window("main")
                        && let Ok(cur) = w.url()
                    {
                        if is_connect_page_url(&cur) {
                            if is_backend || allowed {
                                return true;
                            }
                            let _ = app_handle_clone
                                .opener()
                                .open_url(url.as_str(), None::<&str>);
                            return false;
                        }
                        if matches!(cur.scheme(), "http" | "https")
                            && cur.origin() == url.origin()
                        {
                            return true;
                        }
                        if matches!(cur.scheme(), "http" | "https")
                            && cur.origin() != url.origin()
                        {
                            let _ = app_handle_clone
                                .opener()
                                .open_url(url.as_str(), None::<&str>);
                            return false;
                        }
                    }
                    // 程序化 navigate / 尚无 current URL：仅放行已允许 Origin。
                    if is_backend || allowed {
                        return true;
                    }
                    let _ = app_handle_clone
                        .opener()
                        .open_url(url.as_str(), None::<&str>);
                    false
                }
                _ => false,
            }
        })
        .on_page_load(move |window, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished) {
                return;
            }
            // 连接页小窗；连上 serve（或从连接页返回）时切换几何。
            if is_connect_page_url(payload.url()) {
                apply_connect_page_geometry(&window);
            } else {
                apply_main_ui_geometry(&window);
            }
            reveal_main_window_once(&window, &app_on_load, &revealed_on_load);
        })
        .build()
        .map_err(|e| format!("failed to create main window: {e}"))?;

    match mode {
        MainWindowMode::ConnectPage => {
            // 勿 restore 上次全尺寸，否则连接页会被撑大。
            apply_connect_page_geometry(&window);
        }
        MainWindowMode::DirectUi => {
            if let Err(error) = window.restore_state(desktop_window_state_flags()) {
                eprintln!("[crabmate-desktop] failed to restore window state: {error}");
            }
        }
    }

    let app_fallback = app_handle.clone();
    let revealed_fallback = Arc::clone(&revealed);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let revealed = Arc::clone(&revealed_fallback);
        let app = app_fallback.clone();
        let _ = app_fallback.run_on_main_thread(move || {
            if let Some(main) = app.get_webview_window("main") {
                reveal_main_window_once(&main, &app, &revealed);
            } else {
                close_splash_window(&app);
            }
        });
    });

    Ok(window)
}

fn reveal_main_window_once(
    window: &WebviewWindow,
    app: &tauri::AppHandle,
    revealed: &AtomicBool,
) {
    if revealed.swap(true, Ordering::SeqCst) {
        return;
    }
    if !e2e_hide_app_windows() {
        let on_connect = window
            .url()
            .ok()
            .as_ref()
            .is_some_and(is_connect_page_url);
        if on_connect {
            apply_connect_page_geometry(window);
        }
        if let Err(e) = window.show() {
            eprintln!("[crabmate-desktop] failed to show main window: {e}");
        }
        // 显示后再居中：未映射时 `center()` 在部分合成器上无效。
        if on_connect
            && let Err(e) = window.center()
        {
            eprintln!("[crabmate-desktop] failed to center connect window: {e}");
        }
        if let Err(e) = window.set_focus() {
            eprintln!("[crabmate-desktop] failed to focus main window: {e}");
        }
    }
    close_splash_window(app);
}
