use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn make_test_dir() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "crabmate_file_tool_test_{}_{}_{}",
        std::process::id(),
        ts,
        seq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_read_file_with_line_range() {
    let dir = make_test_dir();
    let file = dir.join("a.txt");
    std::fs::write(&file, "a\nb\nc\nd\n").unwrap();
    let out = read_file(r#"{"path":"a.txt","start_line":2,"end_line":3}"#, &dir);
    assert!(out.contains("2|b"), "应包含第 2 行: {}", out);
    assert!(out.contains("3|c"), "应包含第 3 行: {}", out);
    assert!(!out.contains("1|a"), "不应包含第 1 行: {}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_file_respects_max_lines_without_end_line() {
    let dir = make_test_dir();
    let file = dir.join("big.txt");
    let mut s = String::new();
    for i in 1..=1200 {
        s.push_str(&format!("line{i}\n"));
    }
    std::fs::write(&file, &s).unwrap();
    let out = read_file(r#"{"path":"big.txt","max_lines":100}"#, &dir);
    assert!(out.contains("仍有后续内容"), "应提示分段: {}", out);
    assert!(out.contains("下一段可将 start_line 设为 101"), "{}", out);
    assert!(out.contains("100|line100"), "{}", out);
    assert!(!out.contains("101|line101"), "不应超过 max_lines: {}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_binary_meta_prefix_hash() {
    let dir = make_test_dir();
    let file = dir.join("bin.dat");
    std::fs::write(&file, [1u8, 2, 3, 4, 5]).unwrap();
    let out = read_binary_meta(r#"{"path":"bin.dat","prefix_hash_bytes":64}"#, &dir);
    assert!(out.contains("size_bytes: 5"), "{}", out);
    assert!(out.contains("sha256_prefix:"), "{}", out);
    assert!(out.contains("sha256_prefix_bytes: 5"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_binary_meta_skip_hash() {
    let dir = make_test_dir();
    let file = dir.join("x.bin");
    std::fs::write(&file, b"x").unwrap();
    let out = read_binary_meta(r#"{"path":"x.bin","prefix_hash_bytes":0}"#, &dir);
    assert!(out.contains("size_bytes: 1"), "{}", out);
    assert!(out.contains("已跳过"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_hash_file_sha256_empty() {
    let dir = make_test_dir();
    let file = dir.join("empty.dat");
    std::fs::write(&file, []).unwrap();
    let out = hash_file(r#"{"path":"empty.dat","algorithm":"sha256"}"#, &dir);
    assert!(out.contains("digest_hex:"), "{}", out);
    assert!(out.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_hash_file_blake3_prefix() {
    let dir = make_test_dir();
    let file = dir.join("p.bin");
    std::fs::write(&file, b"hello world").unwrap();
    let full = hash_file(r#"{"path":"p.bin","algorithm":"blake3"}"#, &dir);
    let prefix = hash_file(
        r#"{"path":"p.bin","algorithm":"blake3","max_bytes":5}"#,
        &dir,
    );
    assert!(full.contains("digest_hex:"), "{}", full);
    assert!(prefix.contains("hashed_bytes: 5"), "{}", prefix);
    assert_ne!(
        line_digest(&full),
        line_digest(&prefix),
        "整文件与前缀哈希应不同"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn line_digest(out: &str) -> String {
    out.lines()
        .find(|l| l.starts_with("digest_hex:"))
        .unwrap_or("")
        .to_string()
}

#[test]
fn test_modify_file_replace_lines() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":3,"content":"X"}"#,
        &dir,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    let body = std::fs::read_to_string(&file).unwrap();
    assert_eq!(body, "L1\nX\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_file_reject_invalid_range() {
    let dir = make_test_dir();
    let file = dir.join("a.txt");
    std::fs::write(&file, "x\n").unwrap();
    let out = read_file(r#"{"path":"a.txt","start_line":3}"#, &dir);
    assert!(out.contains("超出文件行数"), "应报越界错误: {}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_extract_rust_fn_block() {
    let dir = make_test_dir();
    let file = dir.join("a.rs");
    let content = r##"
pub fn foo(x: i32) -> i32 {
// braces in line comment: { }
let s1 = "{";
let s2 = "}";
let s3 = r#"{"a":1}"#; // braces inside raw string
let s4 = r#"}"#;        // raw string with '}' earlier than function end
/* block comment with { and } should be ignored: { } */
let c = '}';

let _ = some_macro!({
    // comment with { } inside macro invocation should not break extraction
    println!("macro {{ }} {}", x);
    if x > 0 { x + 1 } else { x - 1 }
});

// The real return is still from the outer if/else, so braces above must not affect boundaries.
if x > 0 {
    x + 1 // { in comment { }
} else {
    x - 1
}
}

pub fn bar() { println!("hi"); }
"##;
    std::fs::write(&file, content).unwrap();

    let out = extract_in_file(
        r#"{"path":"a.rs","pattern":"pub\\s+fn\\s+foo","mode":"rust_fn_block","max_matches":1,"max_block_lines":200,"max_block_chars":2000}"#,
        &dir,
    );
    assert!(out.contains("pub fn foo"));
    assert!(out.contains("else"));
    assert!(out.contains("x - 1"));
    assert!(out.contains("let s4 = r#\"}\"#;"));
    assert!(out.trim_end().ends_with('}'));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_file_reject_outside_workspace() {
    let dir = make_test_dir();
    let outside_name = format!("crabmate_outside_read_{}.txt", std::process::id());
    let outside = std::env::temp_dir().join(&outside_name);
    std::fs::write(&outside, "outside\n").unwrap();
    let arg = serde_json::json!({ "path": format!("../{}", outside_name) }).to_string();
    let out = read_file(&arg, &dir);
    assert!(
        out.contains("路径不能超出工作目录"),
        "应拒绝越界读取: {}",
        out
    );
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_create_file_reject_symlink_escape() {
    use std::os::unix::fs::symlink;

    let dir = make_test_dir();
    let outside = std::env::temp_dir().join(format!(
        "crabmate_outside_symlink_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&outside).unwrap();
    let link = dir.join("link_out");
    symlink(&outside, &link).unwrap();

    let out = create_file(r#"{"path":"link_out/pwned.txt","content":"x"}"#, &dir);
    assert!(
        out.contains("路径不能超出工作目录"),
        "应拒绝 symlink 绕过写入: {}",
        out
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn test_glob_files_recursive_rs() {
    let dir = make_test_dir();
    std::fs::create_dir_all(dir.join("src/nested")).unwrap();
    std::fs::write(dir.join("src/a.rs"), "").unwrap();
    std::fs::write(dir.join("src/nested/b.rs"), "").unwrap();
    std::fs::write(dir.join("readme.txt"), "").unwrap();
    let out = glob_files(r#"{"pattern":"**/*.rs"}"#, &dir);
    assert!(out.contains("src/a.rs"), "{}", out);
    assert!(out.contains("src/nested/b.rs"), "{}", out);
    assert!(!out.contains("readme.txt"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_list_tree_respects_max_depth() {
    let dir = make_test_dir();
    std::fs::create_dir_all(dir.join("a/b")).unwrap();
    std::fs::write(dir.join("a/x.txt"), "").unwrap();
    std::fs::write(dir.join("a/b/y.txt"), "").unwrap();
    let out = list_tree(r#"{"max_depth":1}"#, &dir);
    assert!(out.contains("a/") && out.contains("dir:"), "{}", out);
    assert!(out.contains("a/x.txt"), "{}", out);
    assert!(!out.contains("y.txt"), "不应列出 a/b 内文件: {}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_copy_file_basic() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    let out = copy_file(r#"{"from":"a.txt","to":"sub/b.txt"}"#, &dir);
    assert!(out.contains("已复制"), "{}", out);
    assert_eq!(
        std::fs::read_to_string(dir.join("sub/b.txt")).unwrap(),
        "hello"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_copy_file_reject_existing_without_overwrite() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    let out = copy_file(r#"{"from":"a.txt","to":"b.txt"}"#, &dir);
    assert!(out.contains("overwrite"), "{}", out);
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_copy_file_overwrite() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "new").unwrap();
    std::fs::write(dir.join("b.txt"), "old").unwrap();
    let out = copy_file(r#"{"from":"a.txt","to":"b.txt","overwrite":true}"#, &dir);
    assert!(out.contains("已复制"), "{}", out);
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "new");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_move_file_basic() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "mv").unwrap();
    let out = move_file(r#"{"from":"a.txt","to":"c.txt"}"#, &dir);
    assert!(out.contains("已移动"), "{}", out);
    assert!(!dir.join("a.txt").exists());
    assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "mv");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_move_file_reject_existing_without_overwrite() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    let out = move_file(r#"{"from":"a.txt","to":"b.txt"}"#, &dir);
    assert!(out.contains("overwrite"), "{}", out);
    assert!(dir.join("a.txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
