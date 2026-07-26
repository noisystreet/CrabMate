//! fastembed 初始化：缓存目录统一为 [`crabmate_config::ensure_fastembed_cache_dir`]。

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

/// 使用 XDG Cache 下的 `fastembed/` 子目录初始化嵌入模型。
#[cfg(feature = "fastembed")]
pub fn try_new_text_embedding() -> Result<TextEmbedding, String> {
    let cache = crabmate_config::ensure_fastembed_cache_dir()?;
    TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_cache_dir(cache),
    )
    .map_err(|e| format!("fastembed 初始化失败: {e}"))
}
