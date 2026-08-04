//! 用户消息：将 `file:///{rel}` / `file://./{rel}` / `@{rel}` 切成可展示的文件引用段（链接文字仅显示相对路径）。

/// 一段用户正文：普通文字或文件引用 token。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserTextSeg {
    Plain(String),
    FileRef(String),
}

fn take_path_token_len(rest: &str) -> usize {
    rest.chars()
        .take_while(|c| !c.is_whitespace() && *c != '@')
        .map(|c| c.len_utf8())
        .sum()
}

/// 链接可见文字：去掉协议 / `@`，只保留工作区相对路径。
#[must_use]
pub fn file_ref_visible_label(tok: &str) -> &str {
    tok.strip_prefix("file://./")
        .or_else(|| tok.strip_prefix("file:///"))
        .or_else(|| tok.strip_prefix('@'))
        .unwrap_or(tok)
}

/// 将用户正文切成普通段与文件引用段（保留原文 token，便于复制/导出仍可见协议）。
#[must_use]
pub fn split_user_file_ref_segs(raw: &str) -> Vec<UserTextSeg> {
    let mut out: Vec<UserTextSeg> = Vec::new();
    let mut i = 0usize;
    let mut plain = String::new();
    let flush_plain = |plain: &mut String, out: &mut Vec<UserTextSeg>| {
        if !plain.is_empty() {
            out.push(UserTextSeg::Plain(std::mem::take(plain)));
        }
    };
    while i < raw.len() {
        let ch = raw[i..].chars().next().unwrap();
        let clen = ch.len_utf8();
        // `file://./` 须先于 `file:///`，否则会被后者吞掉。
        if raw[i..].starts_with("file://./") {
            let prefix_len = "file://./".len();
            let rest = &raw[i + prefix_len..];
            let path_len = take_path_token_len(rest);
            if path_len > 0 {
                flush_plain(&mut plain, &mut out);
                out.push(UserTextSeg::FileRef(
                    raw[i..i + prefix_len + path_len].to_string(),
                ));
                i += prefix_len + path_len;
                continue;
            }
        }
        if raw[i..].starts_with("file:///") {
            let prefix_len = "file:///".len();
            let rest = &raw[i + prefix_len..];
            let path_len = take_path_token_len(rest);
            if path_len > 0 {
                flush_plain(&mut plain, &mut out);
                out.push(UserTextSeg::FileRef(
                    raw[i..i + prefix_len + path_len].to_string(),
                ));
                i += prefix_len + path_len;
                continue;
            }
        }
        if ch == '@' {
            let rest = &raw[i + clen..];
            let path_len = take_path_token_len(rest);
            if path_len > 0 {
                flush_plain(&mut plain, &mut out);
                out.push(UserTextSeg::FileRef(
                    raw[i..i + clen + path_len].to_string(),
                ));
                i += clen + path_len;
                continue;
            }
        }
        plain.push(ch);
        i += clen;
    }
    flush_plain(&mut plain, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_file_uri_and_at() {
        let segs = split_user_file_ref_segs("a file:///src/a.rs and @b.rs z");
        assert_eq!(
            segs,
            vec![
                UserTextSeg::Plain("a ".into()),
                UserTextSeg::FileRef("file:///src/a.rs".into()),
                UserTextSeg::Plain(" and ".into()),
                UserTextSeg::FileRef("@b.rs".into()),
                UserTextSeg::Plain(" z".into()),
            ]
        );
        assert_eq!(file_ref_visible_label("file:///src/a.rs"), "src/a.rs");
        assert_eq!(file_ref_visible_label("@b.rs"), "b.rs");
    }
}
