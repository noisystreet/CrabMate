//! Victauri 版 conversation 分页 API + 水合 E2E 测试（Phase 2：需播种，无流存根）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/conversation-messages-pagination.spec.ts` — 分页 API + 水合验证
//!
//! Phase 2 播种模式：`seedConversation(request, ...)` → `eval_js("fetch('/e2e/fixtures/conversation', ...)")`
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_pagination`

use serde_json::Value;
use victauri_test::e2e_test;

const PAGINATE_CONV_ID: &str = "e2e-paginate-conv";
const PAGINATE_TOTAL: usize = 100;
const PAGINATE_PAGE_LIMIT: usize = 80;

/// 通过 webview fetch() 调用后端 API 并返回 JSON。
async fn api_fetch(client: &mut victauri_test::VictauriClient, js_fetch: &str) -> Value {
    let result = client.eval_js(js_fetch).await.unwrap();
    if let Some(s) = result.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        result
    }
}

/// 获取分页消息。
async fn get_messages(
    client: &mut victauri_test::VictauriClient,
    conv_id: &str,
    query: &str,
) -> Value {
    let q = if query.is_empty() {
        String::new()
    } else {
        format!("&{query}")
    };
    api_fetch(
        client,
        &format!(
            "fetch('/conversation/messages?conversation_id={conv_id}{q}').then(r=>r.json()).then(d=>JSON.stringify(d))"
        ),
    )
    .await
}

/// 播种 100 条分页对话。
async fn seed_paginated(client: &mut victauri_test::VictauriClient) {
    // 重置工作区
    let _ = client
        .eval_js(
            "fetch('/workspace',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:null})})"
        )
        .await;

    // 构造 100 条 user 消息
    let msgs: Vec<String> = (0..PAGINATE_TOTAL)
        .map(|i| format!(r#"{{"role":"user","content":"e2e-msg-{i}"}}"#))
        .collect();
    let body = format!(
        r#"{{"conversation_id":"{PAGINATE_CONV_ID}","messages":[{}],"replace":true}}"#,
        msgs.join(",")
    );

    let _ = client
        .eval_js(&format!(
            "fetch('/e2e/fixtures/conversation',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({body})}})"
        ))
        .await;
}

// ---------------------------------------------------------------------------
// 测试 1：全量窗口（无 limit 参数）
// ---------------------------------------------------------------------------
e2e_test!(full_window_when_limit_omitted, |client| async move {
    seed_paginated(&mut client).await;

    let body = get_messages(&mut client, PAGINATE_CONV_ID, "").await;
    assert_eq!(body["conversation_id"].as_str().unwrap_or(""), PAGINATE_CONV_ID);
    assert_eq!(body["total_count"].as_u64().unwrap_or(0) as usize, PAGINATE_TOTAL);
    assert_eq!(body["window_start_index"].as_u64().unwrap_or(999) as usize, 0);
    assert_eq!(body["has_older"].as_bool().unwrap_or(true), false);
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), PAGINATE_TOTAL);
    assert_eq!(msgs[0]["content"].as_str().unwrap_or(""), "e2e-msg-0");
    assert_eq!(
        msgs[PAGINATE_TOTAL - 1]["content"].as_str().unwrap_or(""),
        format!("e2e-msg-{}", PAGINATE_TOTAL - 1)
    );
});

// ---------------------------------------------------------------------------
// 测试 2：尾页 limit=80
// ---------------------------------------------------------------------------
e2e_test!(tail_page_limit_80, |client| async move {
    seed_paginated(&mut client).await;

    let body = get_messages(&mut client, PAGINATE_CONV_ID, &format!("limit={PAGINATE_PAGE_LIMIT}")).await;
    assert_eq!(body["total_count"].as_u64().unwrap_or(0) as usize, PAGINATE_TOTAL);
    assert_eq!(
        body["window_start_index"].as_u64().unwrap_or(0) as usize,
        PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT
    );
    assert_eq!(body["has_older"].as_bool().unwrap_or(false), true);
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), PAGINATE_PAGE_LIMIT);
    assert_eq!(
        msgs[0]["content"].as_str().unwrap_or(""),
        format!("e2e-msg-{}", PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT)
    );
});

// ---------------------------------------------------------------------------
// 测试 3：更早分页 before_index=window_start
// ---------------------------------------------------------------------------
e2e_test!(older_page_before_index, |client| async move {
    seed_paginated(&mut client).await;

    let tail = get_messages(&mut client, PAGINATE_CONV_ID, &format!("limit={PAGINATE_PAGE_LIMIT}")).await;
    let win_start = tail["window_start_index"].as_u64().unwrap_or(0);

    let body = get_messages(
        &mut client,
        PAGINATE_CONV_ID,
        &format!("limit={PAGINATE_PAGE_LIMIT}&before_index={win_start}"),
    ).await;
    assert_eq!(body["total_count"].as_u64().unwrap_or(0) as usize, PAGINATE_TOTAL);
    assert_eq!(body["window_start_index"].as_u64().unwrap_or(999) as usize, 0);
    assert_eq!(body["has_older"].as_bool().unwrap_or(true), false);
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), PAGINATE_TOTAL - PAGINATE_PAGE_LIMIT);
    assert_eq!(msgs[0]["content"].as_str().unwrap_or(""), "e2e-msg-0");
});

// ---------------------------------------------------------------------------
// 测试 4：未知 conversation 返回 404
// ---------------------------------------------------------------------------
e2e_test!(unknown_conversation_returns_404, |client| async move {
    let status: f64 = client
        .eval_js(
            "fetch('/conversation/messages?conversation_id=e2e-no-such-conv').then(r=>r.status)"
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert_eq!(status as u16, 404, "expected 404, got {status}");
});

// ---------------------------------------------------------------------------
// 测试 5：水合尾页显示最新消息和 load-older 控件
// ---------------------------------------------------------------------------
e2e_test!(hydrate_tail_page_shows_latest_and_load_older, |client| async move {
    seed_paginated(&mut client).await;

    // 绑定会话到分页对话
    let _ = client
        .eval_js(
            "fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})"
        )
        .await;

    let _ = client
        .eval_js(&format!(
            "fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'s_e2e_hydrate',title:'E2E hydrate',draft:'',messages:[],updated_at:1,pinned:false,starred:false,server_conversation_id:'{PAGINATE_CONV_ID}',server_revision:1}}],active_session_id:'s_e2e_hydrate'}})}})"
        ))
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(15000), Some(500))
        .await
        .ok();

    // 尾页最新消息应可见
    client
        .wait_for(
            "text",
            Some(&format!("e2e-msg-{}", PAGINATE_TOTAL - 1)),
            Some(15000),
            Some(200),
        )
        .await
        .unwrap();

    // load-older 按钮应可见
    let load_older_visible: bool = client
        .eval_js(
            "(()=>{const el=document.querySelector('[data-testid=\"chat-load-older\"]');return el!==null&&el.offsetParent!==null;})()"
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(load_older_visible, "chat-load-older should be visible");

    // 首条消息不应可见
    let has_msg_0: bool = client
        .eval_js("document.body.innerText.includes('e2e-msg-0')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!has_msg_0, "e2e-msg-0 should not be visible in tail page");
});
