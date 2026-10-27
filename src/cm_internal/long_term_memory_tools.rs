//! 长期记忆显式工具：`long_term_remember` / `long_term_forget` / `long_term_memory_list`。

use std::sync::Arc;

use crate::cm_config::AgentConfig;
use serde::Deserialize;

use crate::cm_internal::memory::long_term_memory::LongTermMemoryRuntime;
use crate::cm_tools::tool_result::ToolError;

pub struct LongTermMemoryToolState<'a> {
    pub cfg: &'a AgentConfig,
    pub rt: &'a Arc<LongTermMemoryRuntime>,
    pub scope: &'a str,
}

#[derive(Debug, Deserialize)]
struct RememberArgs {
    text: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    ttl_secs: u64,
}

#[derive(Debug, Deserialize)]
struct ForgetArgs {
    #[serde(default)]
    memory_id: Option<i64>,
    #[serde(default)]
    memory_text: Option<String>,
    #[serde(default)]
    explicit_only: bool,
}

#[derive(Debug, Deserialize)]
struct ListArgs {
    #[serde(default = "default_list_limit")]
    limit: usize,
}

fn default_list_limit() -> usize {
    16
}

/// 参数 JSON 解析失败统一走 `invalid_args`（正文沿用原文案，便于模型自我纠正）。
fn invalid_json(e: serde_json::Error) -> ToolError {
    ToolError::invalid_args(format!("参数 JSON 无效: {e}"))
}

/// 记忆库 IO / SQLite 失败：`External` 类，`error_code` 统一为 `long_term_memory_io_failed`。
fn memory_io_failed(e: impl std::fmt::Display) -> ToolError {
    ToolError::external_code("long_term_memory_io_failed", format!("错误：{e}"))
}

#[allow(clippy::result_large_err)]
pub fn long_term_remember(
    args_json: &str,
    st: &LongTermMemoryToolState<'_>,
) -> Result<String, ToolError> {
    let args: RememberArgs =
        serde_json::from_str(args_json).map_err(invalid_json)?;
    let text = args.text.trim();
    if text.is_empty() {
        return Err(ToolError::invalid_args("错误：text 不能为空".to_string()));
    }
    let ttl = if args.ttl_secs == 0 {
        None
    } else {
        Some(args.ttl_secs)
    };
    match st
        .rt
        .explicit_remember_blocking(st.cfg, st.scope, text, &args.tags, ttl)
    {
        Ok(id) => Ok(format!("已写入显式长期记忆 id={id}（tags={:?}）", args.tags)),
        Err(e) => Err(memory_io_failed(e)),
    }
}

#[allow(clippy::result_large_err)]
pub fn long_term_forget(
    args_json: &str,
    st: &LongTermMemoryToolState<'_>,
) -> Result<String, ToolError> {
    let args: ForgetArgs =
        serde_json::from_str(args_json).map_err(invalid_json)?;
    match st.rt.explicit_forget_blocking(
        st.cfg,
        st.scope,
        args.memory_id,
        args.memory_text.as_deref(),
        args.explicit_only,
    ) {
        Ok(n) => Ok(format!("已删除 {n} 条记忆")),
        Err(e) => Err(memory_io_failed(e)),
    }
}

/// `list` 的 `limit` 反序列化失败时回退默认值（历史行为，保持不报错）。
#[allow(clippy::result_large_err)]
pub fn long_term_memory_list(
    args_json: &str,
    st: &LongTermMemoryToolState<'_>,
) -> Result<String, ToolError> {
    let args: ListArgs = serde_json::from_str(args_json).unwrap_or(ListArgs {
        limit: default_list_limit(),
    });
    match st.rt.list_recent_blocking(st.cfg, st.scope, args.limit) {
        Ok(s) => Ok(s),
        Err(e) => Err(memory_io_failed(e)),
    }
}

#[derive(Debug, Deserialize)]
struct SummarizeExperienceArgs {
    experience: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    ttl_secs: u64,
}

#[allow(clippy::result_large_err)]
pub fn summarize_experience(
    args_json: &str,
    st: &LongTermMemoryToolState<'_>,
) -> Result<String, ToolError> {
    let args: SummarizeExperienceArgs =
        serde_json::from_str(args_json).map_err(invalid_json)?;
    let text = args.experience.trim();
    if text.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：experience 不能为空".to_string(),
        ));
    }
    if text.chars().count() < 20 {
        return Err(ToolError::invalid_args(
            "错误：经验内容过短（需至少 20 字符），无保存价值".to_string(),
        ));
    }
    let ttl = if args.ttl_secs == 0 {
        None
    } else {
        Some(args.ttl_secs)
    };
    match st.rt.summarize_experience_remember_blocking(
        st.cfg,
        st.scope,
        text,
        &args.tags,
        ttl,
        "summarize_experience",
    ) {
        Ok(id) => Ok(format!("已将经验写入长期记忆 id={id}（tags={:?}）", args.tags)),
        Err(e) => Err(memory_io_failed(e)),
    }
}

/// 供 [`crate::cm_internal::memory_tool_hosts::LongTermMemoryHost`] 持有。
pub struct LongTermMemoryHostInner {
    pub rt: Arc<LongTermMemoryRuntime>,
    pub scope_id: String,
}
