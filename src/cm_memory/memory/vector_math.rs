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

// ---------------------------------------------------------------------------
// f32 切片与 little-endian 字节的互转（`#1247`）
//
// 与上面的 `cosine_sim` 同一原因：`codebase_semantic_index::numeric` 里的版本
// 经 `#[cfg(feature = "fastembed")] pub use` 导出，而 `long_term_memory` 在无
// `fastembed` 的构建下也要编到这段逻辑，于是曾各写一份。提到本中性模块后单一来源。

/// 把 f32 切片按 little-endian 打包成字节（每元素 4 字节）。
///
/// 只被嵌入路径使用，故在无 `fastembed` 的构建里不存在（避免 `dead_code`）。
#[cfg(feature = "fastembed")]
pub fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

/// `f32_slice_to_bytes` 的逆操作；长度不是 4 的整数倍时返回 `None`。
pub fn bytes_to_f32_slice(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    let n = blob.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let chunk = blob.get(i * 4..i * 4 + 4)?;
        let arr: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}
