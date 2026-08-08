//! CrabMate Android 远程薄客户端库入口。
//! 不 spawn 本机 Agent sidecar；连接页探测远程 `serve` 后加载其 Web UI。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(crabmate_connect::SuggestedServerUrl::default())
        .manage(crabmate_connect::AllowedServeOrigin::default())
        .invoke_handler(tauri::generate_handler![
            crabmate_connect::connect_remote,
            crabmate_connect::disconnect_remote,
            crabmate_connect::get_suggested_server_url,
            crabmate_connect::get_connect_bearer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CrabMate mobile");
}
