//! `modify_file` 防行号偏移守卫（expect_content / expect_line_content）与批量
//! `edits` 模式的测试；自 tests.rs 拆出以满足单文件行数上限。

use super::tests::make_test_dir;
use super::*;

#[test]
fn test_modify_file_expect_content_mismatch_rejects_and_relocates() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 行号过期：期望内容实际位于行 3，但调用者给的是行 1-1。
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_content":"L3"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("expect_content 校验失败"), "{}", out);
    assert!(out.contains("实际位于行 3"), "{}", out);
    // 未写盘。
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_content_match_writes() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":3,"content":"X\nY","expect_content":"L2\nL3"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nY\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_content_wrong_line_count_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":2,"content":"X","expect_content":"L1"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("行数不一致"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_line_content_mismatch_rejects() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"insert_after_line","after_line":2,"content":"NEW","expect_line_content":"L3"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("expect_line_content 校验失败"), "{}", out);
    assert!(out.contains("实际位于行 3"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_line_content_match_writes() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"insert_after_line","after_line":2,"content":"NEW","expect_line_content":"L2"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已插入内容"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nNEW\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_guard_cross_field_rejected() {
    let dir = make_test_dir();
    std::fs::write(dir.join("m.txt"), "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_line_content":"L1"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("expect_line_content 仅用于 insert_after_line"), "{}", out);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"insert_after_line","after_line":1,"content":"X","expect_content":"L1"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("expect_content 仅用于 replace_lines"), "{}", out);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"full","content":"X","expect_content":"L1"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("不能与 mode=full/overwrite 混用"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_applies_bottom_up() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\nL5\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 乱序给出多条编辑：行号全部基于同一快照，自底向上应用互不偏移。
    let args = r#"{"path":"m.txt","edits":[
        {"start_line":1,"end_line":1,"content":"A"},
        {"after_line":2,"content":"B1\nB2"},
        {"start_line":4,"end_line":4,"content":""}
    ]}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("已应用 3 条局部编辑"), "{}", out);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "A\nL2\nB1\nB2\nL3\nL5\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_insert_at_head_and_tail() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let args = r#"{"path":"m.txt","edits":[
        {"after_line":0,"content":"TOP"},
        {"after_line":2,"content":"BOT"}
    ]}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("已应用 2 条局部编辑"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "TOP\nL1\nL2\nBOT\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_conflict_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 两段替换区间重叠 [2..3] 与 [3..4]。
    let args = r#"{"path":"m.txt","edits":[
        {"start_line":2,"end_line":3,"content":"A"},
        {"start_line":3,"end_line":4,"content":"B"}
    ]}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("重叠冲突"), "{}", out);
    // insert 锚点落在替换区间内部同样冲突。
    let args = r#"{"path":"m.txt","edits":[
        {"start_line":2,"end_line":3,"content":"A"},
        {"after_line":2,"content":"X"}
    ]}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("重叠冲突"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_exclusive_with_top_level_fields() {
    let dir = make_test_dir();
    std::fs::write(dir.join("m.txt"), "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","edits":[{"start_line":1,"end_line":1,"content":"A"}]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    let out = modify_file(
        r#"{"path":"m.txt","edits":[{"start_line":1,"end_line":1,"content":"A"}],"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_out_of_bounds_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","edits":[{"start_line":1,"end_line":5,"content":"A"}]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("超出文件行数"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_expect_guard_per_edit() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"m.txt","edits":[
            {"start_line":1,"end_line":1,"content":"A"},
            {"after_line":2,"content":"X","expect_line_content":"WRONG"}
        ]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("expect_line_content 校验失败"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_dry_run_unchanged() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let args = r#"{"path":"m.txt","dry_run":true,"edits":[
        {"start_line":1,"end_line":1,"content":"A"},
        {"after_line":1,"content":"B"}
    ]}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("dry_run=true"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_limit_and_empty() {
    let dir = make_test_dir();
    std::fs::write(dir.join("m.txt"), "L1\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let mut parts = Vec::new();
    for i in 0..17 {
        parts.push(format!(r#"{{"after_line":0,"content":"N{i}"}}"#));
    }
    let mut args = String::from(r#"{"path":"m.txt","edits":["#);
    args.push_str(&parts.join(","));
    args.push_str("]}");
    let out = modify_file(&args, &dir, &ctx);
    assert!(out.contains("最多 16 条"), "{}", out);
    let out = modify_file(r#"{"path":"m.txt","edits":[]}"#, &dir, &ctx);
    assert!(out.contains("空数组"), "{}", out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_content_end_line_out_of_bounds_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // end_line 超界且 expect 行数恰好等于区间跨度：必须拒绝而不是越界 panic。
    let out = modify_file(
        r#"{"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":5,"content":"X","expect_content":"L2\nL3\n\n"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("超出文件行数"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\nL3\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_insert_at_replace_end_lands_after_block() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\nL5\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 多行替换 + 锚点等于替换区间末行：插入内容必须在替换块之后。
    let out = modify_file(
        r#"{"path":"m.txt","edits":[
            {"start_line":2,"end_line":3,"content":"N1\nN2\nN3"},
            {"after_line":3,"content":"X"}
        ]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已应用"), "{}", out);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "L1\nN1\nN2\nN3\nX\nL4\nL5\n"
    );
    // 删除替换（content 为空）+ 锚点等于被删区间末行：插入内容落在删除点。
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let out = modify_file(
        r#"{"path":"m.txt","edits":[
            {"start_line":2,"end_line":3,"content":""},
            {"after_line":3,"content":"X"}
        ]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已应用"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_insert_at_head_expect_line_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // after_line=0 无锚点行，配 expect_line_content 必须显式拒绝。
    let out = modify_file(
        r#"{"path":"m.txt","edits":[{"after_line":0,"content":"X","expect_line_content":"L1"}]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("无锚点行可校验"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_top_level_expect_rejected() {
    let dir = make_test_dir();
    let file = dir.join("m.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 顶层 expect_* 会被静默忽略，必须显式拒绝并提示写入 edits[i]。
    let out = modify_file(
        r#"{"path":"m.txt","expect_content":"L1","edits":[{"after_line":1,"content":"X"}]}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    assert!(out.contains("edits[i]"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}
