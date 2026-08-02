//! 用户气泡：将 `file:///{rel}` / `file://./{rel}` / `@{rel}` 展示为内联文件引用样式（链接文字仅显示相对路径）。

use leptos::prelude::*;

use crate::session_search::split_for_find_highlight;

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

fn render_find_segments(text: &str, query: &str) -> AnyView {
    let segs = split_for_find_highlight(text, query);
    segs.into_iter()
        .map(|(s, hl)| {
            if hl {
                view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
            } else {
                view! { {s} }.into_any()
            }
        })
        .collect_view()
        .into_any()
}

/// 渲染可能含文件引用的用户正文（查找高亮仍作用在各段上）。
#[must_use]
pub fn render_user_text_with_file_refs(text: &str, query: &str) -> AnyView {
    let segs = split_user_file_ref_segs(text);
    if segs.iter().all(|s| matches!(s, UserTextSeg::Plain(_))) {
        return render_find_segments(text, query);
    }
    segs.into_iter()
        .map(|seg| match seg {
            UserTextSeg::Plain(p) => render_find_segments(&p, query),
            UserTextSeg::FileRef(tok) => {
                let display = file_ref_visible_label(&tok).to_string();
                view! {
                    <span class="msg-file-ref" title=tok.clone()>
                        {display}
                    </span>
                }
                .into_any()
            }
        })
        .collect_view()
        .into_any()
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
    }

    #[test]
    fn splits_file_dot_slash_uri() {
        let segs = split_user_file_ref_segs("x file://./.gitignore y");
        assert_eq!(
            segs,
            vec![
                UserTextSeg::Plain("x ".into()),
                UserTextSeg::FileRef("file://./.gitignore".into()),
                UserTextSeg::Plain(" y".into()),
            ]
        );
    }

    #[test]
    fn visible_label_strips_scheme() {
        assert_eq!(file_ref_visible_label("file://./.gitignore"), ".gitignore");
        assert_eq!(file_ref_visible_label("file:///.gitignore"), ".gitignore");
        assert_eq!(file_ref_visible_label("@src/main.rs"), "src/main.rs");
    }
}
