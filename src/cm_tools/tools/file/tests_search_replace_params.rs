//! `search_replace` 的参数命名（`old_text` / `new_text`）与「两个值写反」诊断测试。
//!
//! 从 `search_replace.rs` 的 `mod tests` 拆出：实现文件已逼近单文件 920 行上限。

use super::search_replace::search_replace;
use super::test_support::{test_ctx, write};
use std::fs;

/// 主名 `old_text` / `new_text` 的正常路径。
#[test]
fn search_replace_primary_names_work() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "old\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"old","new_text":"new","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("已替换 1 处匹配"), "{out}");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "new\n"
    );
}

/// 兼容别名：旧名与其它常见命名都必须仍能解析（否则改名就是破坏性回归）。
#[test]
fn search_replace_accepts_legacy_and_common_aliases() {
    for (old_key, new_key) in [
        ("search", "replace"),
        ("old_string", "new_string"),
        ("find", "replacement"),
        ("search_text", "replace_with"),
        ("original", "replacement_text"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "old\n");
        let ctx = test_ctx(tmp.path());
        let args =
            format!(r#"{{"path":"a.txt","{old_key}":"old","{new_key}":"new","dry_run":false}}"#);
        let out = search_replace(&args, tmp.path(), &ctx);
        assert!(
            out.contains("已替换 1 处匹配"),
            "{old_key}/{new_key}: {out}"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "new\n",
            "{old_key}/{new_key}"
        );
    }
}

/// 漏传 `new_text`：拒写 + 中文消息 + 列出实际收到的键。
///
/// 回归：此前 `new_text` 有 `#[serde(default)]`，漏传等于 `""`，于是「忘记写新文本」
/// 会静默删除命中的内容。
#[test]
fn search_replace_missing_new_text_is_rejected_with_received_keys() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "old\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"old","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("缺少 new_text 参数"), "{out}");
    assert!(out.contains("收到的字段"), "{out}");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "old\n",
        "漏传 new_text 不得删除内容"
    );
}

/// 漏传 `old_text`：与缺 `new_text` 对称——拒写，并列出实际收到的字段。
#[test]
fn search_replace_missing_old_text_is_rejected_with_received_keys() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "old\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","new_text":"new","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("缺少 old_text 参数"), "{out}");
    assert!(out.contains("收到的字段"), "{out}");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "old\n",
        "缺 old_text 不得写盘"
    );
}

/// 两个值写反：零命中的错误里直接给出对调建议 + `new_text` 的命中行。
#[test]
fn search_replace_reversed_texts_are_diagnosed() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "fn foo(2) {}\n");
    let ctx = test_ctx(tmp.path());
    // 想把 foo(2) 改成 foo(1)，却把两个值写反。
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"fn foo(1)","new_text":"fn foo(2)","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("疑似 old_text 与 new_text 写反"), "{out}");
    assert!(out.contains("L1"), "应列出 new_text 的命中行: {out}");
    assert!(
        out.contains("\"old_text\": \"fn foo(2)\""),
        "应给出对调后的调用: {out}"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "fn foo(2) {}\n",
        "诊断路径不得写盘"
    );
}

/// 对调建议必须带上 `regex`：正则锚点被当字面量重试会「看起来对、结果不同」。
#[test]
fn search_replace_reversed_hint_keeps_regex_flag() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "foo(2)\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"foo\\(1\\)","new_text":"foo(2)","regex":true,"dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("疑似 old_text 与 new_text 写反"), "{out}");
    assert!(
        out.contains("\"regex\": true"),
        "对调建议丢了 regex 标志: {out}"
    );
}

/// 两个值都不在文件中：提示检查 path / old_text，而不是只报「未找到匹配」。
#[test]
fn search_replace_both_texts_absent_hints_path() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "hello\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"zzz","new_text":"yyy","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("new_text 也不在该文件中"), "{out}");
}

/// 显式空 `new_text` 是合法删除，但正文必须以删除警示开头。
#[test]
fn search_replace_empty_new_text_deletes_with_warning() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "keep\ndrop\n");
    let ctx = test_ctx(tmp.path());
    let out = search_replace(
        r#"{"path":"a.txt","old_text":"drop\n","new_text":"","dry_run":false}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("删除内容"), "{out}");
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "keep\n"
    );
}
