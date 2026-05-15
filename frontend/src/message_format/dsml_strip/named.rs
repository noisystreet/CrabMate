use super::tags::{DSML_CLOSE_ASCII, DSML_CLOSE_FW, DSML_OPEN_ASCII, DSML_OPEN_FW};

/// 未知标签：扫描开标签后解析 tag 名，再配对同名闭合片段（与后端 `strip_dsml_named_blocks_fullwidth` 同思路）。
pub(super) fn strip_dsml_named_blocks_fullwidth(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let start = match out.find(DSML_OPEN_FW) {
            Some(s) => s,
            None => break,
        };
        let rest = &out[start + DSML_OPEN_FW.len()..];
        let tag_end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len().min(64));
        let tag = rest.get(..tag_end).unwrap_or("");
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.replace_range(start..start.saturating_add(1), "");
            continue;
        }
        let close = format!("{DSML_CLOSE_FW}{tag}>");
        if let Some(rel) = out[start..].find(&close) {
            let end = start + rel + close.len();
            out.replace_range(start..end, "");
        } else if let Some(rel) = out[start..].find('>') {
            let end = start + rel + 1;
            out.replace_range(start..end, "");
        } else {
            out.replace_range(start..start.saturating_add(1), "");
        }
    }
    out
}

pub(super) fn strip_dsml_named_blocks_ascii(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let start = match out.find(DSML_OPEN_ASCII) {
            Some(s) => s,
            None => break,
        };
        let rest = &out[start + DSML_OPEN_ASCII.len()..];
        let tag_end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len().min(64));
        let tag = rest.get(..tag_end).unwrap_or("");
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.replace_range(start..start.saturating_add(1), "");
            continue;
        }
        let close = format!("{DSML_CLOSE_ASCII}{tag}>");
        if let Some(rel) = out[start..].find(&close) {
            let end = start + rel + close.len();
            out.replace_range(start..end, "");
        } else if let Some(rel) = out[start..].find('>') {
            let end = start + rel + 1;
            out.replace_range(start..end, "");
        } else {
            out.replace_range(start..start.saturating_add(1), "");
        }
    }
    out
}
