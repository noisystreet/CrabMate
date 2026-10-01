//! fastembed 初始化：缓存目录统一为 [`crate::cm_config::ensure_fastembed_cache_dir`]。

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

/// 当前嵌入模型的短标签，用于 `EMBEDDING_RECIPE_VERSION` 的模型耦合测试。
#[cfg(feature = "fastembed")]
pub(crate) const EMBEDDING_MODEL_TAG: &str = "minilm";

/// 使用 XDG Cache 下的 `fastembed/` 子目录初始化嵌入模型。
#[cfg(feature = "fastembed")]
pub fn try_new_text_embedding() -> Result<TextEmbedding, String> {
    let cache = crate::cm_config::ensure_fastembed_cache_dir()?;
    let opts = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_cache_dir(cache);
    TextEmbedding::try_new(opts).map_err(|e| format!("fastembed 初始化失败: {e}"))
}

#[cfg(all(test, feature = "fastembed"))]
mod tests {
    use super::*;
    use crate::cm_memory::memory::long_term_memory_store::EMBEDDING_RECIPE_VERSION;

    #[test]
    fn recipe_version_tracks_model_tag() {
        // 换模型时必须同步改 EMBEDDING_RECIPE_VERSION，否则旧向量不会被作废。
        assert!(EMBEDDING_RECIPE_VERSION.starts_with(EMBEDDING_MODEL_TAG));
    }
}
