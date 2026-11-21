//! 会话库 `layout` 覆盖率（B4/P4 门禁 → #1338 S3 删读路径的判据）。
//!
//! 从 `conversation_store.rs` 拆出来：那边贴着 `fn-nloc` 的文件行数上限，而这份统计与
//! 日常会话 CRUD 不同面。判定本身只在 `layout_column`（唯一口径），这里只做聚合。
//!
//! **口径变更（#1338）**：原来只数「`layout_meta_json` 为 NULL 或空白」的行。于是
//! *非空但 JSON 损坏*、*`layout_schema_version < 2`* 两类行既不算 legacy、也不算
//! 「有 layout」——doctor 报「0 无 layout」，而纯删读路径之后这些会话直接读不出来。
//! 现在三类不可用一起数：[`LayoutCoverage::without_layout`]（缺失）+
//! [`LayoutCoverage::unusable_layout`]（损坏 / v1），判据看 [`LayoutCoverage::unusable`]。

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::TABLE;
use super::layout_column::{LayoutColumnState, classify_layout_column};

/// `layout` 列的三类计数。
///
/// 判定只在这里聚合一次：`crabmate doctor` 直接读它，回填报告另走 `layout_column`，
/// 两处共用同一份判定函数——这正是上一版口径分裂（三处各写一遍）要堵住的点。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutCoverage {
    /// 会话总行数。
    pub total: u64,
    /// `layout_meta_json` 为空（`NULL` / 只含空白）的行数——从未写过布局。
    pub without_layout: u64,
    /// 非空但不可用（JSON 损坏 / `layout_schema_version < 2`）的行数。
    pub unusable_layout: u64,
}

impl LayoutCoverage {
    /// 「无 layout」行占比；`total == 0` 时为 `None`（空库没有「占比」可言，不要报 `0%` 误导）。
    #[must_use]
    pub fn legacy_ratio(&self) -> Option<f64> {
        (self.total > 0).then(|| self.without_layout as f64 / self.total as f64)
    }

    /// 全部不可用行（缺失 + 损坏 / v1）。**删读路径的判据是它为 0。**
    #[must_use]
    pub fn unusable(&self) -> u64 {
        self.without_layout.saturating_add(self.unusable_layout)
    }
}

/// 只读统计会话库的 `layout` 覆盖率：**不**建表、不迁移、不写盘。
///
/// 表不存在（新库 / 从未落盘过会话）按全零返回而不是报错——诊断命令不该因为「还没用过」而失败。
///
/// `SQLITE_OPEN_URI` 只是重申 rusqlite 的 `OpenFlags::default()`；路径来自本地配置，
/// 若含 `?` / `#` 会被当成 URI 参数——那是既有默认行为，不在这里另立规矩。
pub fn layout_coverage(path: &Path) -> Result<LayoutCoverage, rusqlite::Error> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    layout_coverage_from_conn(&conn)
}

/// 逐行判定 `layout_meta_json`。
///
/// 用「全表扫描 + Rust 判定」而不是三条 SQL `count(...)`：判定必须与读路径同源
/// （`serde_json`），SQL 侧的 `json_valid` / `trim` 都只能近似，正是上一版口径分裂的来源。
///
/// 规模前提：行数受 `conversation_store_max_entries`（默认 512）与 TTL 约束，全表 + 每行完整
/// 反序列化在这个量级上可接受；把两项都设成 0（不限）的部署会让 `doctor` 变慢——那是刻意的取舍，
/// 换「口径与读路径一致」，不是漏优化。
pub(super) fn layout_coverage_from_conn(
    conn: &Connection,
) -> Result<LayoutCoverage, rusqlite::Error> {
    let has_table = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![TABLE],
            |_r| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_table {
        return Ok(LayoutCoverage::default());
    }
    let mut stmt = conn.prepare(&format!("SELECT layout_meta_json FROM {TABLE}"))?;
    let mut rows = stmt.query([])?;
    let mut cov = LayoutCoverage::default();
    while let Some(row) = rows.next()? {
        // 列声明为 NOT NULL，但旧库 / 手工数据仍可能出现 NULL：与空白同义。
        let raw: Option<String> = row.get(0)?;
        cov.total = cov.total.saturating_add(1);
        match classify_layout_column(raw.as_deref().unwrap_or_default()) {
            LayoutColumnState::Missing => cov.without_layout = cov.without_layout.saturating_add(1),
            LayoutColumnState::Corrupt | LayoutColumnState::NotV2 { .. } => {
                cov.unusable_layout = cov.unusable_layout.saturating_add(1);
            }
            LayoutColumnState::Usable(_) => {}
        }
    }
    Ok(cov)
}
