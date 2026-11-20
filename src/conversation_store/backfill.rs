//! 会话库 `layout_meta_json` 回填（#1338 E4/S3 前置：把 legacy 行清零）。
//!
//! 单独成文件而不是继续堆在 `conversation_store.rs`：后者已贴着 `fn-nloc` 的文件行数
//! 上限（920 行），而回填是一次性运维动作，与日常会话 CRUD 不同面。
//!
//! 用 [`layout_meta_from_messages`] 从 `messages_json` **重算**（与落盘时同一函数），
//! 所以回填结果与「原本就该写进去的值」同源。

use std::path::Path;

use rusqlite::{Connection, params};

use super::{TABLE, layout_meta_json_for_messages, messages_from_json};

/// `layout_meta_json` 回填结果（#1338 E4/S3 的前置：把 legacy 行清零）。
///
/// 回填只解决「数据侧还有没有 legacy 行」，**不**判断能否删读路径——那是 S3 自己的事。
/// 口径与 [`LayoutCoverage`] 对齐：`layout_meta_json` 为 `NULL` 或只含空白的行都算候选。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutBackfillReport {
    /// 候选行数（`layout_meta_json` 为 `NULL` / 空白）。
    pub candidates: u64,
    /// 成功回填的行数；dry-run 下为「可回填」的行数。
    pub filled: u64,
    /// `messages_json` 解析失败、**跳过且不删行**的条数。
    pub skipped_invalid: u64,
    /// 是否真的写盘（`false` = dry-run）。
    pub applied: bool,
}

impl LayoutBackfillReport {
    /// 是否已经「库内无 legacy 行」（S3 删读路径的前置）。
    ///
    /// 有解析失败行时为 `false`：那些行**没有**被写上 layout，删读路径会让它们不可读。
    #[must_use]
    pub fn ready_for_contract(&self) -> bool {
        self.skipped_invalid == 0 && self.candidates == self.filled
    }
}

/// 回填一个已打开的会话库；`apply = false` 只统计，不写盘。
///
/// 用 `layout_meta_from_messages` 从 `messages_json` **重算**（与落盘时同一函数），
/// 所以回填结果与「原本就该写进去的值」同源。解析失败的行只计数、**不删行**：
/// 那些行的 `messages_json` 本身可能已损坏，删掉等于销毁证据。
pub fn backfill_layout_meta_in_conn(
    conn: &Connection,
    apply: bool,
) -> Result<LayoutBackfillReport, rusqlite::Error> {
    let rows = collect_candidates(conn)?;
    let (filled, skipped_invalid) = if apply {
        apply_rows(conn, &rows)?
    } else {
        dry_run_rows(&rows)
    };
    Ok(LayoutBackfillReport {
        candidates: rows.len() as u64,
        filled,
        skipped_invalid,
        applied: apply,
    })
}

/// `layout_meta_json` 为 `NULL` 或只含空白的行（与 [`super::layout_coverage`] 同一口径）。
fn collect_candidates(conn: &Connection) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, messages_json FROM {TABLE} WHERE layout_meta_json IS NULL OR trim(layout_meta_json) = ''"
    ))?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect::<Result<Vec<_>, _>>()
}

/// 从 `messages_json` 重算 `layout_meta_json`；解析失败返回 `None`（**不删行**：那些行的
/// `messages_json` 本身可能已损坏，删掉等于销毁证据）。
fn build_layout_json(id: &str, messages_json: &str) -> Option<String> {
    let Ok(messages) = messages_from_json(messages_json) else {
        return None;
    };
    Some(layout_meta_json_for_messages(id, &messages))
}

/// 返回 `(filled, skipped_invalid)`。
fn dry_run_rows(rows: &[(String, String)]) -> (u64, u64) {
    let mut filled = 0;
    let mut skipped = 0;
    for (id, json) in rows {
        match build_layout_json(id, json) {
            Some(_) => filled += 1,
            None => skipped += 1,
        }
    }
    (filled, skipped)
}

/// 真写盘：包**一个**事务（几千行逐条自动提交会慢一个量级，中途失败还会留下「一半已回填」的
/// 状态——虽然幂等，但运维时看到半成品数字容易误判）。
fn apply_rows(conn: &Connection, rows: &[(String, String)]) -> Result<(u64, u64), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let mut filled = 0;
    let mut skipped = 0;
    for (id, json) in rows {
        match build_layout_json(id, json) {
            Some(layout_json) => {
                tx.execute(
                    &format!("UPDATE {TABLE} SET layout_meta_json = ?1 WHERE id = ?2"),
                    params![layout_json, id],
                )?;
                filled += 1;
            }
            None => skipped += 1,
        }
    }
    tx.commit()?;
    Ok((filled, skipped))
}

/// 按路径回填（`crabmate backfill-layout` 的实现）。
///
/// **dry-run 用只读连接**（与 [`super::layout_coverage`] 同口径）：既保证「不写盘」是连接级
/// 事实而不是「这段代码恰好没写」，也避免在 `serve` 正在写时去抢写锁。
pub fn backfill_layout_meta(path: &Path, apply: bool) -> Result<LayoutBackfillReport, String> {
    if !path.exists() {
        return Err(format!("会话库不存在：{}", path.display()));
    }
    let flags = if apply {
        rusqlite::OpenFlags::default()
    } else {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI
    };
    let conn = Connection::open_with_flags(path, flags)
        .map_err(|e| format!("打开会话库失败 {}：{e}", path.display()))?;
    backfill_layout_meta_in_conn(&conn, apply).map_err(|e| format!("回填失败：{e}"))
}
