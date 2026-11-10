//! `layout_coverage`（B4/P4 门禁）的独立测试模块。
//!
//! 单独成文件而不是塞进 `tests.rs`：后者已贴着 `fn-nloc` 的文件行数上限（920 行），
//! 再加用例就会超标——这本身也说明覆盖率统计不属会话 CRUD 的主测试面。

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
