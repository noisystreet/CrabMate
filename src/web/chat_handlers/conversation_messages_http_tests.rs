//! `GET /conversation/messages` 分页窗口的轻量 HTTP 契约：真实 axum，不启 LLM、不挂 Bearer。
//!
//! Client 侧的 `victauri_pagination` 曾直接断言这条契约的三条窗口与未知会话 404（#1396）；
//! 迁走后由本仓钉住，免得「参数还在、语义已经漂」。纯切片逻辑另有
//! [`crate::web::conversation_messages_window`] 单测，但那只覆盖函数本身——客户端可见消息过滤
//! （`filter_messages_for_web_client_snapshot`）与 handler 的接线只有走 HTTP 才看得见。
//!
//! 播种直接走 `AppState`：E2E 夹具路由要 `CM_E2E_FIXTURES=1`，契约测试不该依赖环境变量。

use crate::test_serve::{TestServeHandle, start_test_serve};
use crate::types::{Message, MessageContent};
use crate::web::conversation_messages_window::MAX_CONVERSATION_MESSAGES_PAGE_LIMIT;

/// 主用例的播种规模（与 Client 原用例同规模）：100 条才能让 `limit=80` 切出「尾页 20 + 更早 80」。
const SEEDED_MESSAGES: u32 = 100;
const PAGE_LIMIT: u32 = 80;
/// 尾页窗口的起点。派生而非硬编码：改上面两个常量时断言自动跟着走。
const TAIL_WINDOW_START: u32 = SEEDED_MESSAGES - PAGE_LIMIT;
const CONVERSATION_ID: &str = "conv_messages_pagination_smoke";

/// 上限用例的播种规模：**必须**超过单页上限，否则「按上限截断」与「恰好给全量」在响应里无法区分。
const SEEDED_MESSAGES_OVER_CAP: u32 = MAX_CONVERSATION_MESSAGES_PAGE_LIMIT + 50;
const OVER_CAP_CONVERSATION_ID: &str = "conv_messages_pagination_over_cap";

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

/// 第 `index` 条消息的正文；断言靠正文认位置，比靠角色/顺序稳。
fn message_text(index: u32) -> String {
    format!("e2e-msg-{index}")
}

fn seeded_message(index: u32) -> Message {
    let role = if index.is_multiple_of(2) {
        "user"
    } else {
        "assistant"
    };
    Message {
        role: role.to_string(),
        content: Some(MessageContent::from(message_text(index))),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

async fn seed_conversation(handle: &TestServeHandle, conversation_id: &str, count: u32) {
    let messages: Vec<Message> = (0..count).map(seeded_message).collect();
    let outcome = handle
        .state
        .conversation
        .save_conversation_messages_if_revision(
            conversation_id.to_string(),
            messages,
            None,
            None,
            None,
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(outcome, crate::SaveConversationOutcome::Saved, "造会话失败");
}

/// 先钉状态码再解析：失败时直接看到 status + 错误体，而不是「messages 不是数组」。
async fn fetch_messages(
    client: &reqwest::Client,
    base_url: &str,
    query: &[(&str, String)],
) -> serde_json::Value {
    let resp = client
        .get(format!("{base_url}/conversation/messages"))
        .query(query)
        .send()
        .await
        .expect("GET /conversation/messages");
    let status = resp.status();
    let body = resp.text().await.expect("messages body");
    assert!(
        status.is_success(),
        "GET /conversation/messages?{} 期望 2xx，实际 {status}：{body}",
        describe_query(query)
    );
    serde_json::from_str(&body).expect("messages json")
}

fn describe_query(query: &[(&str, String)]) -> String {
    query
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// 响应里每行的 `content`（`MessageContent::Text` 序列化成字符串）。
fn message_texts(body: &serde_json::Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("messages 必须是数组")
        .iter()
        .map(|m| {
            m["content"]
                .as_str()
                .expect("content 应为字符串")
                .to_string()
        })
        .collect()
}

fn conversation_query(conversation_id: &str) -> (&'static str, String) {
    ("conversation_id", conversation_id.to_string())
}

/// 三种窗口：无 `limit` 的全量、尾页、靠 `before_index` 回退的前一页。
#[tokio::test]
async fn pagination_windows_match_the_client_contract() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    seed_conversation(&handle, CONVERSATION_ID, SEEDED_MESSAGES).await;

    // ① 无 `limit` → 全窗口（旧客户端兼容路径）。
    let full = fetch_messages(
        &client,
        &handle.base_url,
        &[conversation_query(CONVERSATION_ID)],
    )
    .await;
    assert_eq!(full["total_count"], SEEDED_MESSAGES);
    assert_eq!(full["window_start_index"], 0);
    assert_eq!(full["has_older"].as_bool(), Some(false));
    let full_texts = message_texts(&full);
    assert_eq!(full_texts.len(), SEEDED_MESSAGES as usize);
    assert_eq!(full_texts.first(), Some(&message_text(0)));
    assert_eq!(
        full_texts.last(),
        Some(&message_text(SEEDED_MESSAGES - 1))
    );

    // ② `limit=80` → 尾页：窗口从第 20 条起，首条正是 `window_start_index` 那一条。
    let tail = fetch_messages(
        &client,
        &handle.base_url,
        &[
            conversation_query(CONVERSATION_ID),
            ("limit", PAGE_LIMIT.to_string()),
        ],
    )
    .await;
    assert_eq!(tail["total_count"], SEEDED_MESSAGES);
    assert_eq!(tail["window_start_index"], TAIL_WINDOW_START);
    assert_eq!(tail["has_older"].as_bool(), Some(true));
    let tail_texts = message_texts(&tail);
    assert_eq!(tail_texts.len(), PAGE_LIMIT as usize);
    assert_eq!(tail_texts.first(), Some(&message_text(TAIL_WINDOW_START)));
    assert_eq!(
        tail_texts.last(),
        Some(&message_text(SEEDED_MESSAGES - 1))
    );

    // ③ 用尾页的 `window_start_index` 当游标 → 前一页回到 0，且不再有更早。
    //    游标取自上一页响应（Client 也这么做），不写死：写死就测不出两者是否同源。
    let cursor = tail["window_start_index"]
        .as_u64()
        .expect("window_start_index u64");
    let older = fetch_messages(
        &client,
        &handle.base_url,
        &[
            conversation_query(CONVERSATION_ID),
            ("limit", PAGE_LIMIT.to_string()),
            ("before_index", cursor.to_string()),
        ],
    )
    .await;
    assert_eq!(older["total_count"], SEEDED_MESSAGES);
    assert_eq!(older["window_start_index"], 0);
    assert_eq!(older["has_older"].as_bool(), Some(false));
    let older_texts = message_texts(&older);
    assert_eq!(older_texts.len(), TAIL_WINDOW_START as usize);
    assert_eq!(older_texts.first(), Some(&message_text(0)));
    assert_eq!(
        older_texts.last(),
        Some(&message_text(TAIL_WINDOW_START - 1))
    );
}

/// `limit` 的两条边界：`0` 等价「省略」（契约明写），超过单页上限按上限截断。
#[tokio::test]
async fn limit_zero_is_full_window_and_oversized_limit_is_clamped() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    seed_conversation(&handle, OVER_CAP_CONVERSATION_ID, SEEDED_MESSAGES_OVER_CAP).await;

    // `limit=0`：handler 用 `filter(|n| n > 0)`，不能退化成空窗口。
    let zero = fetch_messages(
        &client,
        &handle.base_url,
        &[
            conversation_query(OVER_CAP_CONVERSATION_ID),
            ("limit", 0.to_string()),
        ],
    )
    .await;
    assert_eq!(zero["total_count"], SEEDED_MESSAGES_OVER_CAP);
    assert_eq!(zero["window_start_index"], 0);
    assert_eq!(zero["has_older"].as_bool(), Some(false));
    assert_eq!(
        message_texts(&zero).len(),
        SEEDED_MESSAGES_OVER_CAP as usize
    );

    // 超上限：只给 `MAX_CONVERSATION_MESSAGES_PAGE_LIMIT` 条。
    let oversized = fetch_messages(
        &client,
        &handle.base_url,
        &[
            conversation_query(OVER_CAP_CONVERSATION_ID),
            (
                "limit",
                (MAX_CONVERSATION_MESSAGES_PAGE_LIMIT * 2).to_string(),
            ),
        ],
    )
    .await;
    let expected_start = SEEDED_MESSAGES_OVER_CAP - MAX_CONVERSATION_MESSAGES_PAGE_LIMIT;
    assert_eq!(oversized["total_count"], SEEDED_MESSAGES_OVER_CAP);
    assert_eq!(oversized["window_start_index"], expected_start);
    assert_eq!(oversized["has_older"].as_bool(), Some(true));
    let oversized_texts = message_texts(&oversized);
    assert_eq!(
        oversized_texts.len(),
        MAX_CONVERSATION_MESSAGES_PAGE_LIMIT as usize
    );
    assert_eq!(
        oversized_texts.first(),
        Some(&message_text(expected_start))
    );
}

/// 未知 `conversation_id` 必须是 404 `CONVERSATION_NOT_FOUND`（不是空窗口的 200）。
#[tokio::test]
async fn unknown_conversation_is_404() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .get(format!("{}/conversation/messages", handle.base_url))
        .query(&[("conversation_id", "conv_messages_pagination_missing")])
        .send()
        .await
        .expect("GET /conversation/messages");
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.expect("error json");
    assert_eq!(body["code"], "CONVERSATION_NOT_FOUND");
}

/// E4/S3：`layout` **必填**。库内不可用（空 / 坏 JSON / v1）的行直接 **500 `CONVERSATION_LAYOUT_MISSING`**，
/// 不再回 `null`——官方 Client 已无「无布局」读路径，静默降级只会掩盖未回填的数据。
///
/// 用 `start_test_serve_with_config` 把会话库指向临时 SQLite：这样能造出 `save` 路径**写不出来**的行
/// （`save` 总会按 messages 派生 layout），而「未回填」正是本契约要区分的状态。
#[tokio::test]
async fn unusable_layout_answers_500_instead_of_null() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let db = tmp.path().join("conversations.db");
    let messages_json = serde_json::json!({
        "version": 1,
        "messages": vec![Message::user_only("hi".to_string())],
    })
    .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    {
        let conn = rusqlite::Connection::open(&db).expect("open db");
        crate::conversation_store::migrate(&conn).expect("migrate");
        for (id, layout) in [
            ("no_layout", ""),
            ("corrupt", "{not json"),
            ("v1", r#"{"layout_schema_version":1}"#),
            (
                "v2",
                r#"{"layout_schema_version":2,"projection_hash":"x","segments":[]}"#,
            ),
        ] {
            // `active_*` 列由 `migrate` 以 `ALTER TABLE ADD COLUMN` 补，缺省是 `NULL`，而读取侧按
            // `String` 取（空串 = 未设置）——不写空串会让 `load` 报错、handlers 回 404，掩盖本用例。
            conn.execute(
                "INSERT INTO crabmate_conversations \
                 (id, messages_json, revision, updated_at_unix, active_agent_role, \
                  active_session_mode, active_permission_level, layout_meta_json) \
                 VALUES (?1, ?2, 7, ?3, '', '', '', ?4)",
                rusqlite::params![id, messages_json, now as i64, layout],
            )
            .expect("seed row");
        }
    }

    // `start_test_serve` 固定用内存后端，而「未回填」的行只有 SQLite 后端才可能出现
    // （`save` 总会按 messages 派生 layout），所以直接把 backing 换成指向临时库的 SQLite。
    let handle = start_test_serve(None).await;
    let conn = rusqlite::Connection::open(&db).expect("open db");
    *handle.state.conversation.conversation_backing.write().await =
        crate::cm_session_store::ConversationBacking::Sqlite(std::sync::Arc::new(
            std::sync::Mutex::new(conn),
        ));
    let client = loopback_http_client();

    for id in ["no_layout", "corrupt", "v1"] {
        let resp = client
            .get(format!("{}/conversation/messages", handle.base_url))
            .query(&[conversation_query(id)])
            .send()
            .await
            .expect("GET /conversation/messages");
        let status = resp.status();
        let body = resp.text().await.expect("body");
        assert_eq!(
            status,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "{id} 的 layout 不可用，应 500 而不是回 null：{body}"
        );
        assert!(
            body.contains("CONVERSATION_LAYOUT_MISSING"),
            "{id} 的错误体应带 CONVERSATION_LAYOUT_MISSING：{body}"
        );
    }

    // 可用 v2 行照常 200，且 `layout` 是对象（不是 null）。
    let body = fetch_messages(&client, &handle.base_url, &[conversation_query("v2")]).await;
    assert_eq!(
        body["layout"]["layout_schema_version"].as_u64(),
        Some(2),
        "v2 会话必须带回 layout 对象：{body}"
    );
}
