//! `structured_patch` 写侧行为：`base_revision` CAS、原子提交、dry-run unified diff 预览。

use sha2::{Digest, Sha256};

use super::structured_patch_execute;

fn make_test_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "crabmate_spatch_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    dir
}

/// 与 `read_file` 输出头同口径的 revision（内容 SHA-256，小写 hex）。
fn revision_of(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

#[test]
fn structured_patch_stale_base_revision_rejected() {
    let dir = make_test_dir();
    let file = dir.join("c.json");
    std::fs::write(&file, r#"{"a": 1}"#).expect("写测试文件");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    let out = structured_patch_execute(
        r#"{"path":"c.json","query":"/a","value":2,"dry_run":false,"base_revision":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("stale_revision"), "{out}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("读回"),
        r#"{"a": 1}"#,
        "拒写后文件必须保持原样"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_patch_matching_base_revision_writes() {
    let dir = make_test_dir();
    let file = dir.join("c.json");
    let before = r#"{"a": 1}"#;
    std::fs::write(&file, before).expect("写测试文件");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    let args = format!(
        r#"{{"path":"c.json","query":"/a","value":2,"dry_run":false,"base_revision":"{}"}}"#,
        revision_of(before)
    );
    let out = structured_patch_execute(&args, &dir, &ctx);
    assert!(out.contains("已写入"), "{out}");
    let after = std::fs::read_to_string(&file).expect("读回");
    assert!(after.contains("\"a\": 2"), "{after}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_patch_dry_run_previews_unified_diff() {
    let dir = make_test_dir();
    let file = dir.join("c.json");
    std::fs::write(&file, r#"{"a": 1}"#).expect("写测试文件");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    // 不传 dry_run：默认只预览（与另外三个写工具同一旋钮）。
    let out = structured_patch_execute(r#"{"path":"c.json","query":"/a","value":2}"#, &dir, &ctx);
    assert!(out.contains("预览成功"), "{out}");
    assert!(out.contains("--- a/c.json"), "{out}");
    assert!(out.contains("+++ b/c.json"), "{out}");
    assert!(out.contains("@@"), "{out}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("读回"),
        r#"{"a": 1}"#,
        "默认预览不得写盘"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_patch_preview_also_checks_base_revision() {
    let dir = make_test_dir();
    let file = dir.join("c.json");
    std::fs::write(&file, r#"{"a": 1}"#).expect("写测试文件");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    // 预览（`dry_run` 缺省）也必须按显式 `base_revision` 早失败：否则会拿过期快照
    // 生成一份「看起来安全」的 diff，让人据此决定写盘。
    let out = structured_patch_execute(
        r#"{"path":"c.json","query":"/a","value":2,"base_revision":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("stale_revision"), "{out}");
    assert!(!out.contains("预览成功"), "{out}");
    assert_eq!(
        std::fs::read_to_string(&file).expect("读回"),
        r#"{"a": 1}"#,
        "预览失败不得写盘"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn structured_patch_write_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = make_test_dir();
    let file = dir.join("c.json");
    std::fs::write(&file, r#"{"a": 1}"#).expect("写测试文件");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("设置权限");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    let out = structured_patch_execute(
        r#"{"path":"c.json","query":"/a","value":2,"dry_run":false}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已写入"), "{out}");
    let mode = std::fs::metadata(&file)
        .expect("读 metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "原子替换必须继承原文件权限位");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_patch_blank_base_revision_means_no_cas() {
    let dir = make_test_dir();
    let file = dir.join("c.json");
    std::fs::write(&file, r#"{"a": 1}"#).expect("写测试文件");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx = crate::cm_tools::tools::tool_context_for(
        &cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        &dir,
    );
    // 空白 `base_revision` 视为未提供（与 `file::revision::base_revision_arg` 一致）。
    let out = structured_patch_execute(
        r#"{"path":"c.json","query":"/a","value":2,"base_revision":"   "}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("预览成功"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
