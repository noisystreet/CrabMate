//! 用户消息中的 **文件引用**：在入队模型前展开为工作区内只读文件摘要（与 `read_file` 同源路径策略）。
//!
//! - 语法（任选）：
//!   - **`file:///{相对路径}`**（Web 输入框双击插入的首选形式）
//!   - **`@{相对路径}`**（兼容手输 / CLI）
//! - 路径内勿含空白；**禁止**绝对路径或以 `/` 开头。
//! - 同一文件多次引用只展开一次；展开块按首次出现顺序追加在全文末尾。
//! - 总展开字符上限约 **512 KiB**，超出部分跳过并附说明。
//! - **安全**：路径经 [`crate::tools::file::read_file_try`] / `resolve_for_read_open`，与工具一致，不扩大工作区外读取。

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;

use crate::tools::{self, ToolContext};
use crabmate_config::AgentConfig;

/// 与单次 `read_file` 默认一致，避免用户消息爆炸。
const EXPAND_MAX_LINES: usize = 500;
/// 所有文件引用展开块合计正文上限（不含说明头尾）。
const EXPAND_TOTAL_MAX_CHARS: usize = 512 * 1024;

fn at_file_path_token_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@([^\s@]+)").expect("at-file path token regex"))
}

fn file_uri_path_token_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // file:///rel/path — 三斜杠后为工作区相对路径（无前导 /）。
    RE.get_or_init(|| Regex::new(r"file:///([^\s]+)").expect("file-uri path token regex"))
}

fn normalize_rel_path_token(raw: &str) -> String {
    raw.trim().replace('\\', "/")
}

fn validate_workspace_rel_token(token: &str, display: &str) -> Result<(), String> {
    if token.is_empty() {
        return Ok(());
    }
    if Path::new(token).is_absolute() {
        return Err(format!(
            "消息中的文件引用须为相对工作区根的相对路径，不能使用绝对路径：`{display}`"
        ));
    }
    if token.starts_with('/') {
        return Err(format!(
            "消息中的文件引用禁止使用以 `/` 开头的路径：`{display}`"
        ));
    }
    Ok(())
}

/// 剥离 `read_file` 工具成功输出首行 JSON 头，仅保留正文（与单测辅助逻辑一致）。
fn strip_read_file_output_header(body: &str) -> &str {
    let Some((first, rest)) = body.split_once('\n') else {
        return body;
    };
    if first.contains("crabmate_tool_output")
        && first.contains("\"tool\":\"read_file\"")
        && !rest.is_empty()
    {
        rest
    } else {
        body
    }
}

fn fenced_block(lang: &str, body: &str) -> String {
    format!("```{lang}\n{body}\n```\n")
}

fn expand_one_path(rel: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> Result<String, String> {
    let args = serde_json::json!({
        "path": rel,
        "max_lines": EXPAND_MAX_LINES,
    })
    .to_string();
    match crate::tools::read_file_try_at_paths(&args, working_dir, ctx) {
        Ok(s) => {
            let body = strip_read_file_output_header(&s);
            Ok(fenced_block("text", body))
        }
        Err(e) => Ok(fenced_block(
            "text",
            format!(
                "（无法读取工作区文件 `{}`：{}）",
                rel.replace('\\', "/"),
                e.message
            )
            .as_str(),
        )),
    }
}

fn collect_ordered_rel_paths(raw: &str) -> Result<Vec<String>, String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();

    let push_token = |token_raw: &str,
                      display: &str,
                      seen: &mut HashSet<String>,
                      ordered: &mut Vec<String>|
     -> Result<(), String> {
        let token = normalize_rel_path_token(token_raw);
        if token.is_empty() {
            return Ok(());
        }
        validate_workspace_rel_token(&token, display)?;
        if seen.insert(token.clone()) {
            ordered.push(token);
        }
        Ok(())
    };

    for cap in file_uri_path_token_re().captures_iter(raw) {
        let Some(m) = cap.get(1) else {
            continue;
        };
        let display = format!("file:///{}", m.as_str());
        push_token(m.as_str(), &display, &mut seen, &mut ordered)?;
    }
    for cap in at_file_path_token_re().captures_iter(raw) {
        let Some(m) = cap.get(1) else {
            continue;
        };
        // 避免把 `file:///x` 里的残余误当成 @（file URI 不含 @）。
        let display = format!("@{}", m.as_str());
        push_token(m.as_str(), &display, &mut seen, &mut ordered)?;
    }
    Ok(ordered)
}

/// 用户正文是否含工作区文件引用语法（`@` 或 `file:///`），用于未设工作区时的前置校验。
#[must_use]
pub fn user_message_has_workspace_file_ref_syntax(raw: &str) -> bool {
    raw.contains('@') || raw.contains("file:///")
}

/// 将用户消息中的 `file:///` / `@相对路径` 展开为文末附加块；无引用时返回原文。
///
/// 失败仅当路径 token 明显非法（如绝对路径）；可读性错误写入展开块正文，不中断发送。
pub fn expand_at_file_refs_in_user_message(
    raw: &str,
    working_dir: &Path,
    cfg: &AgentConfig,
) -> Result<String, String> {
    if !user_message_has_workspace_file_ref_syntax(raw) {
        return Ok(raw.to_string());
    }
    let ordered = collect_ordered_rel_paths(raw)?;
    if ordered.is_empty() {
        return Ok(raw.to_string());
    }

    let allowed: &[String] = &[];
    let ctx = tools::tool_context_for(cfg, allowed, working_dir);

    let mut budget = EXPAND_TOTAL_MAX_CHARS;
    let mut blocks: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for rel in &ordered {
        let block = expand_one_path(rel, working_dir, &ctx)?;
        let body_len = block.chars().count();
        if body_len <= budget {
            budget = budget.saturating_sub(body_len);
            blocks.push(block);
        } else {
            skipped.push(rel.clone());
        }
    }

    let mut out = raw.to_string();
    if !blocks.is_empty() {
        out.push_str(
            "\n\n---\n**工作区文件引用（由 `file:///` / `@路径` 自动展开，与 `read_file` 策略一致）**\n\n",
        );
        for b in blocks {
            out.push_str(&b);
            out.push('\n');
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "\n（以下路径因展开体积上限（约 {} KiB）未嵌入全文，请分开发送或减少文件引用：{}）\n",
            EXPAND_TOTAL_MAX_CHARS / 1024,
            skipped
                .iter()
                .map(|s| format!("`file:///{s}`"))
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn expand_inserts_snippet_for_existing_file() {
        let tmp = tempdir().expect("tempdir");
        let wd = tmp.path();
        fs::write(wd.join("hello.txt"), "line1\nline2\n").expect("write");
        let mut cfg = crabmate_config::load_config(None).expect("embed default");
        cfg.command_exec.run_command_working_dir = wd.to_string_lossy().to_string();

        let out =
            expand_at_file_refs_in_user_message("see @hello.txt please", wd, &cfg).expect("expand");
        assert!(out.contains("see @hello.txt please"));
        assert!(out.contains("工作区文件引用"));
        assert!(out.contains("line1"));
    }

    #[test]
    fn expand_file_uri_token() {
        let tmp = tempdir().expect("tempdir");
        let wd = tmp.path();
        fs::write(wd.join("hello.txt"), "uri-body\n").expect("write");
        let mut cfg = crabmate_config::load_config(None).expect("embed default");
        cfg.command_exec.run_command_working_dir = wd.to_string_lossy().to_string();

        let out = expand_at_file_refs_in_user_message("see file:///hello.txt please", wd, &cfg)
            .expect("expand");
        assert!(out.contains("file:///hello.txt"));
        assert!(out.contains("uri-body"));
    }

    #[test]
    fn rejects_absolute_at_path() {
        let tmp = tempdir().expect("tempdir");
        let mut cfg = crabmate_config::load_config(None).expect("embed default");
        cfg.command_exec.run_command_working_dir = tmp.path().to_string_lossy().to_string();
        let wd = tmp.path();
        let err = expand_at_file_refs_in_user_message("x @/etc/passwd", wd, &cfg).expect_err("abs");
        assert!(err.contains("绝对路径") || err.contains("/"));
    }

    #[test]
    fn rejects_absolute_file_uri_path() {
        let tmp = tempdir().expect("tempdir");
        let mut cfg = crabmate_config::load_config(None).expect("embed default");
        cfg.command_exec.run_command_working_dir = tmp.path().to_string_lossy().to_string();
        let wd = tmp.path();
        let err = expand_at_file_refs_in_user_message("x file:////etc/passwd", wd, &cfg)
            .expect_err("abs uri");
        assert!(err.contains("绝对路径") || err.contains("/"));
    }
}
