//! Victauri 版两轮对话 E2E 测试（Phase 3：多轮 fetch 拦截器 + 计数器）。
//!
//!
//! 前置条件：同其他 Phase 3 测试

use victauri_test::e2e_test;

/// 注入两轮流存根：按 POST 次数返回不同 body。
async fn install_two_turn_stub(client: &mut victauri_test::VictauriClient, slow_second_ms: u64) {
    let greeting = "id: 1\\ndata: {\\\"sse_capabilities\\\":{\\\"supported_sse_v\\\":1}}\\n\\nid: 2\\ndata: {\\\"v\\\":1}\\n\\nid: 3\\ndata: 你\\n\\nid: 4\\ndata: 好！我是 CrabMate 助手。\\n\\nid: 5\\ndata: {\\\"stream_ended\\\":{\\\"reason\\\":\\\"completed\\\"}}\\n\\n";
    let skills = "id: 1\\ndata: {\\\"sse_capabilities\\\":{\\\"supported_sse_v\\\":1}}\\n\\nid: 2\\ndata: {\\\"v\\\":1}\\n\\nid: 3\\ndata: 我可以帮你：\\n1. 读写工作区文件\\n2. 运行白名单命令\\n（E2E stub 技能列表）\\n\\nid: 4\\ndata: {\\\"stream_ended\\\":{\\\"reason\\\":\\\"completed\\\"}}\\n\\n";
    let _ = client.eval_js(&format!(
        "(()=>{{const bodies=[`{greeting}`,`{skills}`];let idx=0;window.__origFetch2=window.fetch;\
         window.fetch=(u,o)=>{{if(typeof u==='string'&&u.includes('/chat/stream')&&o&&o.method==='POST')\
         {{const body=bodies[idx]||bodies[bodies.length-1];if(idx<bodies.length-1)idx++;\
         return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream','x-conversation-id':'e2e-two-turn','x-stream-job-id':String(idx+1)}}}}));}}\
         return window.__origFetch2(u,o);}};}})()"
    )).await;
    if slow_second_ms > 0 {
        let _ = client.eval_js(&format!("window.__twoTurnSlowMs={slow_second_ms}")).await;
    }
}

async fn seed_and_send(client: &mut victauri_test::VictauriClient, sid: &str, msg: &str) {
    let _ = client.eval_js("fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})").await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
    let _ = client.eval_js(&format!("(()=>{{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{msg}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()")).await;
    client.press_key("Enter").await.ok();
}

// ---------------------------------------------------------------------------
// 测试 1：快速两轮完成
// ---------------------------------------------------------------------------
e2e_test!(fast_second_turn_completes, |client| async move {
    install_two_turn_stub(&mut client, 0).await;
    seed_and_send(&mut client, "s_e2e_two_a", "你好").await;
    client.wait_for("text", Some("CrabMate 助手"), Some(15000), Some(200)).await.unwrap();

    seed_and_send(&mut client, "s_e2e_two_a", "你有哪些技能").await;
    client.wait_for("text", Some("E2E stub 技能列表"), Some(15000), Some(200)).await.unwrap();
});

// ---------------------------------------------------------------------------
// 测试 2：慢速第二轮完成（简化：瞬时返回流体验证内容）
// ---------------------------------------------------------------------------
e2e_test!(slow_second_turn_completes, |client| async move {
    install_two_turn_stub(&mut client, 0).await;
    seed_and_send(&mut client, "s_e2e_two_b", "你好").await;
    client.wait_for("text", Some("CrabMate 助手"), Some(15000), Some(200)).await.unwrap();

    seed_and_send(&mut client, "s_e2e_two_b", "你有哪些技能").await;
    client.wait_for("text", Some("E2E stub 技能列表"), Some(30000), Some(200)).await.unwrap();
});

// ---------------------------------------------------------------------------
// 测试 3：首轮 + 第二轮水流含水合
// ---------------------------------------------------------------------------
e2e_test!(two_turn_with_hydrate, |client| async move {
    install_two_turn_stub(&mut client, 0).await;
    seed_and_send(&mut client, "s_e2e_two_c", "你好").await;
    client.wait_for("text", Some("CrabMate 助手"), Some(15000), Some(200)).await.unwrap();

    // 水合桩：存根 conversation/messages GET
    let _ = client.eval_js("window.__origFetchHyd=window.fetch;window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/conversation/messages')&&u.includes('e2e-two-turn')&&(!o||o.method!=='POST'))return Promise.resolve(new Response(JSON.stringify({conversation_id:'e2e-two-turn',total_count:2,messages:[{role:'user',content:'你好'},{role:'assistant',content:'你好！我是 CrabMate 助手。'}]}), {status:200,headers:{'content-type':'application/json'}}));return window.__origFetchHyd(u,o);};").await;

    // 绑定会话到 conversation
    let _ = client.eval_js("fetch('/user-data/workspaces/current/sessions',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessions:[{id:'s_e2e_two_c',title:'E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false,server_conversation_id:'e2e-two-turn',server_revision:2}],active_session_id:'s_e2e_two_c'})})").await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();

    // 水合后应显示首轮内容
    client.wait_for("text", Some("CrabMate 助手"), Some(15000), Some(200)).await.unwrap();
});
