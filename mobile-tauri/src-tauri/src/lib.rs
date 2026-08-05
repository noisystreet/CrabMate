//! CrabMate Android 远程薄客户端库入口。
//! 不 spawn 本机 Agent sidecar；连接页探测远程 `serve` 后加载其 Web UI。

mod connect;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            connect::connect_remote,
            connect::disconnect_remote
        ])
        .run(tauri::generate_context!())
        .expect("error while running CrabMate mobile");
}
