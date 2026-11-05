//! `serve` 的存储后端组装：会话库与长期记忆运行时。

use std::sync::Arc;

use log::info;

use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::web;

pub(super) fn conversation_backing_from_sqlite_path(
    conv_sqlite: &str,
) -> Result<crate::cm_session_store::ConversationBacking, std::io::Error> {
    if conv_sqlite.trim().is_empty() {
        return Ok(crate::cm_session_store::ConversationBacking::memory_default());
    }
    let p = std::path::Path::new(conv_sqlite.trim());
    let conn = web::open_conversation_sqlite(p).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("无法初始化会话 SQLite {}: {}", p.display(), e),
        )
    })?;
    info!(
        target: "crabmate",
        "Web 会话持久化已启用 path={}",
        p.display()
    );
    Ok(crate::cm_session_store::ConversationBacking::Sqlite(conn))
}

pub(super) fn serve_long_term_memory_runtime(
    ltm_enabled: bool,
    conversation_backing: &crate::cm_session_store::ConversationBacking,
    ltm_store_path: &str,
) -> Option<Arc<LongTermMemoryRuntime>> {
    if !ltm_enabled {
        return None;
    }
    match conversation_backing {
        crate::cm_session_store::ConversationBacking::Sqlite(conn) => {
            Some(LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(conn)))
        }
        crate::cm_session_store::ConversationBacking::Memory(_) => {
            let p = ltm_store_path.trim();
            if p.is_empty() {
                info!(
                    target: "crabmate",
                    "长期记忆已启用：Web 会话为内存模式且未配置 long_term_memory_store_sqlite_path，跳过持久化记忆"
                );
                None
            } else {
                match LongTermMemoryRuntime::open(std::path::Path::new(p)) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        log::warn!(
                            target: "crabmate",
                            "长期记忆库打开失败 path={} error={}",
                            p,
                            e
                        );
                        None
                    }
                }
            }
        }
    }
}
