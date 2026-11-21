//! `layout_coverage`（B4/P4 门禁）的独立测试模块。
//!
//! 单独成文件而不是塞进 `tests.rs`：后者已贴着 `fn-nloc` 的文件行数上限（920 行），
//! 再加用例就会超标——这本身也说明覆盖率统计不属会话 CRUD 的主测试面。

use super::backfill::backfill_layout_meta_in_conn;
use super::coverage::{LayoutCoverage, layout_coverage_from_conn};
use super::*;

#[test]
fn layout_coverage_counts_rows_without_layout() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");

    // 空表：total 为 0，占比是 None 而不是 0.0（空库不该报「0% 无 layout」）。
    let empty = layout_coverage_from_conn(&conn).expect("coverage");
    assert_eq!(empty, LayoutCoverage::default());
    assert_eq!(empty.legacy_ratio(), None);

    // 一行正常落盘（带 layout），另一行把该列清空以模拟旧库行。
    let msgs = vec![Message::user_only("hi".to_string())];
    assert_eq!(
        save_if_revision(&conn, "with-layout", msgs.clone(), None, None, None, None).expect("save"),
        SaveConversationOutcome::Saved
    );
    assert_eq!(
        save_if_revision(&conn, "legacy", msgs, None, None, None, None).expect("save"),
        SaveConversationOutcome::Saved
    );
    conn.execute(
        &format!("UPDATE {TABLE} SET layout_meta_json = '' WHERE id = 'legacy'"),
        [],
    )
    .expect("blank layout column");

    let cov = layout_coverage_from_conn(&conn).expect("coverage");
    assert_eq!(cov.total, 2);
    assert_eq!(cov.without_layout, 1);
    assert_eq!(cov.legacy_ratio(), Some(0.5));
}

#[test]
fn layout_coverage_defaults_when_table_missing() {
    // 新库 / 从未落盘：表不存在时按全零返回，而不是报错——doctor 不该因为「还没用过」而失败。
    let conn = Connection::open_in_memory().expect("in-memory db");
    let cov = layout_coverage_from_conn(&conn).expect("coverage on db without table");
    assert_eq!(cov, LayoutCoverage::default());
    assert_eq!(cov.legacy_ratio(), None);
}

#[test]
fn layout_coverage_reads_file_read_only_and_counts_whitespace_layout() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "crabmate_layout_cov_{}_{nanos}.sqlite",
        std::process::id()
    ));

    {
        let conn = Connection::open(&path).expect("create db");
        migrate(&conn).expect("migrate");
        let msgs = vec![Message::user_only("hi".to_string())];
        assert_eq!(
            save_if_revision(&conn, "a", msgs.clone(), None, None, None, None).expect("save a"),
            SaveConversationOutcome::Saved
        );
        assert_eq!(
            save_if_revision(&conn, "b", msgs, None, None, None, None).expect("save b"),
            SaveConversationOutcome::Saved
        );
        // 纯换行：Rust `str::trim()` 视为空（读路径走 legacy），统计口径必须与之一致。
        conn.execute(
            &format!("UPDATE {TABLE} SET layout_meta_json = char(10) WHERE id = 'b'"),
            [],
        )
        .expect("blank layout with newline");
    }

    // 走公开入口：只读打开真实文件——`crabmate doctor` 用的就是这条路径。
    let cov = layout_coverage(&path).expect("coverage via path");
    assert_eq!(cov.total, 2);
    assert_eq!(cov.without_layout, 1, "只有换行的 layout 也算「无 layout」");
    assert_eq!(cov.legacy_ratio(), Some(0.5));

    std::fs::remove_file(&path).expect("cleanup");
}

// ── 回填（#1338 E4/S3 前置：把 legacy 行清零）─────────────────────────────

/// dry-run 只统计、不写盘；`--apply` 之后同一批行才被填上。
#[test]
fn backfill_layout_meta_dry_run_then_apply() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");
    let msgs = vec![Message::user_only("hi".to_string())];
    for id in ["a", "b"] {
        assert_eq!(
            save_if_revision(&conn, id, msgs.clone(), None, None, None, None).expect("save"),
            SaveConversationOutcome::Saved
        );
    }
    // 模拟旧库行：该列是 NOT NULL，所以「无 layout」只能是空串或纯空白（口径与 coverage 一致）。
    conn.execute(&format!("UPDATE {TABLE} SET layout_meta_json = ''"), [])
        .expect("blank layout");
    conn.execute(
        &format!("UPDATE {TABLE} SET layout_meta_json = ' ' WHERE id = 'b'"),
        [],
    )
    .expect("blank layout");

    let dry = backfill_layout_meta_in_conn(&conn, false).expect("dry-run");
    assert_eq!(dry.candidates, 2, "空串与纯空白都算候选");
    assert_eq!(dry.filled, 2);
    assert_eq!(dry.skipped_invalid, 0);
    assert!(!dry.applied);
    assert!(
        !dry.ready_for_contract(),
        "dry-run 没写盘，不能说「库里已无不可用行」——那是上一版自证成立的错误"
    );
    assert_eq!(dry.remaining_legacy_rows(), 2, "dry-run 后候选一行都没减");
    // dry-run 真的没写：覆盖率仍是 2 行无 layout。
    assert_eq!(
        layout_coverage_from_conn(&conn)
            .expect("coverage")
            .without_layout,
        2
    );

    let applied = backfill_layout_meta_in_conn(&conn, true).expect("apply");
    assert_eq!(applied.filled, 2);
    assert!(applied.applied);
    assert!(applied.ready_for_contract(), "真写盘且无跳过行，判据才成立");
    assert_eq!(applied.remaining_legacy_rows(), 0);
    assert_eq!(
        layout_coverage_from_conn(&conn)
            .expect("coverage")
            .without_layout,
        0,
        "回填后库内再无 legacy 行"
    );
    // 幂等：再跑一次没有候选。
    let again = backfill_layout_meta_in_conn(&conn, true).expect("re-run");
    assert_eq!(again.candidates, 0);
}

/// `messages_json` 已损坏的行：**跳过且不删**，且不能让 `ready_for_contract()` 变真
/// （否则删读路径会让这些行不可读）。
#[test]
fn backfill_layout_meta_skips_broken_rows_without_deleting() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");
    assert_eq!(
        save_if_revision(
            &conn,
            "ok",
            vec![Message::user_only("hi".to_string())],
            None,
            None,
            None,
            None
        )
        .expect("save"),
        SaveConversationOutcome::Saved
    );
    assert_eq!(
        save_if_revision(
            &conn,
            "broken",
            vec![Message::user_only("x".to_string())],
            None,
            None,
            None,
            None
        )
        .expect("save"),
        SaveConversationOutcome::Saved
    );
    conn.execute(&format!("UPDATE {TABLE} SET layout_meta_json = ''"), [])
        .expect("blank layout");
    conn.execute(
        &format!("UPDATE {TABLE} SET messages_json = '{{not json' WHERE id = 'broken'"),
        [],
    )
    .expect("corrupt messages");

    let report = backfill_layout_meta_in_conn(&conn, true).expect("apply");
    assert_eq!(report.candidates, 2);
    assert_eq!(report.filled, 1);
    assert_eq!(report.skipped_invalid, 1);
    assert!(
        !report.ready_for_contract(),
        "存在解析失败行时不得宣称可以删 legacy 读路径"
    );
    // 坏行仍在（没被删）。
    let left: i64 = conn
        .query_row(
            &format!("SELECT count(*) FROM {TABLE} WHERE id = 'broken'"),
            [],
            |r| r.get(0),
        )
        .expect("count broken");
    assert_eq!(left, 1, "解析失败的行只报告、不删除");
}

/// 路径入口：库不存在时报明确错误（而不是静默 0 行）。
#[test]
fn backfill_layout_meta_path_reports_missing_db() {
    let path = std::env::temp_dir().join(format!(
        "crabmate_backfill_missing_{}_{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let err = backfill_layout_meta(&path, false).expect_err("missing db should fail");
    assert!(err.contains("会话库不存在"), "{err}");
}

/// 路径入口的两条打开分支：**dry-run 只读**（不写盘、覆盖率不变），`--apply` 才写。
#[test]
fn backfill_layout_meta_path_dry_run_is_read_only_then_apply_writes() {
    let path = std::env::temp_dir().join(format!(
        "crabmate_backfill_{}_{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    {
        let conn = Connection::open(&path).expect("create db");
        migrate(&conn).expect("migrate");
        assert_eq!(
            save_if_revision(
                &conn,
                "legacy",
                vec![Message::user_only("hi".to_string())],
                None,
                None,
                None,
                None
            )
            .expect("save"),
            SaveConversationOutcome::Saved
        );
        conn.execute(&format!("UPDATE {TABLE} SET layout_meta_json = ''"), [])
            .expect("blank layout");
    }

    // dry-run：只读打开，数字对得上，且库里仍是 1 行无 layout。
    let dry = backfill_layout_meta(&path, false).expect("dry-run");
    assert_eq!((dry.candidates, dry.filled), (1, 1));
    assert!(!dry.applied);
    assert_eq!(
        layout_coverage(&path).expect("coverage").without_layout,
        1,
        "dry-run 不得写盘"
    );

    // apply：写盘后归零。
    let applied = backfill_layout_meta(&path, true).expect("apply");
    assert!(applied.applied && applied.ready_for_contract());
    assert_eq!(layout_coverage(&path).expect("coverage").without_layout, 0);

    std::fs::remove_file(&path).expect("cleanup");
}

/// v1 与坏 JSON 行同样是「不可用」：旧实现只挑空 / 空白，doctor 因而报「0 无 layout」，
/// 而这两类行在纯删读路径后直接读不出来。回填与覆盖率必须给同一个答案。
#[test]
fn backfill_and_coverage_treat_v1_and_corrupt_as_unusable() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");
    let msgs = vec![Message::user_only("hi".to_string())];
    for id in ["v1", "corrupt", "blank"] {
        assert_eq!(
            save_if_revision(&conn, id, msgs.clone(), None, None, None, None).expect("save"),
            SaveConversationOutcome::Saved
        );
    }
    let v1_layout = r#"{"layout_schema_version":1}"#;
    let corrupt_layout = "{not json";
    conn.execute(
        &format!("UPDATE {TABLE} SET layout_meta_json = ?1 WHERE id = 'v1'"),
        rusqlite::params![v1_layout],
    )
    .expect("v1 layout");
    conn.execute(
        &format!("UPDATE {TABLE} SET layout_meta_json = ?1 WHERE id = 'corrupt'"),
        rusqlite::params![corrupt_layout],
    )
    .expect("corrupt layout");
    conn.execute(
        &format!("UPDATE {TABLE} SET layout_meta_json = '' WHERE id = 'blank'"),
        [],
    )
    .expect("blank layout");

    let cov = layout_coverage_from_conn(&conn).expect("coverage");
    assert_eq!(cov.total, 3);
    assert_eq!(cov.without_layout, 1, "只有空白行算「缺失」");
    assert_eq!(cov.unusable_layout, 2, "v1 与坏 JSON 都算「不可用」");
    assert_eq!(cov.unusable(), 3, "删读路径的判据看的是这个总数");

    let dry = backfill_layout_meta_in_conn(&conn, false).expect("dry-run");
    assert_eq!(dry.total, 3);
    assert_eq!(dry.candidates, 3, "三类都必须进候选，否则「归零」判据失真");
    assert_eq!(dry.filled, 3);

    let applied = backfill_layout_meta_in_conn(&conn, true).expect("apply");
    assert!(applied.ready_for_contract(), "三类都回填后判据成立");
    assert_eq!(
        layout_coverage_from_conn(&conn)
            .expect("coverage after")
            .unusable(),
        0
    );
}
