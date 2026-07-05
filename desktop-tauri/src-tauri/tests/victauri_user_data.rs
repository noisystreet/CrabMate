//! Victauri 版 user-data API E2E 测试（Phase 2：纯 API 播种/验证）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/user-data.spec.ts`     — GET/PUT prefs round-trip, PUT/GET sessions
//!   - `e2e/tests/user-data-mcp.spec.ts` — PUT/GET MCP servers, slug assign, status
//!
//! Phase 2 模式：`request.get/put/post()` → `eval_js("fetch(...)")` 通过 webview 同源调用后端。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_user_data`

use serde_json::Value;
use victauri_test::e2e_test;

/// 通过 webview fetch() 调用后端 API 并返回 JSON 结果。
async fn api_fetch(client: &mut victauri_test::VictauriClient, js_fetch: &str) -> Value {
    let result = client.eval_js(js_fetch).await.unwrap();
    // eval_js 返回的是 JSON raw string，需要解析
    if let Some(s) = result.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        result
    }
}

// ---------------------------------------------------------------------------
// user-data.spec.ts 测试 1: GET/PUT prefs round-trip
// ---------------------------------------------------------------------------
e2e_test!(get_put_prefs_roundtrip, |client| async move {
    // GET 初始 prefs
    let get0 = api_fetch(
        &mut client,
        "fetch('/user-data/prefs').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert!(get0.is_object(), "initial GET prefs should return object");

    // PUT 新 prefs
    let _ = client
        .eval_js(
            "fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'en',theme:'dark',side_panel_view:'workspace',side_width:300})})"
        )
        .await;

    // GET 验证
    let get1 = api_fetch(
        &mut client,
        "fetch('/user-data/prefs').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert_eq!(get1["locale"].as_str().unwrap_or(""), "en");
    assert_eq!(get1["theme"].as_str().unwrap_or(""), "dark");
    assert_eq!(get1["side_width"].as_i64().unwrap_or(0), 300);
});

// ---------------------------------------------------------------------------
// user-data.spec.ts 测试 2: PUT/GET current workspace sessions
// ---------------------------------------------------------------------------
e2e_test!(put_get_current_workspace_sessions, |client| async move {
    // 重置工作区
    let _ = client
        .eval_js(
            "fetch('/workspace',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:null})})"
        )
        .await;

    // PUT sessions
    let _ = client
        .eval_js(
            "fetch('/user-data/workspaces/current/sessions',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessions:[{id:'s_e2e_ud',title:'E2E user-data',draft:'',messages:[],updated_at:1,pinned:false,starred:false}],active_session_id:'s_e2e_ud'})})"
        )
        .await;

    // GET 验证
    let get = api_fetch(
        &mut client,
        "fetch('/user-data/workspaces/current/sessions').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert_eq!(get["active_session_id"].as_str().unwrap_or(""), "s_e2e_ud");
    let sessions = get["sessions"].as_array().unwrap();
    assert!(sessions.iter().any(|s| s["id"].as_str() == Some("s_e2e_ud")));
});

// ---------------------------------------------------------------------------
// user-data-mcp.spec.ts 测试 1: PUT assigns slug from name and GET round-trip
// ---------------------------------------------------------------------------
e2e_test!(put_mcp_assigns_slug_and_get_roundtrip, |client| async move {
    let _ = client
        .eval_js(
            "fetch('/user-data/mcp-servers',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,global_enabled:true,tool_timeout_secs:60,servers:[{id:'mcp_e2e_ud',name:'E2E Test Server',slug:'',command:'true',enabled:false,created_at_ms:0,updated_at_ms:0}]})})"
        )
        .await;

    let get = api_fetch(
        &mut client,
        "fetch('/user-data/mcp-servers').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert!(get["global_enabled"].as_bool().unwrap_or(false));
    let servers = get["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["id"].as_str().unwrap_or(""), "mcp_e2e_ud");
    assert_eq!(servers[0]["slug"].as_str().unwrap_or(""), "e2e_test_server");
    assert!(!servers[0]["enabled"].as_bool().unwrap_or(true));
    assert!(servers[0]["has_command"].as_bool().unwrap_or(false));
    // command 不应暴露
    assert!(servers[0].get("command").is_none());
});

// ---------------------------------------------------------------------------
// user-data-mcp.spec.ts 测试 2: GET status lists configured servers
// ---------------------------------------------------------------------------
e2e_test!(get_mcp_status_lists_servers, |client| async move {
    let _ = client
        .eval_js(
            "fetch('/user-data/mcp-servers',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({schema_version:1,global_enabled:false,tool_timeout_secs:45,servers:[{id:'mcp_e2e_status',name:'Status Probe',slug:'status_probe',command:'true',enabled:true,created_at_ms:0,updated_at_ms:0}]})})"
        )
        .await;

    // 验证 file GET
    let file_get = api_fetch(
        &mut client,
        "fetch('/user-data/mcp-servers').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert!(!file_get["global_enabled"].as_bool().unwrap_or(true));

    // 验证 status GET
    let status = api_fetch(
        &mut client,
        "fetch('/user-data/mcp-servers/status').then(r=>r.json()).then(d=>JSON.stringify(d))"
    ).await;
    assert!(!status["global_enabled"].as_bool().unwrap_or(true));
    assert_eq!(status["tool_timeout_secs"].as_i64().unwrap_or(0), 45);
    let rows = status["servers"].as_array().unwrap();
    let row = rows.iter().find(|r| r["id"].as_str() == Some("mcp_e2e_status")).unwrap();
    assert_eq!(row["slug"].as_str().unwrap_or(""), "status_probe");
    assert!(row["enabled"].as_bool().unwrap_or(false));
    assert!(!row["connected"].as_bool().unwrap_or(true));
});
