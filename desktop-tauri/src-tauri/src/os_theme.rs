//! Linux 上解析 OS 明暗，供主窗 `theme(Some(...))` 使用。
//!
//! GNOME 等桌面常用 `org.gnome.desktop.interface color-scheme=prefer-dark`，
//! 同时 GTK 主题名仍为 `Adwaita`（无 `-dark` 后缀）。WebKitGTK / tao 在
//! `theme(None)` 时不会据此打开 `gtk-application-prefer-dark-theme`，
//! WebView 的 `prefers-color-scheme` 会恒为浅色，导致前端 `theme=system` 失效。

use tauri::{AppHandle, Theme};

#[cfg(target_os = "linux")]
use std::io::{BufRead, BufReader};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use tauri::Manager;

/// 创建主窗时使用的主题：Linux 显式 Dark/Light；其它平台 `None` 交 OS。
#[must_use]
pub fn initial_window_theme() -> Option<Theme> {
    #[cfg(target_os = "linux")]
    {
        Some(detect_linux_color_scheme_theme())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// 供前端 invoke：Linux 上是否应视为深色；其它平台返回 `None`（由 WebView matchMedia 决定）。
#[must_use]
pub fn os_prefers_dark_theme() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        Some(matches!(detect_linux_color_scheme_theme(), Theme::Dark))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_color_scheme_theme() -> Theme {
    if let Some(scheme) = gsettings_get("org.gnome.desktop.interface", "color-scheme") {
        let s = scheme.to_ascii_lowercase();
        if s.contains("prefer-dark") {
            return Theme::Dark;
        }
        if s.contains("prefer-light") {
            return Theme::Light;
        }
    }
    if let Some(gtk_theme) = gsettings_get("org.gnome.desktop.interface", "gtk-theme") {
        let s = gtk_theme.to_ascii_lowercase();
        if s.contains("-dark") || s.contains(":dark") || s.ends_with("darker") {
            return Theme::Dark;
        }
    }
    Theme::Light
}

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 监听 GNOME `color-scheme` 变化并更新主窗主题（使 WebView matchMedia 跟随）。
pub fn spawn_linux_color_scheme_watcher(app: AppHandle) {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
    }
    #[cfg(target_os = "linux")]
    {
        thread::Builder::new()
            .name("cm-os-theme".into())
            .spawn(move || {
                let Ok(mut child) = Command::new("gsettings")
                    .args([
                        "monitor",
                        "org.gnome.desktop.interface",
                        "color-scheme",
                    ])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    return;
                };
                let Some(stdout) = child.stdout.take() else {
                    return;
                };
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let lower = line.to_ascii_lowercase();
                    let theme = if lower.contains("prefer-dark") {
                        Theme::Dark
                    } else if lower.contains("prefer-light") {
                        Theme::Light
                    } else {
                        detect_linux_color_scheme_theme()
                    };
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(win) = handle.get_webview_window("main") {
                            let _ = win.set_theme(Some(theme));
                        }
                    });
                }
            })
            .ok();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_dark_or_light() {
        let t = detect_linux_color_scheme_theme();
        assert!(matches!(t, Theme::Dark | Theme::Light));
    }
}
