//! 长期记忆注入前的召回排序：优先可复用「经验」条，辅以向量相似度与标签匹配。

use crate::cm_memory::memory::long_term_memory_store::MemoryRow;

/// 单条待注入记忆（相似度、id、正文、来源角色）。
pub type RecallPick = (f32, i64, String, String);

/// 待排序候选：`(排序分, base, 记忆行)`。
///
/// - 排序分 = `base + boost`（见 [`score_row`]），仅用于排序与最终输出；
/// - base = 未叠加 boost 的余弦/关键词分，仅用于阈值与相对衰减。
///
/// 两者必须分开：boost 是加性的（经验最高 +0.35、标签 +0.2、关键词 ×0.15），
/// 若拿排序分做相对衰减，一条带 boost 的弱相关经验条会把门槛抬到高于更相关的回合条之上。
pub type ScoredRow = (f32, f32, MemoryRow);

/// 向量召回的 base 门槛：**未叠加 boost** 的余弦相似度低于该值即不进候选。
///
/// 必须用 base 判定而不是 `score_row` 的总分——boost 是加性的（经验 +0.25/+0.1、标签 +0.2），
/// 用总分判定时一条与 query 毫无关系的经验条仅靠 boost 就能越过任何门槛。
pub const MIN_RECALL_BASE_SCORE: f32 = 0.30;

/// 相对衰减：低于 `top1 分数 × RELATIVE_SCORE_RATIO` 的候选一律丢弃，剪掉长尾噪声。
pub const RELATIVE_SCORE_RATIO: f32 = 0.55;

/// `prioritize_experience` 时为回合记忆保留的最少槽位，避免经验条独占注入预算。
const MIN_TURN_SLOTS: usize = 2;

const EXPERIENCE_SOURCE_ROLES: &[&str] = &[
    "summarize_experience",
    "auto_summarize_experience",
    "explicit",
];

pub fn is_experience_source_role(source_role: &str) -> bool {
    EXPERIENCE_SOURCE_ROLES.contains(&source_role)
}

/// 是否命中该行任一标签（标签长度 ≥2 且 query 字面包含）。
fn tag_hit_in_query(row: &MemoryRow, query: &str) -> bool {
    let Ok(tags) = serde_json::from_str::<Vec<String>>(&row.tags_json) else {
        return false;
    };
    let ql = query.to_ascii_lowercase();
    tags.iter().any(|tag| {
        let t = tag.trim();
        t.len() >= 2 && ql.contains(&t.to_ascii_lowercase())
    })
}

/// 在向量分（或 0）之上叠加经验优先与标签/关键词加分。
pub fn experience_recall_boost(row: &MemoryRow, query: &str) -> f32 {
    let mut boost = 0.0f32;
    if is_experience_source_role(&row.source_role) {
        boost += 0.25;
        if row.source_role == "auto_summarize_experience"
            || row.source_role == "summarize_experience"
        {
            boost += 0.1;
        }
    }
    if tag_hit_in_query(row, query) {
        boost += 0.2;
    }
    boost + keyword_overlap_score(query, &row.chunk_text) * 0.15
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x30FF      // 日文假名
            | 0x3400..=0x4DBF // 汉字扩展 A
            | 0x4E00..=0x9FFF // 汉字基本区
            | 0xF900..=0xFAFF // 汉字兼容区
            | 0xAC00..=0xD7AF // 韩文音节
    )
}

/// 切分 query：ASCII 片段按空白切分（保留长度 ≥2 的），CJK 片段切成字符 bigram。
///
/// 中文词间无空格，整句 `split_whitespace` 只会得到一个 token 且必然 `contains` 失败，
/// 使关键词召回恒为 0 分；改用 bigram 后「记忆召回」这类查询才能命中正文。
fn query_terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut buf_cjk = false;
    for ch in query.chars() {
        if ch.is_whitespace() {
            flush_terms(&mut buf, buf_cjk, &mut out);
            buf_cjk = false;
            continue;
        }
        let cjk = is_cjk(ch);
        if !buf.is_empty() && cjk != buf_cjk {
            flush_terms(&mut buf, buf_cjk, &mut out);
        }
        buf_cjk = cjk;
        buf.push(ch);
    }
    flush_terms(&mut buf, buf_cjk, &mut out);
    out
}

fn flush_terms(buf: &mut String, is_cjk: bool, out: &mut Vec<String>) {
    if buf.is_empty() {
        return;
    }
    let chars: Vec<char> = buf.chars().collect();
    buf.clear();
    if !is_cjk {
        if chars.len() >= 2 {
            out.push(chars.iter().collect::<String>().to_ascii_lowercase());
        }
        return;
    }
    if chars.len() == 1 {
        out.push(chars[0].to_string());
        return;
    }
    for w in chars.windows(2) {
        out.push(w.iter().collect::<String>());
    }
}

fn keyword_overlap_score(query: &str, text: &str) -> f32 {
    let words = query_terms(query);
    if words.is_empty() {
        return 0.0;
    }
    let tl = text.to_ascii_lowercase();
    let hits = words.iter().filter(|w| tl.contains(w.as_str())).count();
    hits as f32 / words.len() as f32
}

pub fn score_row(base: f32, row: &MemoryRow, query: &str, prioritize_experience: bool) -> f32 {
    if prioritize_experience {
        base + experience_recall_boost(row, query)
    } else {
        base
    }
}

/// 相对衰减：入参需已按排序分降序，返回同样降序的子集。
///
/// 参考点取**全局最大 base**，而非首位（排序分最高）那条的 base——截断只由原始相似度决定，
/// boost 完全不参与；否则带 boost 的经验条会当上参考点，把 bar 压低成几乎空操作。
fn apply_relative_cutoff(scored: Vec<ScoredRow>) -> Vec<ScoredRow> {
    let top1_base = scored.iter().map(|(_, base, _)| *base).fold(0.0f32, f32::max);
    if top1_base <= 0.0 {
        return scored;
    }
    let bar = top1_base * RELATIVE_SCORE_RATIO;
    scored
        .into_iter()
        .filter(|(_, base, _)| *base >= bar)
        .collect()
}

fn into_pick((score, _base, row): ScoredRow) -> RecallPick {
    (score, row.id, row.chunk_text, row.source_role)
}

/// 从候选行中选出 top-k。
///
/// `prioritize_experience` 时经验条优先占位，但为回合记忆保留 `MIN_TURN_SLOTS` 个槽位，
/// 余下槽位由回合按分数回填；经验条不足时回合条可占满剩余槽位。
pub fn pick_recall_chunks(
    top_k: usize,
    _query: &str,
    mut scored: Vec<ScoredRow>,
    prioritize_experience: bool,
) -> Vec<RecallPick> {
    if top_k == 0 || scored.is_empty() {
        return Vec::new();
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let scored = apply_relative_cutoff(scored);

    if !prioritize_experience {
        return scored.into_iter().take(top_k).map(into_pick).collect();
    }

    let (experience, turn_auto): (Vec<_>, Vec<_>) = scored
        .into_iter()
        .partition(|(_, _, r)| is_experience_source_role(&r.source_role));
    let experience_quota = if turn_auto.is_empty() {
        top_k
    } else {
        top_k.saturating_sub(MIN_TURN_SLOTS).max(1)
    };

    let mut out: Vec<RecallPick> = experience
        .into_iter()
        .take(experience_quota)
        .map(into_pick)
        .collect();
    if out.len() < top_k {
        out.extend(turn_auto.into_iter().take(top_k - out.len()).map(into_pick));
    }
    out
}

/// 无向量时对行做关键词初排（供 Disabled 后端与向量失败回退）。
///
/// 关键词与标签都未命中的行不进候选——旧实现会把它们以 0 分塞进 top-k，
/// 等价于「按时间取最新几条」，纯噪声。
pub fn keyword_rank_rows(
    top_k: usize,
    rows: Vec<MemoryRow>,
    query: &str,
    prioritize_experience: bool,
) -> Vec<RecallPick> {
    let scored: Vec<ScoredRow> = rows
        .into_iter()
        .filter_map(|r| {
            let base = keyword_overlap_score(query, &r.chunk_text);
            if base <= 0.0 && !tag_hit_in_query(&r, query) {
                return None;
            }
            Some((score_row(base, &r, query, prioritize_experience), base, r))
        })
        .collect();
    pick_recall_chunks(top_k, query, scored, prioritize_experience)
}

pub fn format_recall_entry(id: i64, source_role: &str, text: &str) -> String {
    let label = match source_role {
        "summarize_experience" | "auto_summarize_experience" => format!("【经验 #{id}】"),
        "explicit" => format!("【显式记忆 #{id}】"),
        _ => format!("[记忆 #{id}]"),
    };
    format!("{label} {text}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, role: &str, text: &str, tags: &str) -> MemoryRow {
        MemoryRow {
            id,
            chunk_text: text.to_string(),
            source_role: role.to_string(),
            created_at_unix: id,
            expires_at_unix: None,
            tags_json: tags.to_string(),
            embedding: None,
        }
    }

    #[test]
    fn keeps_turn_slots_alongside_experience() {
        let mut rows: Vec<MemoryRow> = (1..=10)
            .map(|id| {
                row(
                    id,
                    "summarize_experience",
                    "修复编译错误时先 cargo check 再针对性改",
                    r#"["rust"]"#,
                )
            })
            .collect();
        rows.push(row(101, "user", "auto indexed user question", "[]"));
        rows.push(row(102, "assistant", "auto indexed assistant reply", "[]"));
        let scored: Vec<ScoredRow> = rows.into_iter().map(|r| (0.5f32, 0.5f32, r)).collect();

        let picked = pick_recall_chunks(8, "cargo check 编译", scored, true);
        let roles: Vec<_> = picked.iter().map(|p| p.3.as_str()).collect();
        assert_eq!(picked.len(), 8);
        assert_eq!(
            roles.iter().filter(|r| **r == "summarize_experience").count(),
            6
        );
        assert_eq!(
            roles
                .iter()
                .filter(|r| **r == "user" || **r == "assistant")
                .count(),
            2
        );
    }

    #[test]
    fn experience_fills_top_k_when_no_turn_rows() {
        let rows: Vec<MemoryRow> = (1..=5)
            .map(|id| row(id, "explicit", "显式记忆正文", "[]"))
            .collect();
        let scored: Vec<ScoredRow> = rows.into_iter().map(|r| (0.5f32, 0.5f32, r)).collect();

        let picked = pick_recall_chunks(3, "q", scored, true);
        assert_eq!(picked.len(), 3);
        assert!(picked.iter().all(|p| p.3 == "explicit"));
    }

    #[test]
    fn relative_cutoff_drops_low_score_tail() {
        let scored = vec![
            (0.9f32, 0.9f32, row(1, "explicit", "a", "[]")),
            (0.8f32, 0.8f32, row(2, "explicit", "b", "[]")),
            (0.1f32, 0.1f32, row(3, "explicit", "c", "[]")),
        ];
        let ids: Vec<_> = pick_recall_chunks(8, "q", scored, false)
            .iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn relative_cutoff_uses_base_not_boosted_score() {
        // 经验条被 boost 抬到排序分 1.0，但 base 只有 0.45；回合条 base 更高（0.54）却无 boost。
        // 若按排序分衰减（bar = 1.0 × 0.55 = 0.55）会误删更相关的回合条；
        // 按 base 衰减则 bar = 0.54 × 0.55，两条都保留。
        let scored = vec![
            (
                1.0f32,
                0.45f32,
                row(1, "auto_summarize_experience", "weak exp", r#"["t"]"#),
            ),
            (0.54f32, 0.54f32, row(2, "user", "more relevant turn", "[]")),
        ];
        let ids: Vec<_> = pick_recall_chunks(8, "q", scored, true)
            .iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn cjk_query_matches_via_bigram_terms() {
        assert!(query_terms("怎么优化记忆能力").contains(&"记忆".to_string()));
        assert!(keyword_overlap_score("怎么优化记忆能力", "长期记忆的召回优化") > 0.0);
    }

    #[test]
    fn keyword_path_drops_rows_without_any_hit() {
        let rows = vec![
            row(1, "user", "完全无关的正文内容", "[]"),
            row(2, "user", "关于记忆召回的讨论", "[]"),
        ];
        let ids: Vec<_> = keyword_rank_rows(8, rows, "记忆召回", true)
            .iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn tag_boost_increases_score() {
        let r = row(
            1,
            "explicit",
            "use tokio spawn for async",
            r#"["rust","async"]"#,
        );
        let b = experience_recall_boost(&r, "how to fix async rust");
        assert!(b > 0.4);
    }
}
