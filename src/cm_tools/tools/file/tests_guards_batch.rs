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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_content":"L3","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.content 校验失败"), "{}", out);
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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":3,"content":"X\nY","expect_content":"L2\nL3","allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":2,"content":"X","expect_content":"L1","allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","mode":"insert_after_line","after_line":2,"content":"NEW","expect_line_content":"L3","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.anchor_line 校验失败"), "{}", out);
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
        r#"{"dry_run":false,"path":"m.txt","mode":"insert_after_line","after_line":2,"content":"NEW","expect_line_content":"L2","allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_line_content":"L1","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.anchor_line 仅用于 insert_after_line"), "{}", out);
    let out = modify_file(
        r#"{"dry_run":false,"path":"m.txt","mode":"insert_after_line","after_line":1,"content":"X","expect_content":"L1","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.content 仅用于 replace_lines"), "{}", out);
    let out = modify_file(
        r#"{"dry_run":false,"path":"m.txt","mode":"full","content":"X","expect_content":"L1"}"#,
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
    let args = r#"{"dry_run":false,"path":"m.txt","edits":[
        {"start_line":1,"end_line":1,"content":"A"},
        {"after_line":2,"content":"B1\nB2"},
        {"start_line":4,"end_line":4,"content":""}
    ],"allow_stale_revision":true}"#;
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
    let args = r#"{"dry_run":false,"path":"m.txt","edits":[
        {"after_line":0,"content":"TOP"},
        {"after_line":2,"content":"BOT"}
    ],"allow_stale_revision":true}"#;
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
    ],"allow_stale_revision":true}"#;
    let out = modify_file(args, &dir, &ctx);
    assert!(out.contains("重叠冲突"), "{}", out);
    // insert 锚点落在替换区间内部同样冲突。
    let args = r#"{"path":"m.txt","edits":[
        {"start_line":2,"end_line":3,"content":"A"},
        {"after_line":2,"content":"X"}
    ],"allow_stale_revision":true}"#;
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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","edits":[{"start_line":1,"end_line":1,"content":"A"}],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    let out = modify_file(
        r#"{"dry_run":false,"path":"m.txt","edits":[{"start_line":1,"end_line":1,"content":"A"}],"content":"X","allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","edits":[{"start_line":1,"end_line":5,"content":"A"}],"allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","edits":[
            {"start_line":1,"end_line":1,"content":"A"},
            {"after_line":2,"content":"X","expect_line_content":"WRONG"}
        ],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.anchor_line 校验失败"), "{}", out);
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
    ],"allow_stale_revision":true}"#;
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
    args.push_str("],\"allow_stale_revision\":true}");
    let out = modify_file(&args, &dir, &ctx);
    assert!(out.contains("最多 16 条"), "{}", out);
    let out = modify_file(r#"{"dry_run":false,"path":"m.txt","edits":[],"allow_stale_revision":true}"#, &dir, &ctx);
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
        r#"{"dry_run":false,"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":5,"content":"X","expect_content":"L2\nL3\n\n","allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","edits":[
            {"start_line":2,"end_line":3,"content":"N1\nN2\nN3"},
            {"after_line":3,"content":"X"}
        ],"allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","edits":[
            {"start_line":2,"end_line":3,"content":""},
            {"after_line":3,"content":"X"}
        ],"allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","edits":[{"after_line":0,"content":"X","expect_line_content":"L1"}],"allow_stale_revision":true}"#,
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
        r#"{"dry_run":false,"path":"m.txt","expect_content":"L1","edits":[{"after_line":1,"content":"X"}],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    assert!(out.contains("edits[i]"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_edge_lines_match_writes() {
    // F2：长区间只锚定首尾两行即可校验，不必整段复述 `expect_content`。
    // 这里删除 L2..L4，只声明「首行是 L2、末行是 L4」。
    let dir = make_test_dir();
    let file = dir.join("edge.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\nL5\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","mode":"replace_lines","start_line":2,"end_line":4,"content":"","expect_first_line":"L2","expect_last_line":"L4","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL5\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_edge_lines_mismatch_rejects_and_relocates() {
    let dir = make_test_dir();
    let file = dir.join("edge.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 声称首行是 L2 却给了 start_line=1；实际 L2 在行 2，应拒写并给纠偏行号。
    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","mode":"replace_lines","start_line":1,"end_line":2,"content":"X","expect_first_line":"L2","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.first_line 校验失败"), "{}", out);
    assert!(out.contains("实际位于行 2"), "{}", out);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "L1\nL2\nL3\nL4\n",
        "拒写后文件必须保持原样"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_edge_lines_exclusive_with_expect_content() {
    let dir = make_test_dir();
    let file = dir.join("edge.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_content":"L1","expect_first_line":"L1","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("互斥"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_expect_edge_lines_rejected_for_insert_and_batch_insert() {
    let dir = make_test_dir();
    let file = dir.join("edge.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);

    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","mode":"insert_after_line","after_line":1,"content":"NEW","expect_first_line":"L1","allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("仅用于 replace_lines"), "{}", out);

    // 批量 edits 里的 insert 项同样拒绝（应改用 expect_line_content）。
    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","edits":[{"after_line":1,"content":"NEW","expect_first_line":"L1"}],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.anchor_line"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nL2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_modify_file_batch_edits_supports_expect_edge_lines() {
    let dir = make_test_dir();
    let file = dir.join("edge.txt");
    std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"dry_run":false,"path":"edge.txt","edits":[{"start_line":2,"end_line":3,"content":"X","expect_first_line":"L2","expect_last_line":"L3"}],"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已应用 1 条局部编辑"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\nL4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_write_tools_default_to_preview_without_dry_run() {
    // F4：写盘口径统一为「一个旋钮」——不传 `dry_run` 一律只预览，三个写工具一致。
    let dir = make_test_dir();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let f = dir.join("m.txt");
    std::fs::write(&f, "L1\nL2\n").unwrap();

    // modify_file：不传 dry_run → 预览，磁盘不变。
    let out = modify_file(
        // 预览不落盘，无需快照基线：这里刻意不传 `allow_stale_revision`，验证默认预览不会被
        // `missing_revision_baseline` 拦住（只有 `dry_run=false` 写盘才要求基线）。
        r#"{"path":"m.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("dry_run=true"), "{}", out);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "L1\nL2\n",
        "默认预览不得写盘"
    );

    // search_replace：不传 dry_run → 预览，磁盘不变。
    let out = super::mutate::search_replace(r#"{"path":"m.txt","search":"L1","replace":"Z"}"#, &dir, &ctx);
    assert!(
        out.contains("dry-run") && out.contains("设置 dry_run=false"),
        "{}",
        out
    );
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "L1\nL2\n",
        "默认预览不得写盘"
    );

    // structured_patch：不传 dry_run → 预览，磁盘不变。
    let j = dir.join("c.json");
    std::fs::write(&j, "{\"a\":1}").unwrap();
    let out = crate::cm_tools::tools::structured_data::structured_patch_try(
        r#"{"path":"c.json","query":"/a","value":2}"#,
        &dir,
        &ctx,
    )
    .unwrap_or_else(|e| panic!("预览应成功: {}", e.message));
    assert!(out.contains("dry_run") || out.contains("预览"), "{}", out);
    assert_eq!(std::fs::read_to_string(&j).unwrap(), "{\"a\":1}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 新写法（`guard` 子对象）的 runner 层正向路径：不经 `run_tool` 的 uplift，直接走守卫。
#[test]
fn test_modify_file_guard_subobject_content_match_writes() {
    let dir = make_test_dir();
    let file = dir.join("g.txt");
    std::fs::write(&file, "L1\nL2\nL3\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"dry_run":false,"path":"g.txt","mode":"replace_lines","start_line":2,"end_line":3,"content":"X","guard":{"content":"L2\nL3"},"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains("X") && !after.contains("L2") && !after.contains("L3"),
        "{after}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 空串守卫表示「校验该行为空行」，不是「未设置」——不能静默跳过（曾因过滤空串而退化）。
#[test]
fn test_modify_file_guard_empty_anchor_line_still_checks() {
    let dir = make_test_dir();
    let file = dir.join("blank.txt");
    std::fs::write(&file, "A\n\nB\n").unwrap();
    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    // 锚点行 1 实际是 "A"：声明为空行必须拒写。
    let out = modify_file(
        r#"{"dry_run":false,"path":"blank.txt","mode":"insert_after_line","after_line":1,"content":"X","guard":{"anchor_line":""},"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("guard.anchor_line 校验失败"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "A\n\nB\n");
    // 锚点行 2 确实是空行：守卫通过并写入。
    let out = modify_file(
        r#"{"dry_run":false,"path":"blank.txt","mode":"insert_after_line","after_line":2,"content":"X","guard":{"anchor_line":""},"allow_stale_revision":true}"#,
        &dir,
        &ctx,
    );
    assert!(!out.contains("校验失败"), "{}", out);
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(after.contains('X') && after.contains("A") && after.contains("B"), "{after}");
    let _ = std::fs::remove_dir_all(&dir);
}
