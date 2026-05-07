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
