//! `git_projection` 的单测：`-z` 解析、形状钉死、清单与 `counts` 的不变式。
//!
//! 单独成文件是为了让 `git_projection.rs` 留在 fn-nloc 门禁的文件行数上限内
//! （与 `handlers_git_http_tests.rs` 同一口径）。

use super::git_projection::*;
use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::workspace::{
    WorkspaceGitCounts, WorkspaceGitFileKind, WorkspaceGitHead, WorkspaceGitSource,
    WorkspaceGitState, WorkspaceGitUntrackedMode,
};

/// `-z` 分帧：每条记录（含 `# branch.*` 头行）都以 NUL 结尾。
const HEAD_MAIN: &str = "# branch.oid 1a2b3c4d5e6f\0# branch.head main\0";

/// 设计约束：探测超时 ≤ 缓存 TTL（设计文档 §10.4）。
///
/// 反了就出问题：慢探测的窗口长于复用窗口，同一 TTL 内的并发请求会各自 fork 两条 git 命令。
/// 改任一个值都要先想清楚另一侧。
#[test]
fn probe_timeout_must_not_exceed_cache_ttl() {
    assert!(
        PROBE_TIMEOUT <= CACHE_TTL,
        "PROBE_TIMEOUT({PROBE_TIMEOUT:?}) 必须 ≤ CACHE_TTL({CACHE_TTL:?})"
    );
}

fn parse_ok(stdout: &str) -> Projection {
    parse_porcelain_v2(stdout.as_bytes(), "").expect("parse ok")
}

/// 带工作区前缀的解析（工作区根是仓库子目录）。
fn parse_in_subdir(stdout: &str, prefix: &str) -> Projection {
    parse_porcelain_v2(stdout.as_bytes(), prefix).expect("parse ok")
}

/// 形状钉死（#1498）：宿主 `~/.gitconfig` 与仓库配置都不得影响判据。
///
/// 这条断言是「改形状必须显式改测试」的闸门——尤其 `-z`
/// （缺它 → 非 ASCII 路径被八进制转义，且 path 基准从仓库根相对退回 cwd 相对）。
/// `status.relativePaths` 不在此列：`-z` 下它失效，裁剪改由 `rev-parse --show-prefix` 承担。
#[test]
fn status_argv_pins_shape_config() {
    assert_eq!(
        status_argv(UntrackedMode::Folded),
        vec![
            "-c",
            "core.quotePath=false",
            "-c",
            "status.renames=true",
            "-c",
            "core.fileMode=true",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ]
    );
}

/// 契约不变式：未截断时「各桶求和 == 条目数 == `counts.total`」。
///
/// 这是 issue #1498「同源同口径」的可执行判据——计数与列表由 [`Accumulator::push`]
/// 同一次写入，所以它对任何输入都必须成立。
#[test]
fn file_list_matches_counts_invariant() {
    let s = format!(
        "{HEAD_MAIN}1 A. N... 000000 100644 100644 a b staged.txt\0\
         1 .M N... 100644 100644 100644 a b unstaged.txt\0\
         1 MM N... 100644 100644 100644 a b both.txt\0\
         u UU N... 100644 100644 100644 100644 a b c conflict.txt\0\
         ? untracked.txt\0"
    );
    let p = parse_ok(&s);
    let items = &p.files.items;
    assert_eq!(items.len() as u32, p.counts.total);
    assert_eq!(
        items.iter().filter(|e| e.staged).count() as u32,
        p.counts.staged
    );
    assert_eq!(
        items.iter().filter(|e| e.unstaged).count() as u32,
        p.counts.unstaged
    );
    assert_eq!(
        items.iter().filter(|e| e.untracked).count() as u32,
        p.counts.untracked
    );
    assert_eq!(
        items.iter().filter(|e| e.conflicted).count() as u32,
        p.counts.conflicted
    );
    assert!(!p.files.truncated, "未超限不应截断");
}

#[test]
fn entries_are_sorted_by_path() {
    let p = parse_ok(&format!("{HEAD_MAIN}? b.txt\0? a.txt\0? C.txt\0"));
    let paths: Vec<&str> = p.files.items.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["C.txt", "a.txt", "b.txt"], "字节序升序：{paths:?}");
}

/// 超限：计数照常累加，列表停下并置 `truncated`（不许静默截断）。
#[test]
fn truncation_sets_flag_and_caps_items() {
    let mut s = String::from(HEAD_MAIN);
    for i in 0..(MAX_LISTED_FILES + 1) {
        s.push_str(&format!("? f{i}.txt\0"));
    }
    let p = parse_ok(&s);
    assert_eq!(p.counts.untracked, MAX_LISTED_FILES + 1);
    assert_eq!(p.counts.total, MAX_LISTED_FILES + 1);
    assert_eq!(p.files.items.len() as u32, MAX_LISTED_FILES);
    assert!(p.files.truncated, "超限必须带截断标志");
    assert_eq!(p.files.limit, MAX_LISTED_FILES);
}

/// 折叠的未跟踪目录：一条 `kind=dir`，`path` 去尾斜杠。
#[test]
fn collapsed_untracked_dir_becomes_dir_entry() {
    let p = parse_ok(&format!("{HEAD_MAIN}? newdir/\0"));
    assert_eq!(p.files.items.len(), 1);
    assert_eq!(p.files.items[0].path, "newdir");
    assert_eq!(p.files.items[0].kind, WorkspaceGitFileKind::Dir);
    assert!(p.files.items[0].untracked);
    assert_eq!(p.counts.untracked, 1, "折叠目录仍只算一条");
}

/// 整个工作区根都未跟踪：git 折叠成 `? ./`。
#[test]
fn whole_root_untracked_dir_entry_is_dot() {
    let p = parse_ok(&format!("{HEAD_MAIN}? ./\0"));
    assert_eq!(p.files.items.len(), 1);
    assert_eq!(p.files.items[0].path, ".");
    assert_eq!(p.files.items[0].kind, WorkspaceGitFileKind::Dir);
}

/// `-z` 下路径原样输出：非 ASCII / 空格不做转义（`core.quotePath=false` 双保险）。
#[test]
fn non_ascii_path_kept_verbatim() {
    let p = parse_ok(&format!("{HEAD_MAIN}? 中文 名.txt\0"));
    assert_eq!(p.files.items[0].path, "中文 名.txt");
    assert_eq!(p.files.items[0].kind, WorkspaceGitFileKind::File);
}

/// `-z` 下换行不再是记录分隔符：含 `\n` 的路径原样保留（旧实现会判 UNPARSABLE）。
#[test]
fn path_with_newline_is_kept_whole() {
    let p = parse_ok(&format!("{HEAD_MAIN}? nl\nname.txt\0"));
    assert_eq!(p.files.items.len(), 1);
    assert_eq!(p.files.items[0].path, "nl\nname.txt");
}

#[test]
fn detached_head_has_no_branch_name() {
    let p = parse_ok("# branch.oid 1a2b3c4d5e6f\0# branch.head (detached)\0");
    assert_eq!(
        p.head,
        WorkspaceGitHead::Detached {
            oid: Some("1a2b3c4d5e6f".to_string())
        }
    );
    // detached 序列化后不得出现 name 键。
    let v = serde_json::to_value(&p.head).expect("serialize head");
    assert_eq!(v["kind"], "detached");
    assert!(v.get("name").is_none(), "detached 不应有 name：{v}");
}

#[test]
fn branch_head_carries_name_and_oid() {
    let p = parse_ok(HEAD_MAIN);
    let v = serde_json::to_value(&p.head).expect("serialize head");
    assert_eq!(v["kind"], "branch");
    assert_eq!(v["name"], "main");
    assert_eq!(v["oid"], "1a2b3c4d5e6f");
}

#[test]
fn initial_commit_keeps_oid_null() {
    let p = parse_ok("# branch.oid (initial)\0# branch.head main\0");
    let v = serde_json::to_value(&p.head).expect("serialize head");
    assert_eq!(v["oid"], serde_json::Value::Null, "oid 键必须存在且为 null");
}

#[test]
fn missing_upstream_leaves_ab_null() {
    let p = parse_ok(HEAD_MAIN);
    assert!(p.upstream.is_none());
    assert!(p.ahead.is_none());
    assert!(p.behind.is_none());
}

#[test]
fn ahead_behind_parsed() {
    let p = parse_ok(
        "# branch.oid aa\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\0",
    );
    assert_eq!(p.upstream.as_deref(), Some("origin/main"));
    assert_eq!(p.ahead, Some(2));
    assert_eq!(p.behind, Some(3));
}

#[test]
fn clean_worktree_is_not_dirty() {
    let p = parse_ok(HEAD_MAIN);
    assert_eq!(p.counts, WorkspaceGitCounts::default());
    assert!(!p.dirty());
    assert!(p.files.items.is_empty(), "干净工作区没有条目");
    assert!(!p.files.truncated);
    assert_eq!(p.files.limit, MAX_LISTED_FILES);
}

#[test]
fn untracked_only() {
    let p = parse_ok(&format!("{HEAD_MAIN}? a.txt\0"));
    assert_eq!(p.counts.untracked, 1);
    assert_eq!(p.counts.total, 1);
    assert_eq!((p.counts.staged, p.counts.unstaged), (0, 0));
    assert!(p.dirty());
    assert_eq!(p.files.items.len(), 1);
    assert!(p.files.items[0].untracked);
    assert_eq!(p.files.items[0].path, "a.txt");
    assert_eq!(p.files.items[0].kind, WorkspaceGitFileKind::File);
}

#[test]
fn staged_only() {
    let p = parse_ok(&format!(
        "{HEAD_MAIN}1 A. N... 000000 100644 100644 abc def a.txt\0"
    ));
    assert_eq!(p.counts.staged, 1);
    assert_eq!(p.counts.unstaged, 0);
    assert_eq!(p.counts.total, 1);
    assert!(p.files.items[0].staged);
    assert!(!p.files.items[0].unstaged);
}

#[test]
fn same_file_counts_in_both_buckets_but_one_total() {
    let p = parse_ok(&format!(
        "{HEAD_MAIN}1 MM N... 100644 100644 100644 abc def a.txt\0"
    ));
    assert_eq!(p.counts.staged, 1);
    assert_eq!(p.counts.unstaged, 1);
    assert_eq!(p.counts.total, 1, "同一文件只算一行");
    assert_eq!(p.files.items.len(), 1, "但列表里也只有一条");
    assert!(p.files.items[0].staged && p.files.items[0].unstaged);
}

/// `-z` 下 rename 的原路径是**下一段**：必须被吃掉，且只入列一条。
///
/// 后半段那个 `?` 记录是关键：吃掉逻辑若少走一步，它会被当成原路径吞掉而漏计。
#[test]
fn rename_record_counts_once_and_swallows_orig_segment() {
    let p = parse_ok(&format!(
        "{HEAD_MAIN}2 R. N... 100644 100644 100644 abc def R100 new name.txt\0old name.txt\0? after.txt\0"
    ));
    assert_eq!(p.counts.staged, 1);
    assert_eq!(p.counts.total, 2, "rename 一条 + untracked 一条");
    assert_eq!(p.counts.untracked, 1, "orig 段不得吞掉后续记录");
    assert_eq!(p.files.items.len(), 2);
    let rename = p
        .files
        .items
        .iter()
        .find(|e| e.path == "new name.txt")
        .expect("rename 目的地入列");
    assert!(rename.staged);
    assert!(
        p.files.items.iter().all(|e| e.path != "old name.txt"),
        "原路径不入列：{:?}",
        p.files.items
    );
}

#[test]
fn conflicted_record() {
    let p = parse_ok(&format!(
        "{HEAD_MAIN}u UU N... 100644 100644 100644 100644 a b c a.txt\0"
    ));
    assert_eq!(p.counts.conflicted, 1);
    assert_eq!(p.counts.total, 1);
    assert!(p.files.items[0].conflicted);
}

#[test]
fn path_with_spaces_kept_whole() {
    let p = parse_ok(&format!("{HEAD_MAIN}? my file.txt\0"));
    assert_eq!(p.counts.untracked, 1);
    assert_eq!(p.counts.total, 1);
    assert_eq!(p.files.items[0].path, "my file.txt");
}

#[test]
fn trailing_nul_and_blank_segments_tolerated() {
    // `-z` 下记录不含 CR/LF；末尾 NUL 会切出空段，必须无害。
    let p = parse_ok("# branch.oid aa\0# branch.head main\0? a.txt\0\0");
    assert_eq!(p.counts.untracked, 1);
}

#[test]
fn missing_branch_head_is_error() {
    let e = parse_porcelain_v2(b"# branch.oid aa\0", "").expect_err("must fail");
    assert!(e.contains("branch.head"), "{e}");
}

#[test]
fn garbage_line_is_error() {
    assert!(parse_porcelain_v2(format!("{HEAD_MAIN}zzz\0").as_bytes(), "").is_err());
}

#[test]
fn short_or_unknown_xy_is_error() {
    assert!(
        parse_porcelain_v2(format!("{HEAD_MAIN}1 M N... 1 2 3 4 5 a.txt\0").as_bytes(), "")
            .is_err()
    );
    assert!(
        parse_porcelain_v2(format!("{HEAD_MAIN}1 ZZ N... 1 2 3 4 5 a.txt\0").as_bytes(), "")
            .is_err()
    );
}

#[test]
fn record_with_missing_fields_is_error() {
    assert!(parse_porcelain_v2(format!("{HEAD_MAIN}1 A.\0").as_bytes(), "").is_err());
    assert!(parse_porcelain_v2(format!("{HEAD_MAIN}u UU N...\0").as_bytes(), "").is_err());
}

/// 整条记录就是工作区根自身（剥前缀后为空）⇒ 归一为 `.` + `dir`，不是错误。
#[test]
fn root_self_record_becomes_dot() {
    let p = parse_in_subdir(&format!("{HEAD_MAIN}? sub/\0"), "sub/");
    assert_eq!(p.files.items.len(), 1);
    assert_eq!(p.files.items[0].path, ".");
    assert_eq!(p.files.items[0].kind, WorkspaceGitFileKind::Dir);
    assert_eq!(p.counts.untracked, 1);
}

/// 工作区根是仓库子目录时：不以 `sub/` 为前缀的记录属于根外，必须丢弃。
#[test]
fn records_outside_workspace_root_are_skipped() {
    let p = parse_in_subdir(
        &format!("{HEAD_MAIN}? outside.txt\0? sub2/inside.txt\0? sub/kept.txt\0"),
        "sub/",
    );
    assert_eq!(p.counts.total, 1, "只有 sub/ 下的记录计入");
    assert_eq!(p.counts.untracked, 1);
    let paths: Vec<&str> = p.files.items.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["kept.txt"], "回的是根相对路径，根外不出现");
}

/// 前缀边界必须按目录算：`sub/` 不得匹配 `sub2/`（尾斜杠被剥掉就会误判）。
#[test]
fn prefix_boundary_is_a_directory() {
    let p = parse_in_subdir(&format!("{HEAD_MAIN}? sub2/x.txt\0"), "sub/");
    assert_eq!(p.counts.total, 0);
    assert!(p.files.items.is_empty());
}

#[test]
fn records_inside_workspace_root_are_counted() {
    // 工作区根就是仓库根时前缀为空：普通路径与折叠目录都算根内。
    for line in ["? inside.txt\0", "? newdir/\0"] {
        let p = parse_ok(&format!("{HEAD_MAIN}{line}"));
        assert_eq!(p.counts.untracked, 1, "{line}");
        assert_eq!(p.counts.total, 1, "{line}");
        assert!(p.dirty());
        assert_eq!(p.files.items.len(), 1, "{line}");
    }
}

#[test]
fn outcome_mapping_keeps_states_distinct() {
    let not_repo = response_from_outcome(
        ProbeOutcome::NotARepo,
        7,
        WorkspaceGitSource::Probe,
        UntrackedMode::Folded,
    );
    let unavailable = response_from_outcome(
        ProbeOutcome::GitUnavailable,
        7,
        WorkspaceGitSource::Probe,
        UntrackedMode::Folded,
    );
    let failed = response_from_outcome(
        ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_TIMEOUT,
            message: "超时".to_string(),
        },
        7,
        WorkspaceGitSource::StaleCache,
        UntrackedMode::Folded,
    );
    assert_eq!(not_repo.state, WorkspaceGitState::NotARepo);
    assert_eq!(unavailable.state, WorkspaceGitState::GitUnavailable);
    assert_eq!(failed.state, WorkspaceGitState::Failed);
    assert_eq!(
        failed.reason_code.as_deref(),
        Some(error_codes::GIT_PROBE_TIMEOUT)
    );
    // 三种不可用态都不带 head / counts / files，避免客户端把空值当成「干净」。
    for r in [&not_repo, &unavailable, &failed] {
        assert!(r.head.is_none());
        assert!(r.counts.is_none());
        assert!(r.dirty.is_none());
        assert!(r.files.is_none(), "不可用态不给列表");
        assert_eq!(r.untracked_mode, WorkspaceGitUntrackedMode::Folded);
    }
}

#[test]
fn ok_outcome_carries_dirty_and_counts() {
    let p = parse_ok(&format!("{HEAD_MAIN}? a.txt\0"));
    let r = response_from_outcome(
        ProbeOutcome::Ok(p),
        42,
        WorkspaceGitSource::Cache,
        UntrackedMode::Folded,
    );
    assert_eq!(r.state, WorkspaceGitState::Ok);
    assert_eq!(r.dirty, Some(true));
    assert_eq!(r.counts.map(|c| c.untracked), Some(1));
    assert_eq!(r.computed_at_unix, 42);
    assert_eq!(r.source, WorkspaceGitSource::Cache);
    // 探测结论里列表恒被填入，是否对外可见由 handler 决定。
    let files = r.files.expect("ok 态应带列表");
    assert_eq!(files.items.len(), 1);
    assert_eq!(r.untracked_mode, WorkspaceGitUntrackedMode::Folded);
}
