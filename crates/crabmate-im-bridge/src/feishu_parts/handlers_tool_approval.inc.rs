// 工具审批：`resolve_one_tool_approval` 与 `wait_http` / `wait_message` 共享路径（由 `feishu.rs` `include!`）。

#[derive(Clone, Copy)]
enum ToolApprovalInteractiveWaitKind {
    Http,
    Message,
}

fn wait_mode_fallback_body(
    kind: ToolApprovalInteractiveWaitKind,
    notice: &crate::sse_consumer::CommandApprovalNotice,
    approval_session_id: &str,
) -> String {
    match kind {
        ToolApprovalInteractiveWaitKind::Http => format!(
            "⏸ 需人工审批工具：{}\n参数摘要：{}\n请在桥接服务 POST /feishu/tool-decision（Bearer 或 X-API-Key 为 FEISHU_TOOL_DECISION_SECRET），body: {{\"approval_session_id\":\"{}\",\"decision\":\"deny|allow_once|allow_always\"}}",
            notice.command,
            clip_one_line(&notice.args, 400),
            approval_session_id
        ),
        ToolApprovalInteractiveWaitKind::Message => format!(
            "⏸ 需你确认是否执行：{}\n参数摘要：{}\n请发送：!允许一次 、 !永久允许 、 !拒绝",
            notice.command,
            clip_one_line(&notice.args, 400),
        ),
    }
}

async fn deny_tool_on_wait_channel_closed(
    st: &FeishuBridgeState,
    kind: ToolApprovalInteractiveWaitKind,
    approval_session_id: &str,
) {
    if matches!(kind, ToolApprovalInteractiveWaitKind::Http) {
        warn!("tool decision channel closed");
        let _ = st
            .cfg
            .crabmate
            .send_chat_approval(approval_session_id, "deny")
            .await;
    }
}

async fn deny_tool_on_wait_timeout(
    st: &FeishuBridgeState,
    kind: ToolApprovalInteractiveWaitKind,
    approval_session_id: &str,
) {
    match kind {
        ToolApprovalInteractiveWaitKind::Http => {
            warn!("tool decision wait timeout; deny");
        }
        ToolApprovalInteractiveWaitKind::Message => {
            let _ = st
                .cfg
                .crabmate
                .send_chat_approval(approval_session_id, "deny")
                .await;
        }
    }
}

async fn await_tool_decision_or_deny(
    st: &FeishuBridgeState,
    rx: oneshot::Receiver<String>,
    timeout_secs: u64,
    kind: ToolApprovalInteractiveWaitKind,
    approval_session_id: &str,
) -> String {
    match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(d)) => d,
        Ok(Err(_)) => {
            deny_tool_on_wait_channel_closed(st, kind, approval_session_id).await;
            "deny".to_string()
        }
        Err(_) => {
            deny_tool_on_wait_timeout(st, kind, approval_session_id).await;
            "deny".to_string()
        }
    }
}

fn send_chat_approval_after_wait_warn_msg(kind: ToolApprovalInteractiveWaitKind) -> &'static str {
    match kind {
        ToolApprovalInteractiveWaitKind::Http => "send_chat_approval after wait failed",
        ToolApprovalInteractiveWaitKind::Message => "send_chat_approval after message wait failed",
    }
}

async fn resolve_wait_mode_tool_approval(
    st: &FeishuBridgeState,
    message_id: &str,
    chat_id: &str,
    approval_session_id: &str,
    notice: &crate::sse_consumer::CommandApprovalNotice,
    follow_idx: &mut u64,
    kind: ToolApprovalInteractiveWaitKind,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (tx, rx) = oneshot::channel();
    st.pending_tool_decisions.insert(
        approval_session_id.to_string(),
        PendingToolDecision { reply_tx: tx },
    );
    st.pending_tool_session_by_chat
        .insert(chat_id.to_string(), approval_session_id.to_string());
    *follow_idx = follow_idx.saturating_add(1);
    let uuid = format!("{message_id}-ap-{follow_idx}");
    let fallback_body = wait_mode_fallback_body(kind, notice, approval_session_id);
    if let Err(e) = reply_tool_approval_interactive_card(
        st,
        message_id,
        &uuid,
        notice,
        approval_session_id,
    )
    .await
    {
        warn!(?e, "feishu interactive approval card failed; fallback text");
        reply_followup_text_message(st, message_id, *follow_idx, &fallback_body).await?;
    }

    let timeout_secs = st.cfg.tool_decision_timeout_secs.max(5);
    let decision = await_tool_decision_or_deny(
        st,
        rx,
        timeout_secs,
        kind,
        approval_session_id,
    )
    .await;
    st.pending_tool_decisions.remove(approval_session_id);
    st.pending_tool_session_by_chat.remove(chat_id);
    if let Err(e) = st
        .cfg
        .crabmate
        .send_chat_approval(approval_session_id, decision.trim())
        .await
    {
        warn!(?e, "{}", send_chat_approval_after_wait_warn_msg(kind));
    }
    Ok(())
}

async fn resolve_one_tool_approval(
    st: &FeishuBridgeState,
    message_id: &str,
    chat_id: &str,
    approval_session_id: &str,
    notice: &crate::sse_consumer::CommandApprovalNotice,
    follow_idx: &mut u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match st.cfg.tool_approval_mode {
        FeishuToolApprovalMode::DenyAll => {
            let _ = st
                .cfg
                .crabmate
                .send_chat_approval(approval_session_id, "deny")
                .await;
        }
        FeishuToolApprovalMode::DefaultAllowOnce => {
            if let Err(e) = st
                .cfg
                .crabmate
                .send_chat_approval(approval_session_id, "allow_once")
                .await
            {
                warn!(?e, "send_chat_approval allow_once failed");
            }
            *follow_idx = follow_idx.saturating_add(1);
            reply_followup_text_message(
                st,
                message_id,
                *follow_idx,
                &format!(
                    "✅ 已自动允许一次：{} {}",
                    notice.command,
                    clip_one_line(&notice.args, 200)
                ),
            )
            .await?;
        }
        FeishuToolApprovalMode::WaitHttp => {
            resolve_wait_mode_tool_approval(
                st,
                message_id,
                chat_id,
                approval_session_id,
                notice,
                follow_idx,
                ToolApprovalInteractiveWaitKind::Http,
            )
            .await?;
        }
        FeishuToolApprovalMode::WaitMessage => {
            resolve_wait_mode_tool_approval(
                st,
                message_id,
                chat_id,
                approval_session_id,
                notice,
                follow_idx,
                ToolApprovalInteractiveWaitKind::Message,
            )
            .await?;
        }
    }
    Ok(())
}
