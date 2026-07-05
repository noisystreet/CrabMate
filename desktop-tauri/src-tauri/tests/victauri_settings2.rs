//! Victauri 版 Phase 3 设置页 LLM/MCP 追加测试（fetch 拦截器存根）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/settings-llm.spec.ts`  — LLM model/API key 保存
//!   - `e2e/tests/settings-mcp.spec.ts`  — MCP save/import (剩余 2 tests)

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

async fn seed_settings_session(client: &mut victauri_test::VictauriClient, sid: &str) {
    let _ = client.eval_js(&format!("fetch('/user-data/prefs',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false}})}})")).await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
}

async fn open_settings(client: &mut victauri_test::VictauriClient, section: &str) {
    Locator::test_id("settings-open").click(client).await.unwrap();
    client.wait_for("selector", Some("[data-testid=\"settings-page\"].settings-page-visible"), Some(10000), Some(500)).await.unwrap();
    Locator::test_id(&format!("settings-nav-{section}")).click(client).await.unwrap();
}

async fn close_settings(client: &mut victauri_test::VictauriClient) {
    Locator::test_id("settings-back").click(client).await.unwrap();
    client.wait_for("selector_gone", Some("[data-testid=\"settings-page\"].settings-page-visible"), Some(10000), Some(200)).await.unwrap();
}

e2e_test!(model_and_api_key_save, |client| async move {
    seed_settings_session(&mut client, "s_e2e_llm").await;
    let _ = client.eval_js("(()=>{window.__e2eSecretPut=null;window.__origFetch4=window.fetch;window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/user-data/secrets/client-llm')&&o&&o.method==='PUT'){try{window.__e2eSecretPut=JSON.parse(o.body);}catch(e){}return Promise.resolve(new Response('',{status:204}));}return window.__origFetch4(u,o);};})()").await;
    open_settings(&mut client, "llm").await;
    let _ = client.eval_js("(()=>{const el=document.querySelector('[data-testid=\"settings-llm-model\"]');if(el){const s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;s.call(el,'e2e-test-model');el.dispatchEvent(new Event('input',{bubbles:true}));}})()").await;
    let _ = client.eval_js("(()=>{const el=document.querySelector('[data-testid=\"settings-client-api-key\"]');if(el){const s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;s.call(el,'E2E_STUB_KEY');el.dispatchEvent(new Event('input',{bubbles:true}));}})()").await;
    Locator::test_id("settings-save-all").click(&mut client).await.unwrap();
    client.wait_for("text", Some("已保存"), Some(10000), Some(200)).await.ok();
    close_settings(&mut client).await;
    let secret_ok: bool = client.eval_js("window.__e2eSecretPut?.api_key==='E2E_STUB_KEY'").await.unwrap().as_bool().unwrap_or(false);
    assert!(secret_ok);
    open_settings(&mut client, "llm").await;
    let model_val: String = client.eval_js("document.querySelector('[data-testid=\"settings-llm-model\"]')?.value??''").await.unwrap().as_str().unwrap_or("").to_string();
    assert_eq!(model_val, "e2e-test-model");
    let key_val: String = client.eval_js("document.querySelector('[data-testid=\"settings-client-api-key\"]')?.value??''").await.unwrap().as_str().unwrap_or("").to_string();
    assert_eq!(key_val, "");
});

e2e_test!(import_mcp_json_adds_server_rows, |client| async move {
    seed_settings_session(&mut client, "s_e2e_mcp_import").await;
    open_settings(&mut client, "mcp").await;
    let mcp_json = "{\"mcpServers\":{\"e2e-import\":{\"command\":\"npx\",\"args\":[\"-y\",\"echo\",\"mcp-e2e\"]}}}";
    let _ = client.eval_js(&format!("(()=>{{const el=document.querySelector('[data-testid=\"settings-mcp-import-json\"]');if(!el)return;const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{mcp_json}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()")).await;
    Locator::test_id("settings-mcp-import-apply").click(&mut client).await.unwrap();
    client.wait_for("text", Some("已导入"), Some(10000), Some(200)).await.unwrap();
    let _ = client.eval_js("window.__e2eMcpSaved=false;window.__origFetch5=window.fetch;window.fetch=(u,o)=>{if(typeof u==='string'&&u.includes('/user-data/mcp-servers')&&o&&o.method==='PUT'){window.__e2eMcpSaved=true;return Promise.resolve(new Response('',{status:204}));}return window.__origFetch5(u,o);};").await;
    Locator::test_id("settings-mcp-save").click(&mut client).await.unwrap();
    client.wait_for("text", Some("已保存"), Some(10000), Some(200)).await.ok();
    let saved: bool = client.eval_js("window.__e2eMcpSaved===true").await.unwrap().as_bool().unwrap_or(false);
    assert!(saved);
});
