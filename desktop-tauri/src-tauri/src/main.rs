#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent, Theme, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogButtons, MessageDialogKind};

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
            .arg("3000")
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
            .arg("3000")
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
        .arg("3000")
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
    let backend_url = "http://127.0.0.1:3000".to_string();
    let backend_addr = "127.0.0.1:3000";

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

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[backend] {line}");
        }
    });

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            println!("[backend] {line}");
        }
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if TcpStream::connect(backend_addr).is_ok() {
            break;
        }
        if let Some(status) = child.try_wait().map_err(|e| format!("backend wait failed: {e}"))? {
            return Err(format!(
                "backend exited before becoming ready (status: {status})"
            ));
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for backend to listen on 127.0.0.1:3000".to_string());
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    Ok((child, backend_url))
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
    let backend_state_for_setup = Arc::clone(&backend_state);
    let backend_state_for_exit = Arc::clone(&backend_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            save_text_file_via_dialog,
            confirm_delete_session_via_dialog
        ])
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
