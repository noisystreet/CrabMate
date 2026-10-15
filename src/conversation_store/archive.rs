//! 上下文原文归档（Archive）：持久化被移出 canonical 的原文区间。
//! 见 docs/design/context_window_compaction.md §3.8 / §4.1。

use rusqlite::{Connection, params};

use super::{ARCHIVE_TABLE, messages_from_json, messages_to_json, now_unix};
use crate::types::Message;

/// 归档区间被移出 canonical 的原因（设计文档 §3.8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveReason {
    /// 被 LLM / 基线摘要覆盖的原文。
    Summarized,
    /// 按 token 预算移出的最旧完整回合组。
    RemovedTurnGroup,
    /// 被回收的旧 `ModelContextArtifact` 旁注（见 §3.9）。
    Artifact,
}

impl ArchiveReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveReason::Summarized => "summarized",
            ArchiveReason::RemovedTurnGroup => "removed_turn_group",
            ArchiveReason::Artifact => "artifact",
        }
    }
}

/// 一条待写入归档的原文区间。
#[derive(Debug, Clone)]
pub struct ArchiveRow {
    pub reason: ArchiveReason,
    /// 写入时刻 canonical 的下标快照（**仅**人审 / 排查，不作回放依据，见 §3.8）。
    pub range_start: i64,
    pub range_end: i64,
    pub messages: Vec<Message>,
}

/// 归档读取 / 解码失败——不得静默丢弃（设计文档 §3.8）。
#[derive(Debug)]
pub enum ArchiveError {
    Sqlite(rusqlite::Error),
    /// 某条归档行无法解码（`seq` + 原因），供调用方诊断。
    Decode { seq: i64, reason: String },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::Sqlite(e) => write!(f, "归档 SQLite 错误: {e}"),
            ArchiveError::Decode { seq, reason } => write!(f, "归档解码失败 seq={seq}: {reason}"),
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArchiveError::Sqlite(e) => Some(e),
            ArchiveError::Decode { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for ArchiveError {
    fn from(e: rusqlite::Error) -> Self {
        ArchiveError::Sqlite(e)
    }
}

/// 归一到 `rusqlite::Error`，供「canonical UPDATE + 归档 INSERT 同事务」路径直接 `?`（见 §4.2）。
impl From<ArchiveError> for rusqlite::Error {
    fn from(e: ArchiveError) -> Self {
        match e {
            ArchiveError::Sqlite(inner) => inner,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        }
    }
}

/// 归档 `codec` 选择阈值默认值（设计文档 §3.8：默认 4 KiB）。
pub const ARCHIVE_CODEC_MIN_BYTES_DEFAULT: usize = 4 * 1024;

/// 按 `codec_min_bytes` 选择编码，返回 `(codec, payload)`。
/// `payload` 为 `TEXT` 列内容：`identity` 直接存 JSON；`gzip` 存 base64(gzip(json))。
fn encode_archive_payload(
    messages: &[Message],
    codec_min_bytes: usize,
) -> Result<(String, String), ArchiveError> {
    let json = messages_to_json(messages).map_err(|e| ArchiveError::Decode {
        seq: -1,
        reason: format!("归档消息序列化失败: {e}"),
    })?;
    if codec_min_bytes > 0 && json.len() >= codec_min_bytes {
        use std::io::Write as _;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(json.as_bytes())
            .map_err(|e| ArchiveError::Decode {
                seq: -1,
                reason: format!("归档 gzip 压缩失败: {e}"),
            })?;
        let bytes = enc.finish().map_err(|e| ArchiveError::Decode {
            seq: -1,
            reason: format!("归档 gzip 收尾失败: {e}"),
        })?;
        use base64::Engine as _;
        let payload = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(("gzip".to_string(), payload))
    } else {
        Ok(("identity".to_string(), json))
    }
}

/// 解码单条归档行，失败返回可诊断错误（不静默丢弃，见 §3.8）。
fn decode_archive_payload(
    seq: i64,
    payload: &str,
    codec: &str,
) -> Result<Vec<Message>, ArchiveError> {
    match codec {
        "identity" => messages_from_json(payload).map_err(|reason| ArchiveError::Decode { seq, reason }),
        "gzip" => {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|e| ArchiveError::Decode {
                    seq,
                    reason: format!("归档 base64 解码失败: {e}"),
                })?;
            use std::io::Read as _;
            let mut dec = flate2::read::GzDecoder::new(bytes.as_slice());
            let mut s = String::new();
            dec.read_to_string(&mut s).map_err(|e| ArchiveError::Decode {
                seq,
                reason: format!("归档 gzip 解压失败: {e}"),
            })?;
            messages_from_json(&s).map_err(|reason| ArchiveError::Decode { seq, reason })
        }
        other => Err(ArchiveError::Decode {
            seq,
            reason: format!("未知归档 codec: {other}"),
        }),
    }
}

/// 追加归档行：`seq` 会话内单调递增，按 `codec_min_bytes` 选择 `codec`（设计文档 §3.8）。
///
/// 若 `conn` 处于 autocommit（无外层事务），本函数自建事务包裹整批插入；
/// 否则复用调用方事务——供 A3「canonical UPDATE + 归档 INSERT 同事务」原子提交（见 §3.8 / §4.2）。
pub fn append_archive_rows(
    conn: &Connection,
    conversation_id: &str,
    rows: &[ArchiveRow],
    codec_min_bytes: usize,
) -> Result<(), ArchiveError> {
    if rows.is_empty() {
        return Ok(());
    }
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction()?;
        insert_archive_rows(&tx, conversation_id, rows, codec_min_bytes)?;
        tx.commit()?;
        Ok(())
    } else {
        insert_archive_rows(conn, conversation_id, rows, codec_min_bytes)
    }
}

/// 在当前连接（含调用方事务）内逐行插入归档行。
fn insert_archive_rows(
    conn: &Connection,
    conversation_id: &str,
    rows: &[ArchiveRow],
    codec_min_bytes: usize,
) -> Result<(), ArchiveError> {
    let base_seq: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1"
        ),
        params![conversation_id],
        |r| r.get(0),
    )?;
    let now = now_unix();
    for (offset, row) in rows.iter().enumerate() {
        let seq = base_seq + offset as i64;
        let (codec, payload) = encode_archive_payload(&row.messages, codec_min_bytes)?;
        conn.execute(
            &format!(
                "INSERT INTO {ARCHIVE_TABLE} (conversation_id, seq, reason, range_start, range_end, messages_json, codec, created_at_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
            ),
            params![
                conversation_id,
                seq,
                row.reason.as_str(),
                row.range_start,
                row.range_end,
                payload,
                codec,
                now
            ],
        )?;
    }
    Ok(())
}

/// 按 `seq` 升序读出会话全部归档原文（re-expand 用，见 §3.11）。
/// 解码失败返回可诊断错误、**不静默丢弃**。
// Phase A 仅被 `archive_real_user_count`（分支序号补偿）使用；完整 re-expand 见 Phase C §5.3.2 C4。
pub fn load_archive_messages(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Vec<Message>, ArchiveError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT seq, messages_json, codec FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1 ORDER BY seq ASC"
    ))?;
    let rows = stmt.query_map(params![conversation_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, payload, codec) = row?;
        out.extend(decode_archive_payload(seq, &payload, &codec)?);
    }
    Ok(out)
}

/// 归档中**真实用户回合**的条数（分支序号补偿用，见 §3.11）。
///
/// 客户端提交的 `before_user_ordinal` 是**会话级**序号（含已归档回合），而
/// [`super::truncate_before_user_ordinal_if_revision`] 按 canonical 内序号执行；两者
/// 相差的正是归档中的真实用户回合数。Phase A **不做 re-expand**，仅用此值做换算 / 下界判断。
pub fn archive_real_user_count(
    conn: &Connection,
    conversation_id: &str,
) -> Result<usize, rusqlite::Error> {
    let messages = load_archive_messages(conn, conversation_id)?;
    Ok(messages
        .iter()
        .filter(|message| crate::types::user_message_counts_for_branch_truncation(message))
        .count())
}

/// 读出**全库**归档原文（跨会话），逐行 decode 后重序列化为 JSON 数组字符串。
/// 供 `list_all_messages_json` 合并归档内容（A4，见 §4.2）。
pub fn list_all_archive_messages_json(conn: &Connection) -> Result<Vec<String>, ArchiveError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT seq, messages_json, codec FROM {ARCHIVE_TABLE} ORDER BY conversation_id ASC, seq ASC"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, payload, codec) = row?;
        let messages = decode_archive_payload(seq, &payload, &codec)?;
        let json = messages_to_json(&messages).map_err(|e| ArchiveError::Decode {
            seq,
            reason: format!("归档重序列化失败: {e}"),
        })?;
        out.push(json);
    }
    Ok(out)
}

/// 删除某会话的全部归档行（会话删除 / re-expand 后清理由调用方在同一事务内完成）。
// Phase C 才会接线（会话删除 / re-expand 清理，见设计文档 §3.11 / §5.3.2 C4）。
#[allow(dead_code)]
pub fn delete_archive_for_conversation(
    conn: &Connection,
    conversation_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        &format!("DELETE FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1"),
        params![conversation_id],
    )?;
    Ok(())
}

/// 归档回收：按 `created_at_unix` 的 TTL 淘汰，并按会话保留最近 `max_rows_per_conversation` 行。
/// `ttl_secs == 0` / `max_rows_per_conversation == 0` 分别表示不启用对应维度（见 §3.8）。
// Phase C 才会接线（归档回收配置，见设计文档 §3.8 / §5.3.2 C4）。
#[allow(dead_code)]
pub fn prune_archive(
    conn: &Connection,
    ttl_secs: u64,
    max_rows_per_conversation: usize,
) -> Result<(), rusqlite::Error> {
    let now = now_unix();
    if ttl_secs > 0 {
        let cutoff = now - ttl_secs as i64;
        conn.execute(
            &format!("DELETE FROM {ARCHIVE_TABLE} WHERE created_at_unix < ?1"),
            params![cutoff],
        )?;
    }
    if max_rows_per_conversation == 0 {
        return Ok(());
    }
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT conversation_id FROM {ARCHIVE_TABLE} GROUP BY conversation_id HAVING COUNT(*) > ?1"
        ))?;
        let rows = stmt.query_map(params![max_rows_per_conversation as i64], |r| {
            r.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for id in ids {
        conn.execute(
            &format!(
                "DELETE FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1 AND seq <= (
                    SELECT seq FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1 ORDER BY seq DESC LIMIT 1 OFFSET ?2
                )"
            ),
            params![id, max_rows_per_conversation as i64],
        )?;
    }
    Ok(())
}
