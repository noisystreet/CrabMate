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
