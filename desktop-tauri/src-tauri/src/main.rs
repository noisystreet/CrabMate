#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Manager, RunEvent, Theme, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

#[derive(Debug, Clone, Deserialize)]
struct WebReadyEvent {
    event: String,
    url: String,
}

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

fn resolve_backend_workdir() -> PathBuf {
    if let Ok(dir) = std::env::var("CRABMATE_DESKTOP_WORKDIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }

    // 开发场景下 CARGO_MANIFEST_DIR 为 desktop-tauri/src-tauri，仓库根在上两级。
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest_dir);
    repo_root
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

fn try_spawn_backend(backend_workdir: &std::path::Path) -> Result<Child, String> {
    let mut attempted = Vec::new();
    let mut last_err = String::new();

    if let Ok(explicit) = std::env::var("CRABMATE_DESKTOP_BACKEND_BIN")
        && !explicit.trim().is_empty()
    {
        attempted.push(format!("env: {explicit}"));
        let mut command = Command::new(explicit.trim());
        command
            .arg("serve")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg("0")
            .arg("--desktop-ready-json")
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
        command
            .arg("serve")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg("0")
            .arg("--desktop-ready-json")
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
    command
        .arg("serve")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("0")
        .arg("--desktop-ready-json")
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "backend stdout pipe unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "backend stderr pipe unavailable".to_string())?;

    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[backend] {line}");
        }
    });

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    if let Ok(event) = serde_json::from_str::<WebReadyEvent>(&line) {
                        if event.event == "web_ready" {
                            let _ = ready_tx.send(Ok(event.url));
                            return;
                        }
                    }
                    println!("[backend] {line}");
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("read backend stdout failed: {e}")));
                    return;
                }
            }
        }
        let _ = ready_tx.send(Err(
            "backend exited before emitting web_ready event".to_string()
        ));
    });

    let ready_url = ready_rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| "timed out waiting for backend ready event".to_string())?
        .map_err(|e| format!("backend did not become ready: {e}"))?;

    Ok((child, ready_url))
}

fn main() {
    let backend_state = Arc::new(Mutex::new(None::<Child>));
    let backend_state_for_setup = Arc::clone(&backend_state);
    let backend_state_for_exit = Arc::clone(&backend_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let (child, ready_url) = match spawn_backend_and_wait_ready() {
                Ok(v) => v,
                Err(e) => {
                    let msg = format!("desktop backend bootstrap failed: {e}");
                    app.dialog()
                        .message(msg.clone())
                        .title("CrabMate Desktop 启动失败")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                    return Err(msg.into());
                }
            };

            {
                let mut guard = backend_state_for_setup
                    .lock()
                    .expect("backend mutex poisoned");
                *guard = Some(child);
            }
            app.manage(BackendHandle {
                child: Arc::clone(&backend_state_for_setup),
            });

            let parsed_url = ready_url
                .parse()
                .map_err(|e| format!("invalid backend ready url `{ready_url}`: {e}"))?;

            if let Err(e) = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed_url))
                .title("CrabMate Desktop")
                .inner_size(1280.0, 840.0)
                .resizable(true)
                .theme(Some(Theme::Light))
                .build()
            {
                let msg = format!("failed to create main window: {e}");
                app.dialog()
                    .message(msg.clone())
                    .title("CrabMate Desktop 启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                return Err(msg.into());
            }

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
