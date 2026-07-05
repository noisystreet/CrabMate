//! Victauri 版 Phase 5 可见消息筛选 E2E 测试（Phase 2：需播种，无流存根）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/phase5-visible-messages.spec.ts` — 可见消息筛选规则验证
//!
//! 注意：导出验证（JSON/Markdown download）依赖 Playwright 的 `page.waitForEvent('download')`，
//! 暂未迁移，属于 Phase 3。当前覆盖 UI 聊天列可见性断言。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_visible_messages`

use victauri_test::e2e_test;

/// 播种指定消息列表到会话中，然后刷新页面。
/// 消息 JSON 格式：`[{id,role,text,reasoning_text?,state?,is_tool?}, ...]`
async fn seed_messages_and_goto(
    client: &mut victauri_test::VictauriClient,
    session_id: &str,
    messages_json: &str,
) {
    let _ = client
        .eval_js(&format!(
            "fetch('/user-data/prefs',{{
                method:'PUT',
                headers:{{'Content-Type':'application/json'}},
                body:JSON.stringify({{locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false}})
            }})"
        ))
        .await;

    let _ = client
        .eval_js(&format!(
            "fetch('/user-data/workspaces/current/sessions',{{
                method:'PUT',
                headers:{{'Content-Type':'application/json'}},
                body:JSON.stringify({{
                    sessions:[{{id:'{session_id}',title:'E2E Phase5',draft:'',messages:{messages_json},updated_at:1,pinned:false,starred:false}}],
                    active_session_id:'{session_id}'
                }})
            }})"
        ))
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(15000), Some(500))
        .await
        .ok();
}

/// 获取聊天层中可见的 assistant 气泡数量。
async fn visible_assistant_count(client: &mut victauri_test::VictauriClient) -> usize {
    client
        .eval_js(
            "(()=>{const rows=document.querySelectorAll('.msg-stack');let c=0;for(const r of rows){{const role=r.querySelector('.msg-meta-role');if(role&&/^助手$|^Assistant$/.test(role.textContent||'')){{if(r.offsetParent!==null)c++;}}}}return c;})()"
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0) as usize
}

// ---------------------------------------------------------------------------
// 测试 1：重复 assistant 行——读路径不去重，两条都显示
// ---------------------------------------------------------------------------
e2e_test!(stored_duplicate_assistant_rows_both_visible, |client| async move {
    seed_messages_and_goto(
        &mut client,
        "s_e2e_phase5_fuzzy",
        "[{\"id\":\"u1\",\"role\":\"user\",\"text\":\"分析当前目录\"},{\"id\":\"a1\",\"role\":\"assistant\",\"text\":\"当前目录下有三个压缩包:\\n\\n1.**A\"},{\"id\":\"a2\",\"role\":\"assistant\",\"text\":\"当前目录下有三个压缩包:\\n1.**A\"}]"
    )
    .await;

    client
        .wait_for("text", Some("分析当前目录"), Some(10000), Some(200))
        .await
        .unwrap();

    let count = visible_assistant_count(&mut client).await;
    assert_eq!(count, 2, "expected 2 visible assistant rows, got {count}");
});

// ---------------------------------------------------------------------------
// 测试 2：final_response_snapshot 重复——聊天列只显示首条
// ---------------------------------------------------------------------------
e2e_test!(duplicate_final_response_snapshot_hidden_in_chat, |client| async move {
    seed_messages_and_goto(
        &mut client,
        "s_e2e_phase5_snap",
        r###"[{"id":"u1","role":"user","text":"分析当前目录"},{"id":"a1","role":"assistant","text":"当前目录下有三个压缩包。"},{"id":"snap","role":"assistant","text":"当前目录下有三个压缩包。","state":"{\"k\":\"cm_tl\",\"t\":\"final_response_snapshot\"}"}]"###
    )
    .await;

    client
        .wait_for("text", Some("分析当前目录"), Some(10000), Some(200))
        .await
        .unwrap();

    let count = visible_assistant_count(&mut client).await;
    assert_eq!(count, 1, "snapshot should be hidden, but got {count} rows");
});

// ---------------------------------------------------------------------------
// 测试 3：编排路由 + commentary 被隐藏，只显示终答
// ---------------------------------------------------------------------------
e2e_test!(orchestration_route_and_commentary_hidden_only_final_visible, |client| async move {
    seed_messages_and_goto(
        &mut client,
        "s_e2e_phase5_ephemeral",
        r###"[{"id":"u1","role":"user","text":"分析当前目录"},{"id":"route","role":"assistant","text":"CrabMate staged_timeline\n编排路由：freeform\n{}"},{"id":"cmt","role":"assistant","text":"","reasoning_text":"目录结构分析","state":"commentary_before_tools"},{"id":"a1","role":"assistant","text":"当前目录下有三个压缩包。"}]"###
    )
    .await;

    client
        .wait_for("text", Some("分析当前目录"), Some(10000), Some(200))
        .await
        .unwrap();

    // 终答应可见
    client
        .expect_text_with_timeout("当前目录下有三个压缩包。", 10000)
        .await
        .unwrap();

    // 编排路由不应可见
    let route_visible: bool = client
        .eval_js("document.body.innerText.includes('编排路由')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!route_visible, "orchestration route should be hidden");

    // commentary 不应可见
    let cmt_visible: bool = client
        .eval_js("document.body.innerText.includes('目录结构分析')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!cmt_visible, "commentary before tools should be hidden");

    // 只有 1 个可见 assistant 气泡
    let count = visible_assistant_count(&mut client).await;
    assert_eq!(count, 1, "expected 1 visible assistant row, got {count}");
});
