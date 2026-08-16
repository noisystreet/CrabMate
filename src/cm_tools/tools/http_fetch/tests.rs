use reqwest::Url;

use super::decode::{apply_text_format_if_requested, decode_http_body_text_for_tool};
use super::{
    FetchMethod, HttpBodyTextFormat, RequestMethod, approval_args_display,
    approval_args_display_request, html_to_readable_text, parse_http_fetch_args,
    parse_http_request_args, request_storage_key, url_matches_allowed_prefixes,
};

#[test]
fn parse_defaults_to_get() {
    let (u, m, fmt) = parse_http_fetch_args(r#"{"url":"https://example.com/a"}"#).unwrap();
    assert_eq!(m, FetchMethod::Get);
    assert_eq!(fmt, HttpBodyTextFormat::Raw);
    assert_eq!(u.as_str(), "https://example.com/a");
}

#[test]
fn parse_head() {
    let (u, m, fmt) =
        parse_http_fetch_args(r#"{"url":"https://example.com/","method":"head"}"#).unwrap();
    assert_eq!(m, FetchMethod::Head);
    assert_eq!(fmt, HttpBodyTextFormat::Raw);
    assert_eq!(u.host_str(), Some("example.com"));
}

#[test]
fn parse_html_text_format() {
    let (_, _, fmt) =
        parse_http_fetch_args(r#"{"url":"https://example.com/","text_format":"html_text"}"#)
            .unwrap();
    assert_eq!(fmt, HttpBodyTextFormat::HtmlText);
}

#[test]
fn approval_display_head() {
    let u = Url::parse("https://ex.com/x?q=1").unwrap();
    let s = approval_args_display(FetchMethod::Head, &u);
    assert!(s.starts_with("HEAD "));
    assert!(s.contains("ex.com/x"));
    assert!(!s.contains("q=1"), "query 应被脱敏: {}", s);
}

#[test]
fn request_storage_key_includes_method_and_strips_query() {
    let u = Url::parse("https://ex.com/a?x=1").unwrap();
    let k = request_storage_key(RequestMethod::Post, &u);
    assert_eq!(k, "http_request:POST:https://ex.com/a");
}

#[test]
fn approval_display_request_redacts_and_notes_body() {
    let u = Url::parse("https://ex.com/x?q=1").unwrap();
    let s = approval_args_display_request(RequestMethod::Put, &u, true);
    assert!(s.starts_with("PUT "));
    assert!(s.contains("（含 json_body）"));
    assert!(!s.contains("q=1"));
}

#[test]
fn allowed_prefix_requires_origin_and_path_boundary() {
    let url = Url::parse("https://example.com/api/v1/users").unwrap();
    assert!(url_matches_allowed_prefixes(
        &url,
        &["https://example.com/api/".to_string()]
    ));
    assert!(url_matches_allowed_prefixes(
        &url,
        &["https://example.com/api".to_string()]
    ));
    assert!(!url_matches_allowed_prefixes(
        &url,
        &["https://example.com/ap".to_string()]
    ));
    assert!(!url_matches_allowed_prefixes(
        &url,
        &["https://example.com/api2/".to_string()]
    ));
    assert!(!url_matches_allowed_prefixes(
        &url,
        &["https://example.comx/api/".to_string()]
    ));
}
#[test]
fn parse_http_request_supports_patch_with_body() {
    let (u, m, body, fmt) = parse_http_request_args(
        r#"{"url":"https://example.com/api","method":"patch","json_body":{"x":1}}"#,
    )
    .unwrap();
    assert_eq!(u.as_str(), "https://example.com/api");
    assert_eq!(m, RequestMethod::Patch);
    assert_eq!(fmt, HttpBodyTextFormat::Raw);
    let b = body.unwrap();
    assert_eq!(b.get("x").and_then(|x| x.as_i64()), Some(1));
}

#[test]
fn parse_http_request_rejects_get() {
    let err =
        parse_http_request_args(r#"{"url":"https://example.com","method":"GET"}"#).unwrap_err();
    assert!(err.contains("POST/PUT/PATCH/DELETE"));
}

#[test]
fn decode_body_uses_content_type_charset() {
    let gbk = encoding_rs::GBK;
    let bytes = gbk.encode("你好").0;
    let (text, note) = decode_http_body_text_for_tool("text/plain; charset=gbk", bytes.as_ref());
    assert_eq!(text, "你好");
    assert!(note.to_ascii_lowercase().contains("gbk"), "note={}", note);
}

#[test]
fn decode_body_html_meta_charset() {
    let html =
        b"<!DOCTYPE html><html><head><meta charset=\"gbk\"></head><body>\xc4\xe3\xba\xc3</body>";
    let (text, note) = decode_http_body_text_for_tool("text/html", html);
    assert!(text.contains("你好"), "text={}", text);
    assert!(
        note.contains("meta") || note.to_ascii_lowercase().contains("gbk"),
        "note={}",
        note
    );
}

#[test]
fn decode_body_json_utf8_strict() {
    let (text, note) = decode_http_body_text_for_tool("application/json", br#"{"a":1}"#);
    assert_eq!(text, r#"{"a":1}"#);
    assert!(note.contains("UTF-8"), "note={}", note);
}

#[test]
fn decode_body_xml_declaration() {
    let xml = b"<?xml version=\"1.0\" encoding=\"GBK\"?><r>\xc4\xe3\xba\xc3</r>";
    let (text, note) = decode_http_body_text_for_tool("application/xml", xml);
    assert!(text.contains("你好"), "text={}", text);
    assert!(note.contains("XML"), "note={}", note);
}

#[test]
fn html_to_text_strips_script_and_collapses() {
    let html = r#"<!DOCTYPE html><html><head><title>x</title></head><body>
            <script>alert(1)</script>
            <p>Hello <b>world</b></p>
            <style>.x{}</style>
        </body></html>"#;
    let t = html_to_readable_text(html).unwrap();
    assert!(t.contains("Hello"));
    assert!(t.contains("world"));
    assert!(!t.contains("script"));
    assert!(!t.contains("alert"));
}

#[test]
fn apply_html_text_format_on_html() {
    let html = "<html><body><p>Hi</p></body></html>";
    let (body, note) = apply_text_format_if_requested(
        "text/html",
        HttpBodyTextFormat::HtmlText,
        html.to_string(),
        "正文解码: UTF-8（测试）".to_string(),
    );
    assert_eq!(body, "Hi");
    assert!(note.contains("HTML→纯文本"), "note={}", note);
}
