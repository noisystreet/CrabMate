//! 工具入参/正文里出现工作区图片路径时，提示模型用 Markdown 内嵌（Web `GET /workspace/file/raw`）。

const HINT_MARK: &str = "[crabmate] Web 聊天内嵌";

const HINT_TAIL: &str =
    "。不要声称纯文本无法插图，也不要把文件拷到 CM_WEB_STATIC_DIR 或 /uploads。";

pub(super) fn append_if_needed(args_json: &str, output: String) -> String {
    if output.contains(HINT_MARK) {
        return output;
    }
    let mut hay = String::with_capacity(args_json.len().saturating_add(output.len()).saturating_add(1));
    hay.push_str(args_json);
    hay.push('\n');
    hay.push_str(&output);
    let paths = collect_rel_image_paths(&hay);
    if paths.is_empty() {
        return output;
    }
    let examples = markdown_image_examples(&paths);
    format!("{output}\n\n{HINT_MARK}请在最终回复写：{examples}{HINT_TAIL}")
}

fn markdown_image_examples(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| {
            let alt = std::path::Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("image");
            format!("![{alt}]({p})")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_rel_image_paths(hay: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < hay.len() && out.len() < 4 {
        match image_path_span_at(hay, i) {
            Some((start, end)) => {
                let p = &hay[start..end];
                if rel_image_path_ok(p) && !out.iter().any(|x| x == p) {
                    out.push(p.to_string());
                }
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

fn image_path_span_at(hay: &str, i: usize) -> Option<(usize, usize)> {
    let end = ext_end_from(hay, i)?;
    let start = path_start_before(hay, i);
    if start >= end {
        return None;
    }
    Some((start, end))
}

fn ext_end_from(hay: &str, i: usize) -> Option<usize> {
    let rest = hay.get(i..)?;
    let low = rest.to_ascii_lowercase();
    for (lit, n) in [(".jpeg", 5usize), (".webp", 5), (".png", 4), (".jpg", 4), (".gif", 4)] {
        if low.starts_with(lit) {
            let end = i.checked_add(n)?;
            if ext_boundary_ok(hay, end) {
                return Some(end);
            }
        }
    }
    None
}

fn ext_boundary_ok(hay: &str, end: usize) -> bool {
    hay.as_bytes()
        .get(end)
        .is_none_or(|c| !c.is_ascii_alphanumeric())
}

fn path_start_before(hay: &str, ext_dot: usize) -> usize {
    let bytes = hay.as_bytes();
    let mut start = ext_dot;
    while start > 0 && is_rel_path_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

fn is_rel_path_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'/' | b'-')
}

fn rel_image_path_ok(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains("..")
        && p.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_from_python_savefig_arg() {
        let args = r#"{"code":"plt.savefig('sine_plot.png')"}"#;
        let out = append_if_needed(args, "ok\n".into());
        assert!(out.contains("![sine_plot](sine_plot.png)"), "{out}");
        assert!(out.contains("CM_WEB_STATIC_DIR"), "{out}");
    }

    #[test]
    fn no_hint_without_image_path() {
        let out = append_if_needed(r#"{"code":"print(1)"}"#, "1\n".into());
        assert_eq!(out, "1\n");
    }

    #[test]
    fn rejects_parent_dir() {
        let out = append_if_needed("", "../x.png".into());
        assert_eq!(out, "../x.png");
    }
}
