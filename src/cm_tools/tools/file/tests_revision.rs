//! `read_file` / `modify_file` 的 revision（CAS）校验测试；自 tests.rs 拆出以满足单文件行数上限。

use super::tests::make_test_dir;
use super::*;

/// 从 `read_file` 成功输出首行 `crabmate_tool_output` 头中取字符串字段。
fn read_file_header_str(out: &str, key: &str) -> String {
    let first = out.lines().next().expect("read_file 输出不应为空");
    let v: serde_json::Value = serde_json::from_str(first).expect("首行应为 JSON 头");
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("头缺少字段 {key}: {first}"))
        .to_string()
}

/// 独立计算期望的内容 revision，避免直接复用生产实现。
fn sha256_revision(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

#[test]
fn test_read_file_output_header_contains_content_revision() {
    let dir = make_test_dir();
    let content = "alpha\nbeta\n";
    let file = dir.join("rev.txt");
    std::fs::write(&file, content).unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let out = read_file(r#"{"path":"rev.txt"}"#, &dir, &ctx);
    let revision = read_file_header_str(&out, "revision");
    assert_eq!(revision, sha256_revision(content.as_bytes()));
    assert!(revision.starts_with("sha256:"), "{}", revision);
    assert_eq!(revision.len(), "sha256:".len() + 64);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_read_file_empty_file_reports_empty_content_revision_and_is_writable() {
    let dir = make_test_dir();
    let file = dir.join("empty.txt");
    std::fs::write(&file, "").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let out = read_file(r#"{"path":"empty.txt"}"#, &dir, &ctx);
    let revision = read_file_header_str(&out, "revision");
    assert_eq!(
        revision,
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    let args = format!(
        r#"{{"dry_run":false,"path":"empty.txt","mode":"insert_after_line","after_line":0,"content":"hello","base_revision":"{revision}"}}"#
    );
    let written = modify_file(&args, &dir, &ctx);
    assert!(written.contains("已插入内容"), "{}", written);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_accepts_matching_base_revision() {
    let dir = make_test_dir();
    let file = dir.join("ok.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let out = read_file(r#"{"path":"ok.txt"}"#, &dir, &ctx);
    let revision = read_file_header_str(&out, "revision");
    let args = format!(
        r#"{{"dry_run":false,"path":"ok.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","base_revision":"{revision}"}}"#
    );
    let written = modify_file(&args, &dir, &ctx);
    assert!(written.contains("已按行替换"), "{}", written);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_rejects_stale_base_revision_without_writing() {
    let dir = make_test_dir();
    let file = dir.join("stale.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let out = read_file(r#"{"path":"stale.txt"}"#, &dir, &ctx);
    let stale = read_file_header_str(&out, "revision");

    // 模拟同轮另一处写入（或其它进程）已改动文件，旧 revision 随即过期。
    std::fs::write(&file, "L1\nL2 changed\nL3\n").unwrap();
    let after_external = std::fs::read_to_string(&file).unwrap();

    let cases = [
        format!(
            r#"{{"path":"stale.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","base_revision":"{stale}","dry_run":false}}"#
        ),
        format!(
            r#"{{"path":"stale.txt","mode":"insert_after_line","after_line":1,"content":"INS","base_revision":"{stale}","dry_run":false}}"#
        ),
        format!(
            r#"{{"path":"stale.txt","mode":"full","content":"ALL NEW\n","base_revision":"{stale}","dry_run":false}}"#
        ),
        format!(
            r#"{{"path":"stale.txt","edits":[{{"start_line":2,"end_line":2,"content":"X"}}],"base_revision":"{stale}","dry_run":false}}"#
        ),
        format!(
            r#"{{"path":"stale.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","dry_run":true,"base_revision":"{stale}"}}"#
        ),
    ];
    for args in &cases {
        let out = modify_file(args, &dir, &ctx);
        assert!(out.contains("stale_revision"), "args={args}\nout={out}");
        let r = crate::cm_tools::tool_result::ToolResult::from_output("modify_file", out);
        assert_eq!(
            r.error_code.as_deref(),
            Some("stale_revision"),
            "应归类为 stale_revision: {}",
            r.message
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            after_external,
            "被拒绝的写入不应改动文件: args={args}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn ctx_with_turn_cache<'a>(
    cfg: &'a crate::cm_config::AgentConfig,
    cache: &'a crate::cm_tools::read_file_turn_cache::ReadFileTurnCache,
    dir: &'a std::path::Path,
) -> crate::cm_tools::tools::ToolContext<'a> {
    crate::cm_tools::tools::tool_context_for_with_read_cache_and_memory(
        cfg,
        cfg.command_exec.allowed_commands.as_ref(),
        dir,
        Some(cache),
        None,
        None,
        None,
        None,
    )
}

#[test]
fn test_line_mode_auto_binds_turn_revision_and_rejects_after_external_change() {
    let dir = make_test_dir();
    let file = dir.join("auto.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let cache = crate::cm_tools::read_file_turn_cache::new_turn_cache_handle(8);
    let ctx = ctx_with_turn_cache(&cfg, &cache, &dir);

    let _ = read_file(r#"{"path":"auto.txt"}"#, &dir, &ctx);
    std::fs::write(&file, "L1\nL2 changed\nL3\n").unwrap();
    let after_external = std::fs::read_to_string(&file).unwrap();

    // 未传 base_revision：行号类模式应使用本轮 read_file 的 revision 自动拒绝。
    let args = [
        r#"{"path":"auto.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","dry_run":false}"#,
        r#"{"path":"auto.txt","mode":"insert_after_line","after_line":1,"content":"INS","dry_run":false}"#,
        r#"{"path":"auto.txt","edits":[{"start_line":2,"end_line":2,"content":"X"}],"dry_run":false}"#,
    ];
    for a in args {
        let out = modify_file(a, &dir, &ctx);
        assert!(out.contains("stale_revision"), "args={a}\nout={out}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), after_external);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_allow_stale_revision_skips_turn_binding_but_not_explicit_base_revision() {
    let dir = make_test_dir();
    let file = dir.join("escape.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let cache = crate::cm_tools::read_file_turn_cache::new_turn_cache_handle(8);
    let ctx = ctx_with_turn_cache(&cfg, &cache, &dir);

    let out = read_file(r#"{"path":"escape.txt"}"#, &dir, &ctx);
    let stale = read_file_header_str(&out, "revision");
    std::fs::write(&file, "L1\nL2 changed\nL3\n").unwrap();

    let out = modify_file(
        r#"{"dry_run":false,"path":"escape.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL3\n");

    // 显式 base_revision 仍须校验，allow_stale_revision 不能绕过。
    let out = modify_file(
        &format!(
            r#"{{"path":"escape.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"Y","base_revision":"{stale}","allow_stale_revision":true,"dry_run":false}}"#
        ),
        &dir,
        &ctx,
    );
    assert!(out.contains("stale_revision"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_turn_binding_survives_cache_clear_for_second_write() {
    let dir = make_test_dir();
    let file = dir.join("twice.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let cache = crate::cm_tools::read_file_turn_cache::new_turn_cache_handle(8);
    let ctx = ctx_with_turn_cache(&cfg, &cache, &dir);

    let _ = read_file(r#"{"path":"twice.txt"}"#, &dir, &ctx);
    let first = modify_file(
        r#"{"dry_run":false,"path":"twice.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(first.contains("已按行替换"), "{}", first);

    // 模拟写类工具执行后的正文缓存清理：绑定必须保留，否则第二次写入会漏检。
    cache.clear();
    let second = modify_file(
        r#"{"dry_run":false,"path":"twice.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"Y"}"#,
        &dir,
        &ctx,
    );
    assert!(second.contains("stale_revision"), "{}", second);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_zero_max_entries_still_auto_binds_turn_revision() {
    let dir = make_test_dir();
    let file = dir.join("nocache.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let cache = crate::cm_tools::read_file_turn_cache::new_turn_cache_handle(0);
    let ctx = ctx_with_turn_cache(&cfg, &cache, &dir);

    let _ = read_file(r#"{"path":"nocache.txt"}"#, &dir, &ctx);
    std::fs::write(&file, "L1\nL2 changed\nL3\n").unwrap();
    let after_external = std::fs::read_to_string(&file).unwrap();

    let out = modify_file(
        r#"{"dry_run":false,"path":"nocache.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("stale_revision"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), after_external);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_full_mode_is_not_auto_bound_to_turn_revision() {
    let dir = make_test_dir();
    let file = dir.join("full.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let cache = crate::cm_tools::read_file_turn_cache::new_turn_cache_handle(8);
    let ctx = ctx_with_turn_cache(&cfg, &cache, &dir);

    let _ = read_file(r#"{"path":"full.txt"}"#, &dir, &ctx);
    std::fs::write(&file, "L1\nL2 changed\nL3\n").unwrap();

    // full 模式不依赖行号，不应被自动绑定拦截。
    let out = modify_file(
        r#"{"dry_run":false,"path":"full.txt","mode":"full","content":"M1\nM2\nM3\n"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已整文件覆盖"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "M1\nM2\nM3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// full / edits 均走 temp + rename：不残留临时文件，且替换 inode（不再原地截断）。
#[cfg(unix)]
#[test]
fn test_full_and_edits_commit_via_atomic_rename() {
    use std::os::unix::fs::MetadataExt;

    let dir = make_test_dir();
    let file = dir.join("atomic.txt");
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let ino_before = std::fs::metadata(&file).unwrap().ino();

    let out = modify_file(
        r#"{"dry_run":false,"path":"atomic.txt","mode":"full","content":"M1\nM2\nM3\n"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已整文件覆盖"), "{}", out);
    let ino_after_full = std::fs::metadata(&file).unwrap().ino();
    assert_ne!(ino_before, ino_after_full, "full 应为 rename 原子替换");

    let out = modify_file(
        r#"{"dry_run":false,"path":"atomic.txt","edits":[{"start_line":2,"end_line":2,"content":"X"}],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已应用"), "{}", out);
    assert_ne!(
        ino_after_full,
        std::fs::metadata(&file).unwrap().ino(),
        "edits 应为 rename 原子替换"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "M1\nX\nM3\n");

    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("crabmate_write_tmp"))
        .collect();
    assert!(leftovers.is_empty(), "不应残留临时文件: {leftovers:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_line_mode_without_read_baseline_is_rejected_with_code() {
    // F1 回归：本轮没 `read_file` 过、调用方也没传 `base_revision` 时，行号类模式必须拒写，
    // 而不是凭记忆的行号直接落盘；`allow_stale_revision=true` 仍是显式逃生门。
    let dir = make_test_dir();
    let file = dir.join("nobase.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let err = modify_file_try(
        r#"{"path":"nobase.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","dry_run":false}"#,
        &super::PathAccess::new(&dir),
        &ctx,
    )
    .expect_err("无快照基线的行号编辑应被拒绝");
    assert_eq!(err.code, "missing_revision_baseline", "{}", err.message);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "L1\nL2\nL3\n",
        "拒写后文件必须保持原样"
    );

    // 显式承担风险后放行（仍须按当前行号写入）。
    let out = modify_file(
        r#"{"dry_run":false,"path":"nobase.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}
