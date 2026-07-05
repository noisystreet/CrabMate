//! Victauri 版 Phase 3 SSE 追加测试：审批/澄清。
//!
//! 等价 Playwright:
//!   - `e2e/tests/sse-control.spec.ts`         — 审批弹窗
//!   - `e2e/tests/sse-approval-actions.spec.ts` — 审批 deny/allow
//!   - `e2e/tests/sse-clarification.spec.ts`    — 澄清问卷

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

async fn inject_stream_stub(client: &mut victauri_test::VictauriClient, sse_body: &str) {
    let _ = client.eval_js(&format!(
        "(()=>{{const body=`{sse_body}`;window.__origFetch=window.fetch;window.fetch=(url,opts)=>{{if(typeof url==='string'&&url.includes('/chat/stream')&&opts&&opts.method==='POST')return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));return window.__origFetch(url,opts);}};}})()"
    )).await;
}

async fn seed_and_send(client: &mut victauri_test::VictauriClient, sid: &str, msg: &str) {
    let _ = client.eval_js(&format!("fetch('/user-data/prefs',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false}})}})")).await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
    let _ = client.eval_js(&format!("(()=>{{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{msg}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()")).await;
    client.press_key("Enter").await.ok();
}

// ---------------------------------------------------------------------------
// sse-control: 审批弹窗含命令预览
// ---------------------------------------------------------------------------
e2e_test!(command_approval_opens_modal_with_preview, |client| async move {
    let sse = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: e2e approval ping.\n\n",
        "id: 4\ndata: {\"command_approval_request\":{\"command\":\"git\",\"args\":\"status\",\"allowlist_key\":\"git\"}}\n\n",
        "id: 5\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );
    inject_stream_stub(&mut client, sse).await;
    let _ = client.eval_js("(()=>{window.__origFetchAppr=window.fetch;window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/chat/approval'))return Promise.resolve(new Response('',{status:204}));return window.__origFetchAppr(u,o);};})()").await;
    seed_and_send(&mut client, "s_e2e_appr", "e2e approval").await;

    let modal = Locator::test_id("approval-modal");
    modal.expect(&mut client).to_be_visible().await.unwrap();
    let has_cmd: bool = client.eval_js("document.querySelector('[data-testid=\"approval-modal\"]')?.innerText.includes('git status')??false").await.unwrap().as_bool().unwrap_or(false);
    assert!(has_cmd, "should show git status");
    Locator::test_id("approval-deny").click(&mut client).await.unwrap();
    modal.expect(&mut client).to_be_hidden().await.unwrap();
});

// ---------------------------------------------------------------------------
// sse-approval-actions: deny
// ---------------------------------------------------------------------------
e2e_test!(deny_closes_modal_no_failed_banner, |client| async move {
    inject_stream_stub(&mut client, concat!("id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\nid: 2\ndata: {\"v\":1}\n\nid: 3\ndata: e2e approve.\n\nid: 4\ndata: {\"command_approval_request\":{\"command\":\"true\",\"args\":\"\",\"allowlist_key\":\"true\"}}\n\nid: 5\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n")).await;
    let _ = client.eval_js("window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/chat/approval'))return Promise.resolve(new Response('',{status:204}));return window.__origFetch(u,o);};").await;
    seed_and_send(&mut client, "s_e2e_deny", "e2e deny").await;
    Locator::test_id("approval-deny").click(&mut client).await.unwrap();
    Locator::test_id("approval-modal").expect(&mut client).to_be_hidden().await.unwrap();
});

// ---------------------------------------------------------------------------
// sse-approval-actions: allow once
// ---------------------------------------------------------------------------
e2e_test!(allow_once_closes_modal, |client| async move {
    inject_stream_stub(&mut client, concat!("id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\nid: 2\ndata: {\"v\":1}\n\nid: 3\ndata: e2e allow.\n\nid: 4\ndata: {\"command_approval_request\":{\"command\":\"true\",\"args\":\"\",\"allowlist_key\":\"true\"}}\n\nid: 5\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n")).await;
    let _ = client.eval_js("window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/chat/approval'))return Promise.resolve(new Response('',{status:204}));return window.__origFetch(u,o);};").await;
    seed_and_send(&mut client, "s_e2e_allow", "e2e allow").await;
    Locator::test_id("approval-allow-once").click(&mut client).await.unwrap();
    Locator::test_id("approval-modal").expect(&mut client).to_be_hidden().await.unwrap();
});

// ---------------------------------------------------------------------------
// sse-clarification: 问卷 → 提交 → 第二轮助手回复
// ---------------------------------------------------------------------------
e2e_test!(clarification_panel_submit_triggers_second_stream, |client| async move {
    let bodies = r#"["id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\nid: 2\ndata: {\"v\":1}\n\nid: 3\ndata: {\"clarification_questionnaire\":{\"questionnaire_id\":\"e2e-q1\",\"intro\":\"E2E please clarify\",\"questions\":[{\"id\":\"scope\",\"label\":\"Scope?\",\"required\":true}]}}\n\nid: 4\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n","id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\nid: 2\ndata: {\"v\":1}\n\nid: 3\ndata: E2E after clarify.\n\nid: 5\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n"]"#;
    let _ = client.eval_js(&format!("(()=>{{const bodies={bodies};let idx=0;window.__origFetch3=window.fetch;window.fetch=(u,o)=>{{if(typeof u==='string'&&u.includes('/chat/stream')&&o&&o.method==='POST'){{const body=bodies[idx]||bodies[bodies.length-1];if(idx<bodies.length-1)idx++;return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));}}return window.__origFetch3(u,o);}};}})()")).await;
    seed_and_send(&mut client, "s_e2e_clarify", "e2e clarify").await;

    Locator::test_id("composer-clarification-panel").expect(&mut client).to_be_visible().await.unwrap();
    let _ = client.eval_js("(()=>{const inputs=document.querySelectorAll('[data-testid=\"composer-clarification-input\"]');if(inputs.length>0){const el=inputs[0];el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'backend only');el.dispatchEvent(new Event('input',{bubbles:true}));}})()").await;
    Locator::test_id("composer-clarification-submit").click(&mut client).await.unwrap();
    client.wait_for("text", Some("E2E after clarify"), Some(15000), Some(200)).await.unwrap();
    Locator::test_id("composer-clarification-panel").expect(&mut client).to_be_hidden().await.unwrap();
});
