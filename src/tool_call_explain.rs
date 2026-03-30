//! 工具调用「解释卡」：对非只读工具要求 JSON 参数中带一句人话目的（`crabmate_explain_why`），与审批互补。
//! 校验通过后从参数中剥离该字段再交给各 runner，避免 `additionalProperties: false` 报错。

use std::borrow::Cow;

use crate::config::AgentConfig;
use crate::tool_registry::is_readonly_tool;
use crate::types::Tool;

/// 与发给模型的工具说明一致；须为 JSON 字符串字段。
pub const EXPLAIN_WHY_KEY: &str = "crabmate_explain_why";

const EXPLAIN_APPEND_ZH: &str = concat!(
    "\n\n【解释卡】本工具可能修改工作区或执行命令。",
    "当服务端启用 tool_call_explain_enabled 时，",
    "调用时须在 JSON 顶层附带非空字符串字段 `",
    "crabmate_explain_why",
    "`，用一句自然语言说明本步目的（与审批互补）。"
);

/// 为可能产生副作用的工具描述追加解释卡说明（仅当配置启用）。
pub fn annotate_tool_defs_for_explain_card(tools: &mut [Tool], cfg: &AgentConfig) {
    if !cfg.tool_call_explain_enabled {
        return;
    }
    for t in tools.iter_mut() {
        let name = t.function.name.as_str();
        if is_readonly_tool(cfg, name) || crate::mcp::is_mcp_proxy_tool(name) {
            continue;
        }
        if !t.function.description.contains("crabmate_explain_why") {
            t.function.description.push_str(EXPLAIN_APPEND_ZH);
        }
    }
}

/// 未启用解释卡、或目标为只读工具：`Borrowed(args)`。
/// 否则解析 JSON、校验非空与人话长度，并返回**已去掉** `crabmate_explain_why` 的 `Owned` JSON 字符串。
pub fn require_explain_for_mutation<'a>(
    cfg: &AgentConfig,
    tool_name: &str,
    args: &'a str,
) -> Result<Cow<'a, str>, String> {
    if !cfg.tool_call_explain_enabled
        || is_readonly_tool(cfg, tool_name)
        || crate::mcp::is_mcp_proxy_tool(tool_name)
    {
        return Ok(Cow::Borrowed(args));
    }
    let (cleaned, explain) = strip_explain_why(args)?;
    let Some(e) = explain.filter(|s| !s.is_empty()) else {
        return Err(format!(
            "错误：已启用工具调用解释卡（tool_call_explain_enabled）。调用非只读工具「{tool_name}」时须在 JSON 顶层提供非空字符串字段 `{EXPLAIN_WHY_KEY}`，用一句自然语言说明本步目的（与命令/写操作审批互补，侧重可理解性）。"
        ));
    };
    let n = e.chars().count();
    if n < cfg.tool_call_explain_min_chars {
        return Err(format!(
            "错误：字段 `{EXPLAIN_WHY_KEY}` 过短（至少 {} 个字符，当前 {n}）。请写清本步意图。",
            cfg.tool_call_explain_min_chars
        ));
    }
    if n > cfg.tool_call_explain_max_chars {
        return Err(format!(
            "错误：字段 `{EXPLAIN_WHY_KEY}` 过长（最多 {} 个字符）。请压缩为一句摘要。",
            cfg.tool_call_explain_max_chars
        ));
    }
    Ok(Cow::Owned(cleaned))
}

/// 若参数为 JSON 对象且含 `crabmate_explain_why`，则去掉该键（供 MCP 等路径：不要求解释但避免把多余键传给远端）。
pub(crate) fn strip_explain_why_if_present(args: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(args.trim()) else {
        return args.to_string();
    };
    let Some(obj) = v.as_object_mut() else {
        return args.to_string();
    };
    if obj.remove(EXPLAIN_WHY_KEY).is_none() {
        return args.to_string();
    }
    serde_json::to_string(&v).unwrap_or_else(|_| args.to_string())
}

fn strip_explain_why(args: &str) -> Result<(String, Option<String>), String> {
    let v: serde_json::Value = serde_json::from_str(args.trim()).map_err(|_| {
        "错误：启用解释卡时，工具参数须为合法 JSON 对象，且含字符串字段 `crabmate_explain_why`。".to_string()
    })?;
    let obj = v.as_object().ok_or_else(|| {
        "错误：启用解释卡时，工具参数须为 JSON 对象（可含其它工具字段）。".to_string()
    })?;
    let explain = obj
        .get(EXPLAIN_WHY_KEY)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string());
    let mut obj2 = obj.clone();
    obj2.remove(EXPLAIN_WHY_KEY);
    let cleaned = serde_json::Value::Object(obj2);
    let cleaned_str =
        serde_json::to_string(&cleaned).map_err(|e| format!("错误：重组工具参数失败：{}", e))?;
    Ok((cleaned_str, explain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_on() -> crate::config::AgentConfig {
        let mut c = crate::config::load_config(None).expect("embedded default config");
        c.tool_call_explain_enabled = true;
        c.tool_call_explain_min_chars = 4;
        c.tool_call_explain_max_chars = 200;
        c
    }

    #[test]
    fn readonly_tool_skips_check() {
        let cfg = cfg_on();
        let args = r#"{"path":"a.txt"}"#;
        let got = require_explain_for_mutation(&cfg, "read_file", args).expect("ok");
        assert_eq!(got.as_ref(), args);
    }

    #[test]
    fn mutation_requires_explain() {
        let cfg = cfg_on();
        let err =
            require_explain_for_mutation(&cfg, "create_file", r#"{"path":"x","content":"y"}"#)
                .expect_err("need explain");
        assert!(err.contains("crabmate_explain_why"), "{}", err);
    }

    #[test]
    fn mutation_strips_explain() {
        let cfg = cfg_on();
        let args = r#"{"path":"x","content":"y","crabmate_explain_why":"创建占位文件用于测试"}"#;
        let got = require_explain_for_mutation(&cfg, "create_file", args).expect("ok");
        assert!(!got.contains("crabmate_explain_why"));
        let v: serde_json::Value = serde_json::from_str(got.as_ref()).unwrap();
        assert_eq!(v.get("path").and_then(|x| x.as_str()), Some("x"));
    }
}
