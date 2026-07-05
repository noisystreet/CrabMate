//! Victauri 版 Turn 布局交错 E2E 测试（Phase 3：工具调用 + commentary 交错 SSE）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/sse-turn-layout-interleaved.spec.ts`
//!
//! 前置条件：同其他 Phase 3 测试

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

async fn seed_and_send(client: &mut victauri_test::VictauriClient, sid: &str, msg: &str) {
    let _ = client.eval_js(&format!("fetch('/user-data/prefs',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false}})}})")).await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
    let _ = client.eval_js(&format!("(()=>{{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{msg}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()")).await;
    client.press_key("Enter").await.ok();
}

// ---------------------------------------------------------------------------
// 测试：多工具调用 + commentary 交错 → UI 显示工具卡 + post-tool 块
// ---------------------------------------------------------------------------
e2e_test!(multi_tool_interleaved_layout, |client| async move {
    let sse = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: 好的，先解压。\n\n",
        "id: 4\ndata: {\"tool_call\":{\"tool_call_id\":\"t1\",\"name\":\"archive_list\",\"summary\":\"列出归档\"}}\n\n",
        "id: 5\ndata: {\"tool_result\":{\"name\":\"archive_list\",\"result_version\":1,\"summary\":\"ok\",\"output\":\"hpcg.tar.gz\",\"ok\":true}}\n\n",
        "id: 6\ndata: 读取 INSTALL。\n\n",
        "id: 7\ndata: {\"tool_call\":{\"tool_call_id\":\"t2\",\"name\":\"read_file\",\"summary\":\"读取文件\"}}\n\n",
        "id: 8\ndata: {\"tool_result\":{\"name\":\"read_file\",\"result_version\":1,\"summary\":\"ok\",\"output\":\"cmake_minimum_required\",\"ok\":true}}\n\n",
        "id: 9\ndata: 开始编译。\n\n",
        "id: 10\ndata: {\"tool_call\":{\"tool_call_id\":\"t3\",\"name\":\"run_command\",\"summary\":\"编译命令\"}}\n\n",
        "id: 11\ndata: {\"tool_result\":{\"name\":\"run_command\",\"result_version\":1,\"summary\":\"ok\",\"output\":\"Build succeeded\",\"ok\":true}}\n\n",
        "id: 12\ndata: 编译流程结束。\n\n",
        "id: 13\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );
    let _ = client.eval_js(&format!(
        "(()=>{{const body=`{sse}`;window.__origFetchTL=window.fetch;window.fetch=(u,o)=>{{if(typeof u==='string'&&u.includes('/chat/stream')&&o&&o.method==='POST')return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));return window.__origFetchTL(u,o);}};}})()"
    )).await;

    seed_and_send(&mut client, "s_e2e_turn", "编译 hpcg").await;

    // 终答应可见
    client.wait_for("text", Some("编译流程结束"), Some(20000), Some(200)).await.unwrap();

    // 工具卡应出现
    Locator::test_id("chat-tool-card").expect(&mut client).to_be_visible().await.unwrap();
});
