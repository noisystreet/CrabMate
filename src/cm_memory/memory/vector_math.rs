//! 向量相似度（`long_term_memory` 与 `codebase_semantic_index` 共用）。
//!
//! #1239：此前两处各有一份字节级相同的 `cosine_sim`——`long_term_memory` 持私有一份，
//! `codebase_semantic_index::numeric` 持 `pub` 一份。后者整个模块受 `fastembed` feature
//! 门控，而前者的召回打分在**无** `fastembed` 的构建下也要编译，于是只能各写一份。
//! 把实现提到这个**不带 feature 门控**的中性模块，两条路径都能用。

/// 余弦相似度；长度不等或含零向量时返回 `0.0`。
pub fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let d = na.sqrt() * nb.sqrt();
    if d <= f32::EPSILON { 0.0 } else { dot / d }
}
