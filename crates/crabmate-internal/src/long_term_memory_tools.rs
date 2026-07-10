//! 长期记忆显式工具：`long_term_remember` / `long_term_forget` / `long_term_memory_list`。

use std::sync::Arc;

use crabmate_config::AgentConfig;
use serde::Deserialize;

use crate::memory::long_term_memory::LongTermMemoryRuntime;

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

pub fn long_term_remember(args_json: &str, st: &LongTermMemoryToolState<'_>) -> String {
    let args: RememberArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    let text = args.text.trim();
    if text.is_empty() {
        return "错误：text 不能为空".to_string();
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
        Ok(id) => format!("已写入显式长期记忆 id={id}（tags={:?}）", args.tags),
        Err(e) => format!("错误：{e}"),
    }
}

pub fn long_term_forget(args_json: &str, st: &LongTermMemoryToolState<'_>) -> String {
    let args: ForgetArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    match st.rt.explicit_forget_blocking(
        st.cfg,
        st.scope,
        args.memory_id,
        args.memory_text.as_deref(),
        args.explicit_only,
    ) {
        Ok(n) => format!("已删除 {n} 条记忆"),
        Err(e) => format!("错误：{e}"),
    }
}

pub fn long_term_memory_list(args_json: &str, st: &LongTermMemoryToolState<'_>) -> String {
    let args: ListArgs = serde_json::from_str(args_json).unwrap_or(ListArgs {
        limit: default_list_limit(),
    });
    match st.rt.list_recent_blocking(st.cfg, st.scope, args.limit) {
        Ok(s) => s,
        Err(e) => format!("错误：{e}"),
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

pub fn summarize_experience(args_json: &str, st: &LongTermMemoryToolState<'_>) -> String {
    let args: SummarizeExperienceArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    let text = args.experience.trim();
    if text.is_empty() {
        return "错误：experience 不能为空".to_string();
    }
    if text.chars().count() < 20 {
        return "错误：经验内容过短（需至少 20 字符），无保存价值".to_string();
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
        Ok(id) => format!("已将经验写入长期记忆 id={id}（tags={:?}）", args.tags),
        Err(e) => format!("错误：{e}"),
    }
}

/// 供 [`crate::memory_tool_hosts::LongTermMemoryHost`] 持有。
pub struct LongTermMemoryHostInner {
    pub rt: Arc<LongTermMemoryRuntime>,
    pub scope_id: String,
}
