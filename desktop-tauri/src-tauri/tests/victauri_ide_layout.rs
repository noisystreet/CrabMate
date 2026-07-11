//! Victauri 版 IDE 布局 E2E 测试（Phase 3：fetch 拦截器存根 workspace/file 路由）。
//!
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_ide_layout`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

const STUB_FILE: &str = "e2e-ide-stub.txt";

async fn stub_ide_routes(client: &mut victauri_test::VictauriClient) {
    let _ = client.eval_js(&format!(
        "(()=>{{window.__origFetchIde=window.fetch;\
         window.__ideFileContent='hello ide';\
         window.fetch=(u,o)=>{{if(typeof u!=='string')return window.__origFetchIde(u,o);\
         if(u.includes('/workspace/file')&&o&&o.method==='GET')\
         return Promise.resolve(new Response(JSON.stringify({{path:'{STUB_FILE}',content:window.__ideFileContent,error:null}}),{{status:200,headers:{{'content-type':'application/json'}}}}));\
         if(u.includes('/workspace/file')&&o&&o.method==='POST'){{try{{const body=JSON.parse(o.body);if(body.content)window.__ideFileContent=body.content;}}catch(e){{}}return Promise.resolve(new Response(JSON.stringify({{error:null}}),{{status:200,headers:{{'content-type':'application/json'}}}}));}}\
         if(u.includes('/workspace')&&o&&o.method==='POST')\
         return Promise.resolve(new Response(JSON.stringify({{ok:true,path:'/e2e-mock-root'}}),{{status:200,headers:{{'content-type':'application/json'}}}}));\
         if(u.includes('/workspace'))\
         return Promise.resolve(new Response(JSON.stringify({{path:'/e2e-mock-root',entries:[{{name:'{STUB_FILE}',is_dir:false}}],error:null}}),{{status:200,headers:{{'content-type':'application/json'}}}}));\
         return window.__origFetchIde(u,o);}};}})()"
    )).await;
}

async fn seed_and_goto(client: &mut victauri_test::VictauriClient, sid: &str) {
    let _ = client.eval_js("fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})").await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E IDE',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
}

// ---------------------------------------------------------------------------
// 测试 1：对话模式 → 切换到 IDE → 打开文件显示内容
// ---------------------------------------------------------------------------
e2e_test!(chat_to_ide_shows_editor_content, |client| async move {
    stub_ide_routes(&mut client).await;
    seed_and_goto(&mut client, "s_e2e_ide").await;

    // 确认对话模式
    client.wait_for("text", Some("CrabMate"), Some(10000), Some(200)).await.unwrap();
    let ide_hidden: bool = client.eval_js("document.querySelector('[data-testid=\"ide-layout-root\"]')?.offsetParent===null??true").await.unwrap().as_bool().unwrap_or(true);
    assert!(ide_hidden, "IDE should be hidden in chat mode");

    // 切换到 IDE 布局
    Locator::test_id("layout-mode-toggle").first().click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_visible().await.unwrap();

    // 等待文件树出现
    client.wait_for("text", Some(STUB_FILE), Some(15000), Some(200)).await.unwrap();

    // 点击文件
    let _ = client.eval_js(&format!("(()=>{{const els=document.querySelectorAll('[data-testid=\"workspace-file-tree\"]');if(els.length>0){{const tree=els[0];const items=tree.querySelectorAll('*');for(const item of items){{if(item.textContent?.includes('{STUB_FILE}')){{item.click();break;}}}}}}}})()")).await;

    // 等待 CodeMirror 出现内容
    let cm_visible: bool = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.offsetParent!==null??false").await.unwrap().as_bool().unwrap_or(false);
    assert!(cm_visible, "CodeMirror editor should be visible");
    let content: String = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.textContent??''").await.unwrap().as_str().unwrap_or("").to_string();
    assert!(content.contains("hello ide"), "editor should show 'hello ide', got '{content}'");
});

// ---------------------------------------------------------------------------
// 测试 2：打开文件 → 编辑 → Ctrl+S 保存 → 切回对话
// ---------------------------------------------------------------------------
e2e_test!(edit_save_return_to_chat, |client| async move {
    stub_ide_routes(&mut client).await;
    seed_and_goto(&mut client, "s_e2e_ide2").await;

    Locator::test_id("layout-mode-toggle").first().click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_visible().await.unwrap();
    client.wait_for("text", Some(STUB_FILE), Some(15000), Some(200)).await.unwrap();

    // 点击文件 + 等待 CM 出现
    let _ = client.eval_js(&format!("(()=>{{const tree=document.querySelector('[data-testid=\"workspace-file-tree\"]');if(!tree)return;const items=tree.querySelectorAll('*');for(const item of items){{if(item.textContent?.includes('{STUB_FILE}')){{item.click();break;}}}}}})()")).await;
    client.wait_for("selector", Some("[data-testid=\"ide-editor-cm\"] .cm-content"), Some(10000), Some(200)).await.unwrap();

    // 编辑：Ctrl+A → 输入新内容 → Ctrl+S
    let _ = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.focus()").await;
    client.press_key("Control+a").await.unwrap();
    let _ = client.eval_js("(()=>{const cm=document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content');if(!cm)return;cm.focus();document.execCommand('insertText',false,'hello ide e2e');})()").await;
    client.press_key("Control+s").await.unwrap();

    // 等保存生效
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 切回对话模式
    Locator::test_id("layout-mode-toggle").click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_hidden().await.unwrap();
});

// ---------------------------------------------------------------------------
// 测试 3：对话/IDE 来回切换保留 undo stack
// ---------------------------------------------------------------------------
e2e_test!(chat_roundtrip_preserves_undo_stack, |client| async move {
    stub_ide_routes(&mut client).await;
    seed_and_goto(&mut client, "s_e2e_ide3").await;

    Locator::test_id("layout-mode-toggle").first().click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_visible().await.unwrap();
    client.wait_for("text", Some(STUB_FILE), Some(15000), Some(200)).await.unwrap();

    let _ = client.eval_js(&format!("document.querySelectorAll('[data-testid=\"workspace-file-tree\"]')[0]?.querySelectorAll('*')?.forEach(item=>{{if(item.textContent?.includes('{STUB_FILE}'))item.click()}})")).await;
    client.wait_for("selector", Some("[data-testid=\"ide-editor-cm\"] .cm-content"), Some(10000), Some(200)).await.unwrap();

    // 编辑内容
    let _ = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.focus()").await;
    client.press_key("Control+a").await.unwrap();
    let _ = client.eval_js("document.execCommand('insertText',false,'alpha')").await;

    // 切回对话 → 切回 IDE
    Locator::test_id("layout-mode-toggle").click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_hidden().await.unwrap();
    Locator::test_id("layout-mode-toggle").click(&mut client).await.unwrap();
    Locator::test_id("ide-layout-root").expect(&mut client).to_be_visible().await.unwrap();

    // 确认内容保留
    let content: String = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.textContent??''").await.unwrap().as_str().unwrap_or("").to_string();
    assert!(content.contains("alpha"), "content should be 'alpha', got '{content}'");

    // Ctrl+Z undo
    let _ = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.focus()").await;
    client.press_key("Control+z").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let content2: String = client.eval_js("document.querySelector('[data-testid=\"ide-editor-cm\"] .cm-content')?.textContent??''").await.unwrap().as_str().unwrap_or("").to_string();
    assert!(content2.contains("hello ide") && !content2.contains("alpha"), "undo should restore 'hello ide', got '{content2}'");
});
