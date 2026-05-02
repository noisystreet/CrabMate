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

async fn reply_interactive_card(
    st: &FeishuBridgeState,
    message_id: &str,
    uuid: &str,
    card: &Value,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let token = get_tenant_access_token(st).await?;
    let url = format!("https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply");
    let content = serde_json::to_string(card)?;
    let body = json!({
        "content": content,
        "msg_type": "interactive",
        "uuid": uuid,
    });
    let resp = st
        .http
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes).trim().to_string();
        warn!(%status, %preview, "feishu reply interactive API http error");
        return Err(format!("feishu interactive http {status}").into());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        warn!(%code, body=%String::from_utf8_lossy(&bytes), "feishu reply interactive API business error");
        return Err(format!("feishu interactive code {code}").into());
    }
    let new_id = v
        .pointer("/data/message_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(new_id)
}

async fn patch_interactive_card_message(
    st: &FeishuBridgeState,
    patch_message_id: &str,
    card: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = get_tenant_access_token(st).await?;
    let url = format!("https://open.feishu.cn/open-apis/im/v1/messages/{patch_message_id}");
    let content = serde_json::to_string(card)?;
    let body = json!({ "content": content });
    let resp = st
        .http
        .patch(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes).trim().to_string();
        warn!(%status, %preview, "feishu PATCH interactive message http error");
        return Err(format!("feishu patch interactive http {status}").into());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        warn!(%code, body=%String::from_utf8_lossy(&bytes), "feishu PATCH interactive message business error");
        return Err(format!("feishu patch interactive code {code}").into());
    }
    Ok(())
}

async fn reply_tool_approval_interactive_card(
    st: &FeishuBridgeState,
    message_id: &str,
    uuid: &str,
    notice: &crate::sse_consumer::CommandApprovalNotice,
    approval_session_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let card = feishu_tool_card::tool_approval_interactive_content(
        &notice.command,
        &clip_one_line(&notice.args, 1200),
        approval_session_id,
    );
    let _ = reply_interactive_card(st, message_id, uuid, &card).await?;
    Ok(())
}

fn clip_one_line(s: &str, max_chars: usize) -> String {
    let t = s.trim().replace('\n', " ");
    let count = t.chars().count();
    if count <= max_chars {
        t
    } else {
        t.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

async fn reply_followup_text_message(
    st: &FeishuBridgeState,
    root_message_id: &str,
    seq: u64,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let uuid = format!("{root_message_id}-{seq}");
    reply_text_message_with_uuid(st, root_message_id, &uuid, text).await
}

async fn reply_text_message(
    st: &FeishuBridgeState,
    message_id: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    reply_text_message_with_uuid(st, message_id, message_id, text).await
}

async fn reply_text_message_with_uuid(
    st: &FeishuBridgeState,
    message_id: &str,
    uuid: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = get_tenant_access_token(st).await?;
    let url = format!("https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply");
    let content = serde_json::to_string(&json!({ "text": text }))?;
    let body = json!({
        "content": content,
        "msg_type": "text",
        "uuid": uuid,
    });
    let resp = st
        .http
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes).trim().to_string();
        warn!(%status, %preview, "feishu reply API http error");
        return Ok(());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        warn!(%code, body=%String::from_utf8_lossy(&bytes), "feishu reply API business error");
    }
    Ok(())
}

fn split_reply_for_feishu(s: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(256);
    let count = s.chars().count();
    if count <= max_chars {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if rest.chars().count() <= max_chars {
            out.push(rest.to_string());
            break;
        }
        let chunk: String = rest.chars().take(max_chars).collect();
        let split_at = chunk
            .char_indices()
            .rev()
            .find(|(_, c)| *c == '\n')
            .map(|(i, _)| i)
            .or_else(|| {
                chunk
                    .char_indices()
                    .rev()
                    .find(|(_, c)| c.is_whitespace())
                    .map(|(i, _)| i)
            })
            .unwrap_or_else(|| {
                chunk
                    .char_indices()
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(chunk.len())
            });
        let (a, b) = rest.split_at(split_at.min(rest.len()));
        let piece = a.trim_end();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        rest = b.trim_start();
        if rest.is_empty() {
            break;
        }
    }
    out
}

async fn ensure_workspace_for_chat(
    st: &FeishuBridgeState,
    chat_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(tmpl) = st
        .cfg
        .workspace_root_template
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };

    let path = expand_workspace_root_template(tmpl, chat_id);
    if path.is_empty() {
        return Ok(());
    }

    let mut last = st.last_workspace_path.lock().await;
    if last.as_deref() == Some(path.as_str()) {
        return Ok(());
    }

    st.cfg.crabmate.set_workspace(&path).await.map_err(
        |e: CrabmateError| -> Box<dyn std::error::Error + Send + Sync> {
            warn!(error=%e, path=%path, "CrabMate POST /workspace failed");
            Box::new(e)
        },
    )?;
    *last = Some(path);
    Ok(())
}

fn message_mentions_bot_open_id(message: &Value, bot_open_id: &str) -> bool {
    let Some(arr) = message.get("mentions").and_then(|m| m.as_array()) else {
        return false;
    };
    arr.iter().any(|m| {
        m.get("mentioned_type").and_then(|t| t.as_str()) == Some("bot")
            && m.pointer("/id/open_id").and_then(|x| x.as_str()) == Some(bot_open_id)
    })
}

fn strip_feishu_mention_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' && i + 7 <= bytes.len() && &bytes[i..i + 7] == b"@_user_" {
            let rest = &s[i + 7..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            i += 7 + end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out.trim().to_string()
}

fn is_duplicate(map: &DashMap<String, Instant>, id: &str, ttl: Duration) -> bool {
    let now = Instant::now();
    if let Some(v) = map.get(id)
        && now.duration_since(*v) < ttl
    {
        return true;
    }
    map.insert(id.to_string(), now);
    // 粗清理：条目过多时清空（MVP；生产可换 TTL 队列）
    if map.len() > 50_000 {
        map.clear();
    }
    false
}

async fn get_tenant_access_token(
    st: &FeishuBridgeState,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let now = time_unix_secs();
    let mut guard = st.token.lock().await;
    if !guard.token.is_empty() && guard.expires_at.saturating_sub(now) > 120 {
        return Ok(guard.token.clone());
    }

    let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
    let body = json!({
        "app_id": st.cfg.app_id,
        "app_secret": st.cfg.app_secret,
    });
    let resp = st.http.post(url).json(&body).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(format!("token http {}: {}", status, String::from_utf8_lossy(&bytes)).into());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!("token api code={code}: {}", String::from_utf8_lossy(&bytes)).into());
    }
    let token = v
        .get("tenant_access_token")
        .and_then(|t| t.as_str())
        .ok_or("missing tenant_access_token")?
        .to_string();
    let expire = v.get("expire").and_then(|e| e.as_i64()).unwrap_or(7200);
    guard.token = token.clone();
    guard.expires_at = now + expire;
    Ok(token)
}

fn time_unix_secs() -> i64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_verification_plain_challenge() {
        let v = json!({
            "type": "url_verification",
            "challenge": "abc123"
        });
        assert_eq!(url_verification_challenge(&v), Some("abc123".into()));
    }

    #[test]
    fn signature_skipped_when_no_header() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: Some("ek".into()),
            verify_signature_when_possible: true,
            verification_token: None,
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            max_message_content_json_chars: 12000,
            async_worker: false,
            event_queue_capacity: 1,
            workspace_root_template: None,
            tool_approval_mode: FeishuToolApprovalMode::DenyAll,
            tool_decision_secret: None,
            tool_decision_timeout_secs: 300,
            quiet_sse_status: false,
            result_card_max_body_chars: 3500,
            in_place_progress_card: false,
            event_queue_sqlite_path: None,
            sqlite_queue_max_retries: 5,
            sqlite_queue_poll_ms: 200,
            sqlite_queue_lease_secs: 600,
        };
        let headers = HeaderMap::new();
        assert!(!verify_lark_signature_if_needed(&cfg, &headers, "{}").unwrap());
    }

    #[test]
    fn parse_lark_ts_seconds_vs_millis() {
        assert_eq!(parse_lark_timestamp_secs("1600000000"), Some(1_600_000_000));
        assert_eq!(
            parse_lark_timestamp_secs("1600000000000"),
            Some(1_600_000_000)
        );
    }

    #[test]
    fn verification_token_ok() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: None,
            verify_signature_when_possible: false,
            verification_token: Some("vtok".into()),
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            max_message_content_json_chars: 12000,
            async_worker: false,
            event_queue_capacity: 1,
            workspace_root_template: None,
            tool_approval_mode: FeishuToolApprovalMode::DenyAll,
            tool_decision_secret: None,
            tool_decision_timeout_secs: 300,
            quiet_sse_status: false,
            result_card_max_body_chars: 3500,
            in_place_progress_card: false,
            event_queue_sqlite_path: None,
            sqlite_queue_max_retries: 5,
            sqlite_queue_poll_ms: 200,
            sqlite_queue_lease_secs: 600,
        };
        let v = json!({ "header": { "token": "vtok" } });
        assert!(verify_event_verification_token(&cfg, &v).is_ok());
    }

    #[test]
    fn mentions_detect_bot_open_id() {
        let m = json!({
            "mentions": [
                {
                    "mentioned_type": "bot",
                    "id": { "open_id": "ou_bot_1" }
                }
            ]
        });
        assert!(message_mentions_bot_open_id(&m, "ou_bot_1"));
        assert!(!message_mentions_bot_open_id(&m, "ou_other"));
    }

    #[test]
    fn split_for_result_card_splits_tail() {
        let s = "x".repeat(100);
        let (card, tail) = split_for_result_card(&s, 50);
        assert!(card.contains('…'));
        assert!(!tail.is_empty());
        assert!(card.chars().count() + tail.chars().count() >= s.chars().count());
    }
}
