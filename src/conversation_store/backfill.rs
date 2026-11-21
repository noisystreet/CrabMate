//! 会话库 `layout_meta_json` 回填（#1338 E4/S3 前置：把**不可用**的行清零）。
//!
//! 单独成文件而不是继续堆在 `conversation_store.rs`：后者已贴着 `fn-nloc` 的文件行数
//! 上限（920 行），而回填是一次性运维动作，与日常会话 CRUD 不同面。
//!
//! 候选取舍走 `layout_column` 的唯一判定：**不只** `NULL` / 空白，还包括 `layout_meta_json`
//! 非空但 JSON 损坏、`layout_schema_version < 2` 的行——纯删读路径之后这两类同样读不出来。
//! 用 `layout_meta_from_messages` 从 `messages_json` **重算**（与落盘时同一函数），
//! 所以回填结果与「原本就该写进去的值」同源。

use std::path::Path;

use rusqlite::{Connection, params};

use super::layout_column::classify_layout_column;
use super::{TABLE, layout_meta_json_for_messages, messages_from_json};

/// `layout_meta_json` 回填结果（#1338 E4/S3 的前置：把不可用行清零）。
///
/// 回填只解决「数据侧还有没有不可用行」，**不**判断能否删读路径——那是 S3 后半的事。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutBackfillReport {
    /// 库内会话总行数（判据自证：`total > 0 && candidates == 0` 才是「本来就没有」）。
    pub total: u64,
    /// 需要回填的行数（缺失 / 损坏 / `layout_schema_version < 2`）。
    pub candidates: u64,
    /// 成功回填的行数；dry-run 下为「可回填」的行数。
    pub filled: u64,
    /// `messages_json` 解析失败、**跳过且不删行**的条数。
    pub skipped_invalid: u64,
    /// 是否真的写盘（`false` = dry-run）。
    pub applied: bool,
}

impl LayoutBackfillReport {
    /// 本次动作之后**库里还剩**多少行不可用。
    ///
    /// dry-run 一行都没写，全部候选仍在；`--apply` 之后剩的是「候选里没被填上」的那些
    /// （即解析失败跳过的行——`filled + skipped_invalid == candidates`，不要重复加）。
    #[must_use]
    pub fn remaining_legacy_rows(&self) -> u64 {
        if self.applied {
            self.candidates.saturating_sub(self.filled)
        } else {
            self.candidates
        }
    }

    /// 是否已经「库内无不可用 layout 行」（S3 删读路径的前置）。
    ///
    /// **只有真写盘过**才可能为 `true`：dry-run 说「可以删读路径」与事实相反，是上一版
    /// 判据会自证成立的同款错误。解析失败行也让它为 `false`——那些行没被写上 layout，
    /// 删读路径会让它们不可读。
    #[must_use]
    pub fn ready_for_contract(&self) -> bool {
        self.applied && self.skipped_invalid == 0 && self.candidates == self.filled
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
    let (total, candidate_ids) = collect_candidates(conn)?;
    let (filled, skipped_invalid) = if apply {
        apply_rows(conn, &candidate_ids)?
    } else {
        dry_run_rows(conn, &candidate_ids)?
    };
    Ok(LayoutBackfillReport {
        total,
        candidates: candidate_ids.len() as u64,
        filled,
        skipped_invalid,
        applied: apply,
    })
}

/// 总行数 + 需要回填的 id。
///
/// 只取 `id` / `layout_meta_json` 两列再按需读 `messages_json`：候选通常很少，
/// 没必要把整个库的消息正文一次性读进内存。
fn collect_candidates(conn: &Connection) -> Result<(u64, Vec<String>), rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("SELECT id, layout_meta_json FROM {TABLE}"))?;
    let mut rows = stmt.query([])?;
    let mut total = 0u64;
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        total = total.saturating_add(1);
        let id: String = row.get(0)?;
        let raw: Option<String> = row.get(1)?;
        if classify_layout_column(raw.as_deref().unwrap_or_default()).needs_backfill() {
            ids.push(id);
        }
    }
    Ok((total, ids))
}

/// 取候选行的消息正文。
///
/// **查不到就显式失败**（不静默当成跳过）：候选列表与这次读取之间若被并发的 `serve` prune 掉，
/// 说明库正在变，此刻给出的「归零」结论本身不可信——宁可让命令失败、请人停写后重跑。
fn messages_json_of(conn: &Connection, id: &str) -> Result<String, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT messages_json FROM {TABLE} WHERE id = ?1"),
        params![id],
        |r| r.get(0),
    )
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
fn dry_run_rows(conn: &Connection, ids: &[String]) -> Result<(u64, u64), rusqlite::Error> {
    let mut filled = 0;
    let mut skipped = 0;
    for id in ids {
        match build_layout_json(id, &messages_json_of(conn, id)?) {
            Some(_) => filled += 1,
            None => skipped += 1,
        }
    }
    Ok((filled, skipped))
}

/// 真写盘：包**一个**事务（几千行逐条自动提交会慢一个量级，中途失败还会留下「一半已回填」的
/// 状态——虽然幂等，但运维时看到半成品数字容易误判）。
fn apply_rows(conn: &Connection, ids: &[String]) -> Result<(u64, u64), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let mut filled = 0;
    let mut skipped = 0;
    for id in ids {
        match build_layout_json(id, &messages_json_of(&tx, id)?) {
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
/// **dry-run 用只读连接**（与 `layout_coverage` 同口径）：既保证「不写盘」是连接级
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
    backfill_layout_meta_in_conn(&conn, apply).map_err(|e| {
        format!("回填失败：{e}（若 serve 正在写同一库，请停写后重跑——会变的库给不出「归零」结论）")
    })
}
