use super::tags::{DSML_CLOSE_ASCII, DSML_CLOSE_FW, DSML_OPEN_ASCII, DSML_OPEN_FW};

/// 移除带属性的成对 DSML 块（全角与 ASCII 两套 open、close 前缀）。
pub(super) fn strip_tagged_blocks_both_widths(mut s: String, tag: &str) -> String {
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
    while let Some(start) = out.find(open_prefix) {
        let after_open = &out[start + open_prefix.len()..];
        let rel_gt = match after_open.find('>') {
            Some(r) => r,
            None => {
                let ch = out[start..].chars().next().unwrap_or('\u{fffd}');
                out.replace_range(start..start + ch.len_utf8(), "");
                continue;
            }
        };
        let inner_start = start + open_prefix.len() + rel_gt + 1;
        let tail = &out[inner_start..];
        let rel_close = match tail.find(close_tag) {
            Some(r) => r,
            None => {
                out.replace_range(start..inner_start, "");
                continue;
            }
        };
        let end = inner_start + rel_close + close_tag.len();
        out.replace_range(start..end, "");
    }
    out
}
