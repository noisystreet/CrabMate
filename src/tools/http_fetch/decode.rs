use std::ops::Deref;
use std::sync::LazyLock;

use chardetng::EncodingDetector;
use ego_tree::NodeRef;
use encoding_rs::Encoding;
use regex::Regex;
use scraper::{Html, Node, Selector};

use super::args::HttpBodyTextFormat;

/// 扫描 HTML 声明编码的前缀长度（与常见文档头部一致即可）
const HTML_CHARSET_SNIFF_MAX: usize = 16 * 1024;
/// `chardetng` 嗅探用的最大前缀
const CHARDET_SNIFF_MAX: usize = 64 * 1024;

static RE_META_CHARSET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<meta\b[^>]{0,1024}?\bcharset\s*=\s*["']?([^"'>\s]+)"#)
        .expect("http_fetch meta charset regex")
});
static RE_META_HTTP_EQUIV_CT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)<meta\b[^>]{0,1024}?\bhttp-equiv\s*=\s*["']?content-type["']?[^>]{0,1024}?\bcontent\s*=\s*["']([^"']+)["']"#,
    )
    .expect("http_fetch meta http-equiv regex")
});
static RE_XML_ENCODING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<\?xml\b[^>]*\bencoding\s*=\s*["']([^"']+)["']"#)
        .expect("http_fetch xml encoding regex")
});

static SEL_HTML_MAIN: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("main, article, [role='main']").expect("http_fetch selector main/article")
});
static SEL_HTML_BODY: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("body").expect("http_fetch selector body"));

fn walk_visible_text(node: NodeRef<'_, Node>, out: &mut String, in_skip: bool) {
    match node.value() {
        Node::Element(el) => {
            let name = el.name();
            let skip = in_skip
                || name.eq_ignore_ascii_case("script")
                || name.eq_ignore_ascii_case("style")
                || name.eq_ignore_ascii_case("noscript")
                || name.eq_ignore_ascii_case("template");
            for child in node.children() {
                walk_visible_text(child, out, skip);
            }
        }
        Node::Text(t) => {
            if !in_skip {
                let s = t.trim();
                if !s.is_empty() {
                    if !out.is_empty() && !out.chars().last().is_some_and(|c| c.is_whitespace()) {
                        out.push(' ');
                    }
                    out.push_str(s);
                }
            }
        }
        _ => {
            for child in node.children() {
                walk_visible_text(child, out, in_skip);
            }
        }
    }
}

/// 将 HTML 文档转为单行间距可读的纯文本（供模型阅读；非完整排版还原）。
pub fn html_to_readable_text(html: &str) -> Result<String, String> {
    let doc = Html::parse_document(html);
    let root = doc.root_element();
    let scope = root
        .select(&SEL_HTML_MAIN)
        .next()
        .or_else(|| root.select(&SEL_HTML_BODY).next())
        .unwrap_or(root);
    let node: NodeRef<'_, Node> = *scope.deref();
    let mut out = String::new();
    walk_visible_text(node, &mut out, false);
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Err("HTML 解析后无可见文本".to_string());
    }
    Ok(collapsed)
}

fn looks_like_html_document(content_type: &str, decoded: &str) -> bool {
    if should_sniff_html_meta(content_type) || content_type.trim().is_empty() {
        return true;
    }
    let t = decoded.trim_start();
    if !t.starts_with('<') {
        return false;
    }
    let lower = t.chars().take(256).collect::<String>().to_ascii_lowercase();
    lower.contains("<html") || lower.starts_with("<!doctype html")
}

/// 供 `sync_fetch` 与单元测试使用。
pub(crate) fn apply_text_format_if_requested(
    content_type: &str,
    format: HttpBodyTextFormat,
    decoded: String,
    decode_note: String,
) -> (String, String) {
    if format != HttpBodyTextFormat::HtmlText {
        return (decoded, decode_note);
    }
    if is_likely_binary_content_type(content_type) || is_json_content_type(content_type) {
        let note = format!(
            "{decode_note}；正文呈现: 已请求 html_text，但 Content-Type 非 HTML，保留解码原文"
        );
        return (decoded, note);
    }
    if !looks_like_html_document(content_type, &decoded) {
        let note =
            format!("{decode_note}；正文呈现: 已请求 html_text，但正文不像 HTML，保留解码原文");
        return (decoded, note);
    }
    match html_to_readable_text(&decoded) {
        Ok(t) => (
            t,
            format!("{decode_note}；正文呈现: HTML→纯文本（scraper/html5ever）"),
        ),
        Err(e) => (
            decoded,
            format!("{decode_note}；正文呈现: HTML→纯文本失败（{e}），以下为解码原文"),
        ),
    }
}

fn charset_from_content_type(content_type: &str) -> Option<String> {
    for part in content_type.split(';').skip(1) {
        let part = part.trim();
        let (name, value) = part.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("charset") {
            continue;
        }
        let v = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if v.is_empty() {
            continue;
        }
        return Some(v.to_string());
    }
    None
}

fn encoding_for_label(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.as_bytes())
}

/// 自 HTML 前缀提取声明编码（`charset` 或 `http-equiv=content-type` 内的 charset）。
fn xml_declared_encoding_prefix(bytes: &[u8]) -> Option<&'static Encoding> {
    let take = bytes.len().min(4096);
    if take == 0 {
        return None;
    }
    let lossy = String::from_utf8_lossy(&bytes[..take]);
    let c = RE_XML_ENCODING.captures(lossy.as_ref())?;
    encoding_for_label(c.get(1)?.as_str())
}

fn html_declared_encoding_prefix(bytes: &[u8]) -> Option<&'static Encoding> {
    let take = bytes.len().min(HTML_CHARSET_SNIFF_MAX);
    if take == 0 {
        return None;
    }
    let head = &bytes[..take];
    let lossy = String::from_utf8_lossy(head);
    if let Some(c) = RE_META_CHARSET.captures(lossy.as_ref())
        && let Some(m) = c.get(1)
        && let Some(enc) = encoding_for_label(m.as_str())
    {
        return Some(enc);
    }
    if let Some(c) = RE_META_HTTP_EQUIV_CT.captures(lossy.as_ref())
        && let Some(m) = c.get(1)
        && let Some(cs) = charset_from_content_type(m.as_str())
    {
        return encoding_for_label(&cs);
    }
    None
}

fn sniff_encoding_chardetng(bytes: &[u8]) -> &'static Encoding {
    let take = bytes.len().min(CHARDET_SNIFF_MAX);
    let slice = if take == 0 { bytes } else { &bytes[..take] };
    let mut det = EncodingDetector::new();
    det.feed(slice, true);
    det.guess(None, true)
}

fn is_likely_binary_content_type(content_type: &str) -> bool {
    let ct = content_type.trim();
    let lower = ct.to_ascii_lowercase();
    let essence = lower.split(';').next().unwrap_or("").trim();
    essence.starts_with("image/")
        || essence.starts_with("video/")
        || essence.starts_with("audio/")
        || essence == "application/octet-stream"
        || essence == "application/pdf"
        || essence == "application/zip"
        || essence == "application/gzip"
        || essence == "application/x-gzip"
        || essence == "application/wasm"
}

fn is_json_content_type(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    let essence = lower.split(';').next().unwrap_or("").trim();
    essence == "application/json" || essence.ends_with("+json") || essence == "text/json"
}

fn is_xml_family_content_type(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    let essence = lower.split(';').next().unwrap_or("").trim();
    essence == "application/xml" || essence == "text/xml" || essence.ends_with("+xml")
}

fn should_sniff_html_meta(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    let essence = lower.split(';').next().unwrap_or("").trim();
    essence == "text/html" || essence == "application/xhtml+xml" || essence.is_empty()
}

/// 将响应体解码为工具输出用字符串，并返回一行「正文解码」说明（写入 `http_fetch` / `http_request` 输出）。
pub(crate) fn decode_http_body_text_for_tool(content_type: &str, bytes: &[u8]) -> (String, String) {
    if bytes.is_empty() {
        return (String::new(), "正文解码: (空 body)".to_string());
    }

    if is_likely_binary_content_type(content_type) {
        let lossy = String::from_utf8_lossy(bytes);
        let label = "正文解码: 非文本 Content-Type，按 UTF-8 有损预览（可能含乱码或不可打印字符）"
            .to_string();
        return (lossy.into_owned(), label);
    }

    if is_json_content_type(content_type) {
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                return (
                    s.to_string(),
                    "正文解码: UTF-8（JSON Content-Type）".to_string(),
                );
            }
            Err(_) => {
                let lossy = String::from_utf8_lossy(bytes);
                return (
                    lossy.into_owned(),
                    "正文解码: JSON 声明但字节非合法 UTF-8，已用 UTF-8 有损预览".to_string(),
                );
            }
        }
    }

    // 1) Content-Type charset
    if let Some(ref label_str) = charset_from_content_type(content_type)
        && let Some(enc) = encoding_for_label(label_str)
    {
        let (cow, _) = enc.decode_with_bom_removal(bytes);
        return (
            cow.into_owned(),
            format!("正文解码: {}（Content-Type charset）", enc.name()),
        );
    }

    // 2) HTML / XHTML meta
    if (should_sniff_html_meta(content_type) || content_type.trim().is_empty())
        && let Some(enc) = html_declared_encoding_prefix(bytes)
    {
        let (cow, _) = enc.decode_with_bom_removal(bytes);
        return (
            cow.into_owned(),
            format!("正文解码: {}（HTML meta 声明）", enc.name()),
        );
    }

    // 3) XML 声明 encoding=
    if is_xml_family_content_type(content_type)
        && let Some(enc) = xml_declared_encoding_prefix(bytes)
    {
        let (cow, _) = enc.decode_with_bom_removal(bytes);
        return (
            cow.into_owned(),
            format!("正文解码: {}（XML 声明 encoding）", enc.name()),
        );
    }

    // 4) chardetng
    let guessed = sniff_encoding_chardetng(bytes);
    let (cow, _) = guessed.decode_with_bom_removal(bytes);
    (
        cow.into_owned(),
        format!("正文解码: {}（chardetng 嗅探）", guessed.name()),
    )
}
