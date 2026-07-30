//! 桌面壳窗口生命周期：单实例唤醒、最小化到托盘与系统托盘。

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime};

const MAIN_WINDOW_LABEL: &str = "main";
const SPLASH_WINDOW_LABEL: &str = "splash";
const TRAY_ID: &str = "crabmate-main";
const TRAY_TOGGLE_ID: &str = "tray-toggle-main";
const TRAY_QUIT_ID: &str = "tray-quit";

#[derive(Default)]
struct DesktopLifecycleState {
    tray_available: AtomicBool,
}

/// 第二实例回调：优先唤醒主窗口；启动尚未完成时聚焦 splash。
pub(crate) fn focus_existing_instance<R: Runtime>(app: &AppHandle<R>) {
    if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        show_and_focus(&main);
    } else if let Some(splash) = app.get_webview_window(SPLASH_WINDOW_LABEL) {
        show_and_focus(&splash);
    }
}

/// 托盘可用时，主窗口的最小化动作应改为隐藏到托盘。
pub(crate) fn tray_available<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<DesktopLifecycleState>()
        .is_some_and(|state| state.tray_available.load(Ordering::Relaxed))
}

/// 初始化托盘菜单。若桌面环境不支持托盘，最小化保留为普通窗口最小化。
pub(crate) fn setup_tray(app: &App) {
    app.manage(DesktopLifecycleState::default());
    match build_tray(app) {
        Ok(()) => {
            app.state::<DesktopLifecycleState>()
                .tray_available
                .store(true, Ordering::Relaxed);
        }
        Err(error) => {
            eprintln!(
                "[crabmate-desktop] system tray unavailable; minimize will use the taskbar: {error}"
            );
        }
    }
}

fn build_tray(app: &App) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, TRAY_TOGGLE_ID, "显示/隐藏", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("CrabMate")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_TOGGLE_ID => toggle_main_window(app),
            TRAY_QUIT_ID => crate::request_desktop_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if is_primary_click(&event) {
                toggle_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        focus_existing_instance(app);
        return;
    };
    match main.is_visible() {
        Ok(true) => {
            if let Err(error) = main.hide() {
                eprintln!("[crabmate-desktop] failed to hide main window: {error}");
            }
        }
        Ok(false) => show_and_focus(&main),
        Err(error) => eprintln!("[crabmate-desktop] failed to read main visibility: {error}"),
    }
}

fn show_and_focus<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    if let Err(error) = window.show() {
        eprintln!("[crabmate-desktop] failed to show window: {error}");
        return;
    }
    let _ = window.unminimize();
    if let Err(error) = window.set_focus() {
        eprintln!("[crabmate-desktop] failed to focus window: {error}");
    }
}

fn is_primary_click(event: &TrayIconEvent) -> bool {
    match event {
        TrayIconEvent::Click {
            button,
            button_state,
            ..
        } => is_primary_click_parts(*button, *button_state),
        _ => false,
    }
}

fn is_primary_click_parts(button: MouseButton, button_state: MouseButtonState) -> bool {
    button == MouseButton::Left && button_state == MouseButtonState::Up
}

#[cfg(test)]
mod tests {
    use super::is_primary_click_parts;
    use tauri::tray::{MouseButton, MouseButtonState};

    #[test]
    fn only_released_left_click_toggles_window() {
        assert!(is_primary_click_parts(
            MouseButton::Left,
            MouseButtonState::Up
        ));
        assert!(!is_primary_click_parts(
            MouseButton::Right,
            MouseButtonState::Up
        ));
        assert!(!is_primary_click_parts(
            MouseButton::Left,
            MouseButtonState::Down
        ));
    }
}
