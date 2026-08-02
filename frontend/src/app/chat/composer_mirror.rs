//! 输入框镜像层：将工作区文件引用渲染为与正文区分的 HTML（仅用于 composer 高亮层）。
//!
//! 约定 token（与插入 / 服务端展开一致）：
//! - **`file:///{相对路径}`**（首选，双击工作区树插入）
//! - **`@{相对路径}`**（兼容手输）

fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn take_path_token_len(rest: &str) -> usize {
    rest.chars()
        .take_while(|c| !c.is_whitespace() && *c != '@')
        .map(|c| c.len_utf8())
        .sum()
}

/// 将草稿中的 `file:///` / `@` 文件引用包在 `<span class="composer-ws-ref">` 内；其余字符 HTML 转义。
/// 与 [`super::workspace_panel::make_insert_workspace_path_into_composer`] 插入的 `file:///{rel}` 约定一致。
pub fn composer_workspace_at_refs_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().saturating_mul(2));
    let mut i = 0usize;
    while i < raw.len() {
        let ch = raw[i..].chars().next().unwrap();
        let clen = ch.len_utf8();
        if raw[i..].starts_with("file:///") {
            let prefix_len = "file:///".len();
            let rest = &raw[i + prefix_len..];
            let path_len = take_path_token_len(rest);
            if path_len > 0 {
                let full = &raw[i..i + prefix_len + path_len];
                out.push_str(r#"<span class="composer-ws-ref" title=""#);
                push_escaped(&mut out, full);
                out.push_str(r#"">"#);
                push_escaped(&mut out, full);
                out.push_str("</span>");
                i += prefix_len + path_len;
                continue;
            }
        }
        if ch == '@' {
            let rest = &raw[i + clen..];
            let path_len = take_path_token_len(rest);
            if path_len > 0 {
                let full = &raw[i..i + clen + path_len];
                out.push_str(r#"<span class="composer-ws-ref" title=""#);
                push_escaped(&mut out, full);
                out.push_str(r#"">"#);
                push_escaped(&mut out, full);
                out.push_str("</span>");
                i += clen + path_len;
                continue;
            }
        }
        push_escaped(&mut out, &raw[i..i + clen]);
        i += clen;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::composer_workspace_at_refs_html;

    #[test]
    fn wraps_file_uri_token() {
        let h = composer_workspace_at_refs_html("see file:///src/main.rs ok");
        assert!(h.contains("composer-ws-ref"));
        assert!(h.contains("file:///src/main.rs"));
        assert!(h.contains("see "));
        assert!(h.contains(" ok"));
    }

    #[test]
    fn wraps_at_workspace_token() {
        let h = composer_workspace_at_refs_html("see @src/main.rs ok");
        assert!(h.contains("composer-ws-ref"));
        assert!(h.contains("@src/main.rs"));
    }

    #[test]
    fn escapes_html_outside_refs() {
        let h = composer_workspace_at_refs_html("<x> file:///y ");
        assert!(h.contains("&lt;x&gt;"));
        assert!(h.contains("composer-ws-ref"));
    }
}
