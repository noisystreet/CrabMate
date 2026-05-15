//! 与后端 `text_sanitize::strip_deepseek_dsml_for_display` 对齐的展示剥离（WASM 无 `regex` 依赖）。
//!
//! 用于 Web 气泡在 **`assistant_text_for_display`** 路径上剥掉 DeepSeek DSML 噪声，避免与 CLI/TUI 已剥内容不一致。

mod tags;

use tags::{DSML_CLOSE_ASCII, DSML_CLOSE_FW, DSML_OPEN_ASCII, DSML_OPEN_FW};

fn normalize_deepseek_dsml_vendor_variants(s: &str) -> String {
    s.replace("<｜｜DSML｜｜", "<｜DSML｜")
        .replace("</｜｜DSML｜｜", "</｜DSML｜")
        .replace("<||DSML||", "<|DSML|")
        .replace("</||DSML||", "</|DSML|")
}

fn collapse_blank_runs(s: &str) -> String {
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

/// 未知标签：扫描开标签后解析 tag 名，再配对同名闭合片段（与后端 `strip_dsml_named_blocks_fullwidth` 同思路）。
fn strip_dsml_named_blocks_fullwidth(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(start) = out.find(DSML_OPEN_FW) else {
            break;
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

fn strip_dsml_named_blocks_ascii(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(start) = out.find(DSML_OPEN_ASCII) else {
            break;
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

/// 移除带属性的成对 DSML 块（全角与 ASCII 两套 open、close 前缀）。
fn strip_tagged_blocks_both_widths(mut s: String, tag: &str) -> String {
    let open_fw = format!("{DSML_OPEN_FW}{tag}");
    let close_fw = format!("{DSML_CLOSE_FW}{tag}>");
    let open_ac = format!("{DSML_OPEN_ASCII}{tag}");
    let close_ac = format!("{DSML_CLOSE_ASCII}{tag}>");
    loop {
        let next = strip_one_delimited_block_family(&s, &open_fw, &close_fw);
        let next = strip_one_delimited_block_family(&next, &open_ac, &close_ac);
        if next == s {
            break;
        }
        s = next;
    }
    s
}

fn strip_one_delimited_block_family(s: &str, open_prefix: &str, close_tag: &str) -> String {
    let mut out = s.to_string();
    loop {
        let Some(start) = out.find(open_prefix) else {
            break;
        };
        let after_open = &out[start + open_prefix.len()..];
        let Some(rel_gt) = after_open.find('>') else {
            let ch = out[start..].chars().next().unwrap_or('\u{fffd}');
            out.replace_range(start..start + ch.len_utf8(), "");
            continue;
        };
        let inner_start = start + open_prefix.len() + rel_gt + 1;
        let tail = &out[inner_start..];
        let Some(rel_close) = tail.find(close_tag) else {
            out.replace_range(start..inner_start, "");
            continue;
        };
        let end = inner_start + rel_close + close_tag.len();
        out.replace_range(start..end, "");
    }
    out
}

fn strip_orphan_open_fw(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let Some(rel) = out[scan..].find(DSML_OPEN_FW) else {
            break;
        };
        let start = scan + rel;
        let after = &out[start + DSML_OPEN_FW.len()..];
        let mut content_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut advance_one = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                advance_one = true;
                break;
            }
            if c == '>' {
                if content_chars <= 300 {
                    remove_to = Some(start + DSML_OPEN_FW.len() + i + c.len_utf8());
                } else {
                    advance_one = true;
                }
                break;
            }
            content_chars += 1;
            if content_chars > 300 {
                advance_one = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if advance_one {
            scan = start + 1;
        } else {
            scan = start + DSML_OPEN_FW.len();
        }
    }
    out
}

fn strip_orphan_close_fw(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let Some(rel) = out[scan..].find(DSML_CLOSE_FW) else {
            break;
        };
        let start = scan + rel;
        let after = &out[start + DSML_CLOSE_FW.len()..];
        let mut inner_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut bump_scan = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                bump_scan = true;
                break;
            }
            if c == '>' {
                if (1..=80).contains(&inner_chars) {
                    remove_to = Some(start + DSML_CLOSE_FW.len() + i + c.len_utf8());
                } else {
                    bump_scan = true;
                }
                break;
            }
            inner_chars += 1;
            if inner_chars > 80 {
                bump_scan = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if bump_scan {
            scan = start + 1;
        } else {
            scan = start + DSML_CLOSE_FW.len();
        }
    }
    out
}

fn strip_orphan_open_ascii(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let Some(rel) = out[scan..].find(DSML_OPEN_ASCII) else {
            break;
        };
        let start = scan + rel;
        let after = &out[start + DSML_OPEN_ASCII.len()..];
        let mut content_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut advance_one = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                advance_one = true;
                break;
            }
            if c == '>' {
                if content_chars <= 300 {
                    remove_to = Some(start + DSML_OPEN_ASCII.len() + i + c.len_utf8());
                } else {
                    advance_one = true;
                }
                break;
            }
            content_chars += 1;
            if content_chars > 300 {
                advance_one = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if advance_one {
            scan = start + 1;
        } else {
            scan = start + DSML_OPEN_ASCII.len();
        }
    }
    out
}

fn strip_orphan_close_ascii(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let Some(rel) = out[scan..].find(DSML_CLOSE_ASCII) else {
            break;
        };
        let start = scan + rel;
        let after = &out[start + DSML_CLOSE_ASCII.len()..];
        let mut inner_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut bump_scan = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                bump_scan = true;
                break;
            }
            if c == '>' {
                if (1..=80).contains(&inner_chars) {
                    remove_to = Some(start + DSML_CLOSE_ASCII.len() + i + c.len_utf8());
                } else {
                    bump_scan = true;
                }
                break;
            }
            inner_chars += 1;
            if inner_chars > 80 {
                bump_scan = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if bump_scan {
            scan = start + 1;
        } else {
            scan = start + DSML_CLOSE_ASCII.len();
        }
    }
    out
}

pub(crate) fn strip_deepseek_dsml_for_display(s: &str) -> String {
    let mut out = normalize_deepseek_dsml_vendor_variants(s);
    if !out.contains("DSML") {
        return out;
    }
    for tag in ["tool_calls", "parameter", "invoke", "function_calls"] {
        out = strip_tagged_blocks_both_widths(out, tag);
    }
    out = strip_dsml_named_blocks_fullwidth(&out);
    out = strip_dsml_named_blocks_ascii(&out);
    out = strip_orphan_open_fw(&out);
    out = strip_orphan_close_fw(&out);
    out = strip_orphan_open_ascii(&out);
    out = strip_orphan_close_ascii(&out);
    collapse_blank_runs(&out)
}
