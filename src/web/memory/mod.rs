//! `/memory/*` handler：长期记忆只读列表与按 id 删除（P0）。
//!
//! 契约真源见 `docs/design/memory_management_api.md` §3；JSON 形状见
//! [`crate::web::http_types::memory`]；路由表见 [`crate::web::routes::memory`]。

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::cm_api_contract::error_codes;
use crate::cm_memory::memory::long_term_memory::LongTermMemoryRuntime;
use crate::cm_memory::memory::long_term_memory_store::{
    MemoryKindFilter, MemoryListFilter, memory_kind_for_source_role,
};
use crate::web::app_state_facets::MemoryAppFacet;
use crate::web::http_types::memory::{
    MemoryDeleteQuery, MemoryEntryView, MemoryErrorBody, MemoryListQuery, MemoryListResponse,
    parse_tags_json,
};
use crate::web::normalize_client_conversation_id;

const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 200;

fn bad_request(code: &'static str, message: impl Into<String>) -> (StatusCode, Json<MemoryErrorBody>) {
    (StatusCode::BAD_REQUEST, Json(MemoryErrorBody::new(code, message)))
}

fn internal_error(message: impl Into<String>) -> (StatusCode, Json<MemoryErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(MemoryErrorBody::new(error_codes::INTERNAL_ERROR, message)),
    )
}

fn disabled() -> (StatusCode, Json<MemoryErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(MemoryErrorBody::disabled()),
    )
}

/// 长期记忆运行时可用性闸门：配置开启**且**运行时已装配，否则统一 503。
async fn require_runtime(
    state: &MemoryAppFacet,
) -> Result<Arc<LongTermMemoryRuntime>, (StatusCode, Json<MemoryErrorBody>)> {
    let enabled = state.cfg.read().await.long_term_memory.long_term_memory_enabled;
    enabled
        .then(|| state.long_term_memory.clone())
        .flatten()
        .ok_or_else(disabled)
}

/// 必填作用域守卫：复用 `conversation_id` 校验（长度上限、字符集）。
fn require_scope(raw: Option<&str>) -> Result<String, (StatusCode, Json<MemoryErrorBody>)> {
    match normalize_client_conversation_id(raw) {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(bad_request(
            error_codes::INVALID_CONVERSATION_ID,
            "conversation_id 不能为空",
        )),
        Err(msg) => Err(bad_request(error_codes::INVALID_CONVERSATION_ID, msg)),
    }
}

/// `kind` 参数 → 内部过滤枚举；非枚举值 400。
fn parse_kind_filter(
    raw: Option<&str>,
) -> Result<Option<MemoryKindFilter>, (StatusCode, Json<MemoryErrorBody>)> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match s {
        "turn" => Ok(Some(MemoryKindFilter::Turn)),
        "explicit" => Ok(Some(MemoryKindFilter::Explicit)),
        "experience" => Ok(Some(MemoryKindFilter::Experience)),
        "other" => Ok(Some(MemoryKindFilter::Other)),
        _ => Err(bad_request(
            error_codes::INVALID_MEMORY_KIND,
            format!("不支持的 kind：{s}（可选 turn / explicit / experience / other）"),
        )),
    }
}

/// `sort`：仅 `created_at_asc` 为升序，缺省或其它值一律降序（P0 不为该参数新增错误码）。
fn sort_ascending(raw: Option<&str>) -> bool {
    matches!(raw.map(str::trim), Some("created_at_asc"))
}

fn trimmed_owned(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `GET /memory/list`：按 scope 投影列表（**不含** `embedding`），分页 / 过滤 / 排序 / 总数。
pub(crate) async fn memory_list_handler(
    State(state): State<MemoryAppFacet>,
    Query(q): Query<MemoryListQuery>,
) -> Result<Json<MemoryListResponse>, (StatusCode, Json<MemoryErrorBody>)> {
    let runtime = require_runtime(&state).await?;
    let scope = require_scope(q.conversation_id.as_deref())?;
    let kind = parse_kind_filter(q.kind.as_deref())?;
    let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let offset = q.offset.unwrap_or(0);
    let tag = trimmed_owned(q.tag);
    let text = trimmed_owned(q.q);
    let sort_asc = sort_ascending(q.sort.as_deref());

    let scope_for_closure = scope.clone();
    let (rows, total) = tokio::task::spawn_blocking(move || {
        let filter = MemoryListFilter {
            kind,
            tag: tag.as_deref(),
            q: text.as_deref(),
            sort_asc,
        };
        runtime.list_projected_for_scope_blocking(&scope_for_closure, &filter, limit, offset)
    })
    .await
    .map_err(|e| internal_error(format!("长期记忆列表任务失败: {e}")))?
    .map_err(internal_error)?;

    let entries = rows
        .into_iter()
        .map(|r| MemoryEntryView {
            id: r.id,
            text: r.chunk_text,
            kind: memory_kind_for_source_role(&r.source_role),
            tags: parse_tags_json(&r.tags_json),
            created_at_unix: r.created_at_unix,
            expires_at_unix: r.expires_at_unix,
        })
        .collect::<Vec<_>>();
    let consumed = offset.saturating_add(entries.len());
    let has_more = (consumed as i64) < total;
    log::debug!(
        "memory list conversation_id={scope} limit={limit} offset={offset} returned={} total={total}",
        entries.len()
    );
    Ok(Json(MemoryListResponse {
        conversation_id: scope,
        entries,
        total,
        limit,
        offset,
        has_more,
    }))
}

/// `DELETE /memory/{id}`：按 `(conversation_id, id)` 删除，幂等（未命中同样 204）。
pub(crate) async fn memory_delete_handler(
    State(state): State<MemoryAppFacet>,
    Path(id): Path<String>,
    Query(q): Query<MemoryDeleteQuery>,
) -> Result<StatusCode, (StatusCode, Json<MemoryErrorBody>)> {
    let runtime = require_runtime(&state).await?;
    let scope = require_scope(q.conversation_id.as_deref())?;
    let memory_id: i64 = id.trim().parse().map_err(|_| {
        bad_request(
            error_codes::INVALID_MEMORY_ID,
            "memory id 必须是十进制整数",
        )
    })?;

    let scope_for_closure = scope.clone();
    let deleted = tokio::task::spawn_blocking(move || {
        runtime.delete_by_id_for_scope_blocking(&scope_for_closure, memory_id)
    })
    .await
    .map_err(|e| internal_error(format!("删除长期记忆任务失败: {e}")))?
    .map_err(internal_error)?;
    log::debug!("memory delete conversation_id={scope} id={memory_id} deleted={deleted}");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_config::LongTermMemoryVectorBackend;
    use crate::cm_memory::memory::long_term_memory_store;
    use rusqlite::Connection;
    use std::sync::Mutex;

    /// 内存 SQLite + 建表；返回连接便于插桩与断言。
    fn test_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        long_term_memory_store::migrate(&conn).expect("migrate");
        Arc::new(Mutex::new(conn))
    }

    /// 直接构造 [`MemoryAppFacet`]（绕过整包 [`crate::AppState`]）。
    fn facet(enabled: bool, runtime: Option<Arc<LongTermMemoryRuntime>>) -> MemoryAppFacet {
        let mut cfg = crate::cm_config::load_config_test_env::without_cm_env_overrides(|| {
            crate::cm_config::load_config(None).expect("加载测试默认配置")
        });
        cfg.long_term_memory.long_term_memory_enabled = enabled;
        cfg.long_term_memory.long_term_memory_vector_backend = LongTermMemoryVectorBackend::Disabled;
        MemoryAppFacet {
            cfg: Arc::new(tokio::sync::RwLock::new(cfg)),
            long_term_memory: runtime,
        }
    }

    fn conn_scope_count(conn: &Arc<Mutex<Connection>>, scope: &str) -> i64 {
        let c = conn.lock().unwrap();
        long_term_memory_store::count_projected_for_scope(&c, scope, &MemoryListFilter::default())
            .unwrap()
    }

    fn list_query(conversation_id: Option<&str>) -> MemoryListQuery {
        MemoryListQuery {
            conversation_id: conversation_id.map(str::to_string),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn list_projects_paginates_and_isolates_scope() {
        let conn = test_conn();
        {
            let c = conn.lock().unwrap();
            long_term_memory_store::insert_explicit_chunk(
                &c,
                "s1",
                "one",
                "explicit",
                r#"["math"]"#,
                None,
                None,
            )
            .unwrap();
            long_term_memory_store::insert_chunk(&c, "s1", "two", "user", None, None).unwrap();
            long_term_memory_store::insert_explicit_chunk(
                &c,
                "s1",
                "three",
                "brand_new_role",
                "[]",
                None,
                None,
            )
            .unwrap();
            long_term_memory_store::insert_chunk(&c, "s2", "other scope", "user", None, None)
                .unwrap();
        }
        let state = facet(true, Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))));

        let mut q = list_query(Some("s1"));
        q.limit = Some(2);
        q.offset = Some(0);
        let page1 = memory_list_handler(State(state.clone()), Query(q))
            .await
            .expect("list ok")
            .0;
        assert_eq!(page1.conversation_id, "s1");
        assert_eq!(page1.total, 3);
        assert_eq!(page1.limit, 2);
        assert_eq!(page1.offset, 0);
        assert_eq!(page1.entries.len(), 2);
        assert!(page1.has_more);
        // scope 隔离：s2 的行不出现在结果里。
        assert!(page1.entries.iter().all(|e| e.text != "other scope"));

        let mut q2 = list_query(Some("s1"));
        q2.limit = Some(2);
        q2.offset = Some(2);
        let page2 = memory_list_handler(State(state), Query(q2))
            .await
            .expect("list ok")
            .0;
        assert_eq!(page2.entries.len(), 1);
        assert!(!page2.has_more);

        // 未知 source_role 落 `other`，且 `kind` / `tags` 投影正确。
        let all = list_query(Some("s1"));
        let full = memory_list_handler(
            State(facet(
                true,
                Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))),
            )),
            Query(all),
        )
        .await
        .expect("list ok")
        .0;
        let by_text = |t: &str| full.entries.iter().find(|e| e.text == t).unwrap();
        assert_eq!(by_text("one").kind, "explicit");
        assert_eq!(by_text("one").tags, vec!["math"]);
        assert_eq!(by_text("two").kind, "turn");
        assert_eq!(by_text("three").kind, "other");
    }

    #[tokio::test]
    async fn list_clamps_limit_and_returns_empty_scope() {
        let conn = test_conn();
        let state = facet(
            true,
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))),
        );

        // 空 scope → 200 空列表。
        let empty = memory_list_handler(State(state.clone()), Query(list_query(Some("s1"))))
            .await
            .expect("list ok")
            .0;
        assert!(empty.entries.is_empty());
        assert_eq!(empty.total, 0);
        assert!(!empty.has_more);

        // 超限被 clamp 到 200；0 被 clamp 到 1。
        let mut hi = list_query(Some("s1"));
        hi.limit = Some(9999);
        assert_eq!(
            memory_list_handler(State(state.clone()), Query(hi))
                .await
                .expect("list ok")
                .0
                .limit,
            200
        );
        let mut lo = list_query(Some("s1"));
        lo.limit = Some(0);
        assert_eq!(
            memory_list_handler(State(state), Query(lo))
                .await
                .expect("list ok")
                .0
                .limit,
            1
        );
    }

    #[tokio::test]
    async fn list_rejects_bad_scope_and_kind() {
        let conn = test_conn();
        let state = facet(
            true,
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))),
        );

        let missing = memory_list_handler(State(state.clone()), Query(list_query(None)))
            .await
            .unwrap_err();
        assert_eq!(missing.0, StatusCode::BAD_REQUEST);
        assert_eq!(missing.1.0.code, error_codes::INVALID_CONVERSATION_ID);

        let bad_id = memory_list_handler(State(state.clone()), Query(list_query(Some("bad!id"))))
            .await
            .unwrap_err();
        assert_eq!(bad_id.0, StatusCode::BAD_REQUEST);
        assert_eq!(bad_id.1.0.code, error_codes::INVALID_CONVERSATION_ID);

        let mut bad_kind = list_query(Some("s1"));
        bad_kind.kind = Some("bogus".into());
        let err = memory_list_handler(State(state), Query(bad_kind))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1.0.code, error_codes::INVALID_MEMORY_KIND);
    }

    #[tokio::test]
    async fn disabled_runtime_returns_503_with_enabled_false() {
        let conn = test_conn();
        let runtime = LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn));

        // 配置开启但运行时未装配。
        let no_runtime = memory_list_handler(State(facet(true, None)), Query(list_query(Some("s1"))))
            .await
            .unwrap_err();
        assert_eq!(no_runtime.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(no_runtime.1.0.code, error_codes::LONG_TERM_MEMORY_DISABLED);
        assert_eq!(no_runtime.1.0.enabled, Some(false));

        // 配置关闭（即使运行时在）。
        let disabled = memory_list_handler(
            State(facet(false, Some(runtime))),
            Query(list_query(Some("s1"))),
        )
        .await
        .unwrap_err();
        assert_eq!(disabled.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(disabled.1.0.code, error_codes::LONG_TERM_MEMORY_DISABLED);
    }

    #[tokio::test]
    async fn delete_is_idempotent_and_scoped() {
        let conn = test_conn();
        let id = {
            let c = conn.lock().unwrap();
            long_term_memory_store::insert_explicit_chunk(&c, "s1", "m1", "explicit", "[]", None, None)
                .unwrap()
        };
        let state = facet(
            true,
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))),
        );

        // 跨 scope 的合法 id → 204，但目标行仍在（作用域守卫反向断言）。
        let cross = memory_delete_handler(
            State(state.clone()),
            Path(id.to_string()),
            Query(MemoryDeleteQuery {
                conversation_id: Some("s2".into()),
            }),
        )
        .await
        .expect("cross-scope delete");
        assert_eq!(cross, StatusCode::NO_CONTENT);
        assert_eq!(conn_scope_count(&conn, "s1"), 1);

        // 命中 → 204 且行消失。
        let hit = memory_delete_handler(
            State(state.clone()),
            Path(id.to_string()),
            Query(MemoryDeleteQuery {
                conversation_id: Some("s1".into()),
            }),
        )
        .await
        .expect("delete ok");
        assert_eq!(hit, StatusCode::NO_CONTENT);
        assert_eq!(conn_scope_count(&conn, "s1"), 0);

        // 重复删除 → 仍 204。
        let again = memory_delete_handler(
            State(state),
            Path(id.to_string()),
            Query(MemoryDeleteQuery {
                conversation_id: Some("s1".into()),
            }),
        )
        .await
        .expect("repeat delete");
        assert_eq!(again, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_rejects_bad_id_and_missing_scope() {
        let conn = test_conn();
        let state = facet(
            true,
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(&conn))),
        );

        let bad_id = memory_delete_handler(
            State(state.clone()),
            Path("abc".to_string()),
            Query(MemoryDeleteQuery {
                conversation_id: Some("s1".into()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(bad_id.0, StatusCode::BAD_REQUEST);
        assert_eq!(bad_id.1.0.code, error_codes::INVALID_MEMORY_ID);

        let no_scope = memory_delete_handler(
            State(state),
            Path("1".to_string()),
            Query(MemoryDeleteQuery { conversation_id: None }),
        )
        .await
        .unwrap_err();
        assert_eq!(no_scope.0, StatusCode::BAD_REQUEST);
        assert_eq!(no_scope.1.0.code, error_codes::INVALID_CONVERSATION_ID);
    }
}
