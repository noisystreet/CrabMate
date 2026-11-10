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
