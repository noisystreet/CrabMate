async fn handle_im_message_receive(
    st: &FeishuBridgeState,
    envelope: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sender_type = envelope
        .pointer("/event/sender/sender_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if sender_type != "user" {
        return Ok(());
    }

    let message = envelope
        .pointer("/event/message")
        .cloned()
        .unwrap_or(json!({}));
    let message_id = message
        .get("message_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if message_id.is_empty() {
        return Ok(());
    }

    if is_duplicate(&st.seen_message_ids, &message_id, st.cfg.dedup_ttl) {
        return Ok(());
    }

    let chat_id = message
        .get("chat_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if chat_id.is_empty() {
        return Ok(());
    }

    let chat_type = message
        .get("chat_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");

    if st.cfg.group_require_bot_mention && chat_type == "group" {
        let Some(bot_id) = st
            .cfg
            .bot_open_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            warn!(
                "FEISHU_GROUP_REQUIRE_BOT_MENTION=1 but FEISHU_BOT_OPEN_ID empty; skip group message"
            );
            return Ok(());
        };
        if !message_mentions_bot_open_id(&message, bot_id) {
            return Ok(());
        }
    }

    let msg_type = message
        .get("message_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let content_raw = message
        .get("content")
        .and_then(|x| x.as_str())
        .unwrap_or("{}");

    if st.cfg.tool_approval_mode == FeishuToolApprovalMode::WaitMessage
        && let Some(session_id) = st
            .pending_tool_session_by_chat
            .get(&chat_id)
            .map(|e| e.value().clone())
        && msg_type == "text"
        && let Ok(c) = serde_json::from_str::<Value>(content_raw)
        && let Some(plain) = c.get("text").and_then(|x| x.as_str())
        && let Some(dec) = feishu_message_command_decision(plain)
        && let Some((_, pending)) = st.pending_tool_decisions.remove(&session_id)
    {
        let _ = pending.reply_tx.send(dec.to_string());
        st.pending_tool_session_by_chat.remove(&chat_id);
        reply_text_message(st, &message_id, "已收到审批，正在继续执行…").await?;
        return Ok(());
    }

    let Some(text) =
        incoming_content_as_user_text(msg_type, content_raw, st.cfg.max_message_content_json_chars)
    else {
        return Ok(());
    };
    let text = strip_feishu_mention_placeholders(&text);
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }

    {
        let _turn = st.turn_lock.lock().await;
        ensure_workspace_for_chat(st, &chat_id).await?;
    }

    crabmate_turn_with_feishu(st, &message_id, &chat_id, text).await?;
    Ok(())
}

async fn crabmate_turn_with_feishu(
    st: &FeishuBridgeState,
    message_id: &str,
    chat_id: &str,
    user_text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conv = format!("feishu:{chat_id}");
    let approval_session_id = match st.cfg.tool_approval_mode {
        FeishuToolApprovalMode::DenyAll => None,
        _ => Some(format!("feishu:{message_id}")),
    };

    let mut follow_idx: u64 = 0;
    let mut progress_card_message_id: Option<String> = None;
    if st.cfg.in_place_progress_card {
        follow_idx = follow_idx.saturating_add(1);
        let uuid = format!("{message_id}-prog-{follow_idx}");
        let card = feishu_tool_card::progress_placeholder_card();
        match reply_interactive_card(st, message_id, &uuid, &card).await {
            Ok(Some(mid)) if !mid.is_empty() => progress_card_message_id = Some(mid),
            Ok(_) => {
                warn!("feishu progress card sent but no message_id in reply; fallback text");
                follow_idx = follow_idx.saturating_add(1);
                reply_followup_text_message(
                    st,
                    message_id,
                    follow_idx,
                    "⏳ 已开始处理：CrabMate 正在执行（若含工具可能耗时数分钟）。完成后将发送结果摘要。",
                )
                .await?;
            }
            Err(e) => {
                warn!(?e, "feishu progress placeholder card failed; fallback text");
                follow_idx = follow_idx.saturating_add(1);
                reply_followup_text_message(
                    st,
                    message_id,
                    follow_idx,
                    "⏳ 已开始处理：CrabMate 正在执行（若含工具可能耗时数分钟）。完成后将发送结果摘要。",
                )
                .await?;
            }
        }
    } else {
        follow_idx = follow_idx.saturating_add(1);
        reply_followup_text_message(
            st,
            message_id,
            follow_idx,
            "⏳ 已开始处理：CrabMate 正在执行（若含工具可能耗时数分钟）。完成后将发送结果摘要卡片。",
        )
        .await?;
    }

    let resp = match st
        .cfg
        .crabmate
        .post_chat_stream(user_text, Some(&conv), approval_session_id.as_deref())
        .await
    {
        Ok(r) => r,
        Err(CrabmateError::HttpStatus {
            status,
            body_preview,
        }) => {
            warn!(status, %body_preview, "CrabMate /chat/stream error");
            follow_idx = follow_idx.saturating_add(1);
            reply_followup_text_message(
                st,
                message_id,
                follow_idx,
                &format!("（CrabMate 返回 HTTP {status}，请检查服务与密钥。）"),
            )
            .await?;
            return Ok(());
        }
        Err(e) => {
            warn!(?e, "CrabMate /chat/stream request failed");
            follow_idx = follow_idx.saturating_add(1);
            reply_followup_text_message(
                st,
                message_id,
                follow_idx,
                &format!("（调用 CrabMate 失败：{e}）"),
            )
            .await?;
            return Ok(());
        }
    };

    let mut acc = StreamAccum::default();
    consume_sse_stream_to_accum(
        st,
        message_id,
        chat_id,
        approval_session_id.as_deref(),
        resp,
        &mut acc,
        &mut follow_idx,
    )
    .await?;

    let card_cap = st.cfg.result_card_max_body_chars.max(200);
    if acc.saw_error && acc.answer.trim().is_empty() {
        follow_idx = follow_idx.saturating_add(1);
        reply_followup_text_message(
            st,
            message_id,
            follow_idx,
            &format!("（CrabMate 流结束报错：{}）", acc.error_preview),
        )
        .await?;
        return Ok(());
    }

    let final_text = if acc.answer.trim().is_empty() {
        "（本轮无正文输出）".to_string()
    } else {
        acc.answer
    };
    let title = if acc.saw_error {
        "完成（部分报错）"
    } else {
        "CrabMate 执行完成"
    };
    reply_turn_result_card_and_remainder(
        st,
        message_id,
        progress_card_message_id.as_deref(),
        title,
        &final_text,
        card_cap,
        &mut follow_idx,
    )
    .await?;
    Ok(())
}

async fn consume_sse_stream_to_accum(
    st: &FeishuBridgeState,
    message_id: &str,
    chat_id: &str,
    approval_session_id: Option<&str>,
    resp: reqwest::Response,
    acc: &mut StreamAccum,
    follow_idx: &mut u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _conv_hdr = resp
        .headers()
        .get("x-conversation-id")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut buf = String::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(
            |e: reqwest::Error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) },
        )?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        for block in take_complete_sse_blocks(&mut buf) {
            dispatch_one_sse_block_to_feishu(
                st,
                message_id,
                chat_id,
                approval_session_id,
                &block,
                acc,
                follow_idx,
            )
            .await?;
        }
    }
    if !buf.trim().is_empty() {
        buf.push_str("\n\n");
        for block in take_complete_sse_blocks(&mut buf) {
            dispatch_one_sse_block_to_feishu(
                st,
                message_id,
                chat_id,
                approval_session_id,
                &block,
                acc,
                follow_idx,
            )
            .await?;
        }
    }
    Ok(())
}

async fn dispatch_one_sse_block_to_feishu(
    st: &FeishuBridgeState,
    message_id: &str,
    chat_id: &str,
    approval_session_id: Option<&str>,
    block: &str,
    acc: &mut StreamAccum,
    follow_idx: &mut u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (appr, status_lines) = dispatch_sse_event_block_collect(block, acc);
    if !st.cfg.quiet_sse_status {
        for line in status_lines {
            *follow_idx = follow_idx.saturating_add(1);
            reply_followup_text_message(st, message_id, *follow_idx, &line).await?;
        }
    }
    for notice in appr {
        if let Some(sid) = approval_session_id {
            resolve_one_tool_approval(st, message_id, chat_id, sid, &notice, follow_idx).await?;
        }
    }
    Ok(())
}

async fn reply_turn_result_card_and_remainder(
    st: &FeishuBridgeState,
    message_id: &str,
    progress_card_message_id: Option<&str>,
    title: &str,
    final_text: &str,
    card_cap: usize,
    follow_idx: &mut u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (card_body, remainder) = split_for_result_card(final_text, card_cap);
    let card = feishu_tool_card::turn_result_card(title, &card_body);
    let mut seq_after = *follow_idx;
    let mut patched = false;
    if let Some(pid) = progress_card_message_id {
        if patch_interactive_card_message(st, pid, &card).await.is_ok() {
            patched = true;
        } else {
            warn!("feishu PATCH result onto progress card failed; send new card");
        }
    }
    if !patched {
        *follow_idx = follow_idx.saturating_add(1);
        let card_uuid = format!("{message_id}-result-{follow_idx}");
        seq_after = *follow_idx;
        if reply_interactive_card(st, message_id, &card_uuid, &card)
            .await
            .is_err()
        {
            warn!("feishu result card failed; fallback text");
            seq_after = seq_after.saturating_add(1);
            reply_followup_text_message(
                st,
                message_id,
                seq_after,
                &format!("{title}\n{}", card_body),
            )
            .await?;
        }
    }
    if !remainder.is_empty() {
        reply_text_chunks_followup(st, message_id, seq_after, &remainder, 3500).await?;
    }
    Ok(())
}

fn split_for_result_card(s: &str, max_body_chars: usize) -> (String, String) {
    let t = s.trim();
    if t.is_empty() {
        return ("（无正文）".to_string(), String::new());
    }
    let n = t.chars().count();
    if n <= max_body_chars {
        return (t.to_string(), String::new());
    }
    let head_len = max_body_chars.saturating_sub(30).max(80);
    let head: String = t.chars().take(head_len).collect();
    let card_body = format!("{head}\n…（摘要已截断，下文单独发送）");
    let tail: String = t.chars().skip(head_len).collect();
    (card_body, tail)
}

async fn reply_text_chunks_followup(
    st: &FeishuBridgeState,
    message_id: &str,
    mut start_seq: u64,
    text: &str,
    chunk_max: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts = split_reply_for_feishu(text, chunk_max);
    for seg in parts {
        start_seq = start_seq.saturating_add(1);
        reply_followup_text_message(st, message_id, start_seq, &seg).await?;
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
            let (tx, rx) = oneshot::channel();
            st.pending_tool_decisions.insert(
                approval_session_id.to_string(),
                PendingToolDecision { reply_tx: tx },
            );
            st.pending_tool_session_by_chat
                .insert(chat_id.to_string(), approval_session_id.to_string());
            *follow_idx = follow_idx.saturating_add(1);
            let uuid = format!("{message_id}-ap-{follow_idx}");
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
                reply_followup_text_message(
                    st,
                    message_id,
                    *follow_idx,
                    &format!(
                        "⏸ 需人工审批工具：{}\n参数摘要：{}\n请在桥接服务 POST /feishu/tool-decision（Bearer 或 X-API-Key 为 FEISHU_TOOL_DECISION_SECRET），body: {{\"approval_session_id\":\"{}\",\"decision\":\"deny|allow_once|allow_always\"}}",
                        notice.command,
                        clip_one_line(&notice.args, 400),
                        approval_session_id
                    ),
                )
                .await?;
            }

            let timeout_secs = st.cfg.tool_decision_timeout_secs.max(5);
            let decision = match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
                Ok(Ok(d)) => d,
                Ok(Err(_)) => {
                    warn!("tool decision channel closed");
                    let _ = st
                        .cfg
                        .crabmate
                        .send_chat_approval(approval_session_id, "deny")
                        .await;
                    "deny".to_string()
                }
                Err(_) => {
                    warn!("tool decision wait timeout; deny");
                    "deny".to_string()
                }
            };
            st.pending_tool_decisions.remove(approval_session_id);
            st.pending_tool_session_by_chat.remove(chat_id);
            if let Err(e) = st
                .cfg
                .crabmate
                .send_chat_approval(approval_session_id, decision.trim())
                .await
            {
                warn!(?e, "send_chat_approval after wait failed");
            }
        }
        FeishuToolApprovalMode::WaitMessage => {
            let (tx, rx) = oneshot::channel();
            st.pending_tool_decisions.insert(
                approval_session_id.to_string(),
                PendingToolDecision { reply_tx: tx },
            );
            st.pending_tool_session_by_chat
                .insert(chat_id.to_string(), approval_session_id.to_string());
            *follow_idx = follow_idx.saturating_add(1);
            let uuid = format!("{message_id}-ap-{follow_idx}");
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
                reply_followup_text_message(
                    st,
                    message_id,
                    *follow_idx,
                    &format!(
                        "⏸ 需你确认是否执行：{}\n参数摘要：{}\n请发送：!允许一次 、 !永久允许 、 !拒绝",
                        notice.command,
                        clip_one_line(&notice.args, 400),
                    ),
                )
                .await?;
            }

            let timeout_secs = st.cfg.tool_decision_timeout_secs.max(5);
            let decision = match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
                Ok(Ok(d)) => d,
                Ok(Err(_)) => "deny".to_string(),
                Err(_) => {
                    let _ = st
                        .cfg
                        .crabmate
                        .send_chat_approval(approval_session_id, "deny")
                        .await;
                    "deny".to_string()
                }
            };
            st.pending_tool_decisions.remove(approval_session_id);
            st.pending_tool_session_by_chat.remove(chat_id);
            if let Err(e) = st
                .cfg
                .crabmate
                .send_chat_approval(approval_session_id, decision.trim())
                .await
            {
                warn!(?e, "send_chat_approval after message wait failed");
            }
        }
    }
    Ok(())
}
