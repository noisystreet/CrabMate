//! 长期记忆注入前的召回排序：优先可复用「经验」条，辅以向量相似度与标签匹配。

use crate::memory::long_term_memory_store::MemoryRow;

/// 单条待注入记忆（相似度、id、正文、来源角色）。
pub(crate) type RecallPick = (f32, i64, String, String);

const EXPERIENCE_SOURCE_ROLES: &[&str] = &[
    "summarize_experience",
    "auto_summarize_experience",
    "explicit",
];

pub(crate) fn is_experience_source_role(source_role: &str) -> bool {
    EXPERIENCE_SOURCE_ROLES.contains(&source_role)
}

/// 在向量分（或 0）之上叠加经验优先与标签/关键词加分。
pub(crate) fn experience_recall_boost(row: &MemoryRow, query: &str) -> f32 {
    let mut boost = 0.0f32;
    if is_experience_source_role(&row.source_role) {
        boost += 0.25;
        if row.source_role == "auto_summarize_experience"
            || row.source_role == "summarize_experience"
        {
            boost += 0.1;
        }
    }
    if let Ok(tags) = serde_json::from_str::<Vec<String>>(&row.tags_json) {
        let ql = query.to_ascii_lowercase();
        for tag in tags {
            let t = tag.trim();
            if t.len() >= 2 && ql.contains(&t.to_ascii_lowercase()) {
                boost += 0.2;
                break;
            }
        }
    }
    boost + keyword_overlap_score(query, &row.chunk_text) * 0.15
}

fn keyword_overlap_score(query: &str, text: &str) -> f32 {
    let words: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.chars().count() >= 2)
        .map(|w| w.to_ascii_lowercase())
        .collect();
    if words.is_empty() {
        return 0.0;
    }
    let tl = text.to_ascii_lowercase();
    let hits = words.iter().filter(|w| tl.contains(w.as_str())).count();
    hits as f32 / words.len() as f32
}

pub(crate) fn score_row(
    base: f32,
    row: &MemoryRow,
    query: &str,
    prioritize_experience: bool,
) -> f32 {
    if prioritize_experience {
        base + experience_recall_boost(row, query)
    } else {
        base
    }
}

/// 从候选行中选出 top-k；`prioritize_experience` 时预留经验槽位，减少 auto 回合全文挤占。
pub(crate) fn pick_recall_chunks(
    top_k: usize,
    _query: &str,
    mut scored: Vec<(f32, MemoryRow)>,
    prioritize_experience: bool,
) -> Vec<RecallPick> {
    if top_k == 0 || scored.is_empty() {
        return Vec::new();
    }

    if !prioritize_experience {
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        return scored
            .into_iter()
            .take(top_k)
            .map(|(s, r)| (s, r.id, r.chunk_text, r.source_role))
            .collect();
    }

    let mut experience: Vec<(f32, MemoryRow)> = Vec::new();
    let mut turn_auto: Vec<(f32, MemoryRow)> = Vec::new();
    for (s, r) in scored {
        if is_experience_source_role(&r.source_role) {
            experience.push((s, r));
        } else {
            turn_auto.push((s, r));
        }
    }
    let sort_desc = |a: &mut Vec<(f32, MemoryRow)>| {
        a.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    };
    sort_desc(&mut experience);
    sort_desc(&mut turn_auto);

    let mut out: Vec<RecallPick> = Vec::with_capacity(top_k);
    if !experience.is_empty() {
        for (s, r) in experience.into_iter().take(top_k) {
            out.push((s, r.id, r.chunk_text, r.source_role));
        }
        return out;
    }
    for (s, r) in turn_auto.into_iter().take(top_k) {
        out.push((s, r.id, r.chunk_text, r.source_role));
    }
    out
}

/// 无向量时对行做关键词初排（供 Disabled 后端与向量失败回退）。
pub(crate) fn keyword_rank_rows(
    top_k: usize,
    rows: Vec<MemoryRow>,
    query: &str,
    prioritize_experience: bool,
) -> Vec<RecallPick> {
    let scored: Vec<(f32, MemoryRow)> = rows
        .into_iter()
        .map(|r| {
            let base = keyword_overlap_score(query, &r.chunk_text);
            (score_row(base, &r, query, prioritize_experience), r)
        })
        .collect();
    pick_recall_chunks(top_k, query, scored, prioritize_experience)
}

pub(crate) fn format_recall_entry(id: i64, source_role: &str, text: &str) -> String {
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
    fn reserves_experience_slots_over_auto_turn() {
        let rows = vec![
            row(
                1,
                "assistant",
                "auto indexed assistant reply from turn",
                "[]",
            ),
            row(2, "user", "auto indexed user question from turn", "[]"),
            row(
                3,
                "auto_summarize_experience",
                "【经验·自动】cargo check 修复未使用导入",
                r#"["auto","rust"]"#,
            ),
            row(
                4,
                "summarize_experience",
                "修复编译错误时先 cargo check 再针对性改",
                r#"["rust"]"#,
            ),
        ];
        let scored: Vec<(f32, MemoryRow)> = rows.into_iter().map(|r| (0.1f32, r)).collect();
        let picked = pick_recall_chunks(4, "cargo check 编译", scored, true);
        let roles: Vec<_> = picked.iter().map(|p| p.3.as_str()).collect();
        assert!(roles.contains(&"auto_summarize_experience"));
        assert!(roles.contains(&"summarize_experience"));
        assert_eq!(picked.len(), 2);
        assert!(!roles.contains(&"assistant"));
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
