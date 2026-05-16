// 按标签名配对剥除 DSML 片段。独立成文件并避免 `let … else`：部分静态分析器（lizard）在相邻正则字面量场景下会误合并函数体 nloc。

// 用 `concat!` 拼出含 `</` 与 `<|` 的片段，避免 lizard 等工具误把后续 `fn` 并入同一函数体。
pub(super) const DSML_OPEN_FW: &str = concat!("<", "\u{ff5c}", "DSML", "\u{ff5c}");
pub(super) const DSML_CLOSE_FW: &str = concat!("</", "\u{ff5c}", "DSML", "\u{ff5c}");
pub(super) const DSML_OPEN_ASCII: &str = concat!("<|", "DSML|");
pub(super) const DSML_CLOSE_ASCII: &str = concat!("</", "|DSML|");

/// `regex` crate 不支持反向引用；未知标签通过扫描配对「全角 DSML 闭标签 + 标签名」移除。
pub(super) fn strip_dsml_named_blocks_fullwidth(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(start) = out.find(DSML_OPEN_FW) {
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
    while let Some(start) = out.find(DSML_OPEN_ASCII) {
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

pub(super) fn collapse_blank_runs(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    lines
        .split(|line| line.is_empty())
        .map(|g| g.join("\n"))
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}
