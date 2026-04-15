//! 长期记忆显式工具：`long_term_remember` / `long_term_forget` / `long_term_memory_list`。

use serde::Deserialize;

use crate::tools::ToolContext;

#[derive(Debug, Deserialize)]
struct RememberArgs {
    text: String,
    #[serde(default)]
    tags: Vec<String>,
    /// 省略或 0：永不过期（仍受条数上限淘汰）。
    #[serde(default)]
    ttl_secs: u64,
}

#[derive(Debug, Deserialize)]
struct ForgetArgs {
    #[serde(default)]
    memory_id: Option<i64>,
    #[serde(default)]
    memory_text: Option<String>,
    /// 为 true 时仅删除 `explicit` 来源条目。
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

pub fn long_term_remember(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig（长期记忆工具不可用）".to_string();
    };
    let Some(ref rt) = ctx.long_term_memory else {
        return "错误：当前会话未挂载长期记忆运行时（或未启用持久化）".to_string();
    };
    let Some(ref scope) = ctx.long_term_memory_scope_id else {
        return "错误：长期记忆作用域未设置".to_string();
    };
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
    match rt.explicit_remember_blocking(cfg, scope.as_str(), text, &args.tags, ttl) {
        Ok(id) => format!("已写入显式长期记忆 id={id}（tags={:?}）", args.tags),
        Err(e) => format!("错误：{e}"),
    }
}

pub fn long_term_forget(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let Some(ref rt) = ctx.long_term_memory else {
        return "错误：当前会话未挂载长期记忆运行时".to_string();
    };
    let Some(ref scope) = ctx.long_term_memory_scope_id else {
        return "错误：长期记忆作用域未设置".to_string();
    };
    let args: ForgetArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    match rt.explicit_forget_blocking(
        cfg,
        scope.as_str(),
        args.memory_id,
        args.memory_text.as_deref(),
        args.explicit_only,
    ) {
        Ok(n) => format!("已删除 {n} 条记忆"),
        Err(e) => format!("错误：{e}"),
    }
}

pub fn long_term_memory_list(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let Some(ref rt) = ctx.long_term_memory else {
        return "错误：当前会话未挂载长期记忆运行时".to_string();
    };
    let Some(ref scope) = ctx.long_term_memory_scope_id else {
        return "错误：长期记忆作用域未设置".to_string();
    };
    let args: ListArgs = serde_json::from_str(args_json).unwrap_or(ListArgs {
        limit: default_list_limit(),
    });
    match rt.list_recent_blocking(cfg, scope.as_str(), args.limit) {
        Ok(s) => s,
        Err(e) => format!("错误：{e}"),
    }
}

#[derive(Debug, Deserialize)]
struct SummarizeExperienceArgs {
    /// 从本轮对话中提炼的核心经验（由模型生成）；应简洁、通用、可复用。
    experience: String,
    /// 经验分类标签，如 rust、debug、git、performance。
    #[serde(default)]
    tags: Vec<String>,
    /// 过期秒数；省略或 0 表示永不过期（仍受条数上限淘汰）。
    #[serde(default)]
    ttl_secs: u64,
}

/// 将模型从当前回复中提炼的经验写入长期记忆。
///
/// 适用于：解决了一个有价值的问题、发现了一个通用模式、记录了一个重要踩坑。
/// 与 `long_term_remember` 共用写入路径，区别在于调用来源和内容是否经过提炼。
pub fn summarize_experience(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let Some(ref rt) = ctx.long_term_memory else {
        return "错误：当前会话未挂载长期记忆运行时（请确认 long_term_memory_enabled = true）"
            .to_string();
    };
    let Some(ref scope) = ctx.long_term_memory_scope_id else {
        return "错误：长期记忆作用域未设置".to_string();
    };
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
    match rt.explicit_remember_blocking(cfg, scope.as_str(), text, &args.tags, ttl) {
        Ok(id) => format!("已将经验写入长期记忆 id={id}（tags={:?}）", args.tags),
        Err(e) => format!("错误：{e}"),
    }
}
