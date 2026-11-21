//! `GET /workspace/git` 的纯逻辑：`--porcelain=v2` 解析、状态映射、探针与缓存键。
//!
//! **与工具卡判据的有意分歧**（见 `docs/design/workspace_git_status_projection.md` §3.2）：
//! `cm_tools::tools::structured_preview` 的 `git status` 判据是「拿不准就退回纯文本」的宽松启发式
//! （服务工具卡预览，认 human-readable 输出与本机 locale 文案）；本模块是确定性真源，只认
//! `--porcelain=v2 --branch`，解析不出来就明确失败，**不以 human-readable 输出兜底**。
//! 两者不共享代码是刻意的：一边要「像不像」的容错，一边要「是或不是」的确定。
//!
//! 仓库判据用 `git rev-parse --is-inside-work-tree`，与 `git::ensure_git_repo_try` **完全同源**
//! （同一命令、同一输出值）。将来若给工具侧加额外检查，请同步这里，
//! 否则两个面会得出不同的「是不是仓库」。

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::workspace::{
    WorkspaceGitCounts, WorkspaceGitHead, WorkspaceGitSource, WorkspaceGitState,
    WorkspaceGitStatusResponse,
};

/// 单次探测的墙钟上限（`docs/design/workspace_git_status_projection.md` §10.4）。
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 缓存 TTL。**必须 ≥ [`PROBE_TIMEOUT`]**：否则慢探测的窗口长于复用窗口，
/// 同一 TTL 周期内的并发请求会各自发起新探测（fork 放大）。
pub(crate) const CACHE_TTL: Duration = Duration::from_secs(2);

/// `XY` 允许出现的字符（`--porcelain=v2` 的 index / worktree 状态位）。
const XY_CHARS: &str = "MADRCU?!.";

/// 一条 `--porcelain=v2` 输出的解析结果（不含可用性状态与时效字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Projection {
    pub head: WorkspaceGitHead,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub counts: WorkspaceGitCounts,
}

impl Projection {
    /// 「当前工作区根下」是否有未提交改动。**不**代表仓库干净（仓库根可能在工作区根之上）。
    pub(crate) fn dirty(&self) -> bool {
        self.counts.total > 0
    }
}

/// 探测结论。三种不可用态互不相同，UI 可各给一句文案（#1457 验收）。
#[derive(Clone, Debug)]
pub(crate) enum ProbeOutcome {
    Ok(Projection),
    /// 工作区根不是 git 工作树。
    NotARepo,
    /// 找不到 / 起不来 `git`。
    GitUnavailable,
    /// 其余失败，带错误码。
    Failed {
        reason_code: &'static str,
        message: String,
    },
}

/// 解析 `git status --porcelain=v2 --branch` 的 stdout。
///
/// **路径基准（实测 git 2.x）**：`git status` 在 cwd 下运行时把 path 转成**相对 cwd**——
/// 工作区根之内是 `x` / `./x`，工作区根之外是 `../x`；而它**仍会列出根外的改动**（只是加了 `../`）。
/// 所以只统计**非 `../` 开头**的记录：否则工作区根之外的路径会泄露给客户端
/// （与其它 `/workspace/*` 端点一致，返回内容不得越出工作区根）。
///
/// 解析不出来一律 `Err`：不猜、不降级、不认 human-readable 文案。
pub(crate) fn parse_porcelain_v2(stdout: &str) -> Result<Projection, String> {
    let mut base_oid: Option<String> = None;
    let mut head_name: Option<String> = None;
    let mut upstream: Option<String> = None;
    let mut ahead: Option<u32> = None;
    let mut behind: Option<u32> = None;
    let mut counts = WorkspaceGitCounts::default();

    for raw in stdout.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            // `(initial)` 表示尚无提交：字段仍存在，值为 null。
            let oid = rest.trim();
            base_oid = if oid == "(initial)" {
                None
            } else {
                Some(oid.to_string())
            };
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            head_name = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let (a, b) = parse_ahead_behind(rest)?;
            ahead = Some(a);
            behind = Some(b);
            continue;
        }
        match line.chars().next().unwrap_or(' ') {
            '1' => {
                let fields: Vec<&str> = line.splitn(9, ' ').collect();
                let (xy, path) = record_fields(&fields, 9, 8)?;
                count_change(xy, path, &mut counts)?;
            }
            '2' => {
                let fields: Vec<&str> = line.splitn(10, ' ').collect();
                let (xy, path_field) = record_fields(&fields, 10, 9)?;
                let path = path_field.split('\t').next().unwrap_or_default();
                count_change(xy, path, &mut counts)?;
            }
            'u' => {
                let fields: Vec<&str> = line.splitn(11, ' ').collect();
                let (_, path) = record_fields(&fields, 11, 10)?;
                if in_workspace(path) {
                    counts.conflicted += 1;
                    counts.total += 1;
                }
            }
            '?' => {
                let path = line
                    .strip_prefix("? ")
                    .ok_or_else(|| format!("无法解析 untracked 记录：{line}"))?;
                if in_workspace(path) {
                    counts.untracked += 1;
                    counts.total += 1;
                }
            }
            _ => return Err(format!("未识别的 porcelain=v2 记录：{line}")),
        }
    }

    let name = head_name.ok_or_else(|| "缺少 # branch.head（无法判定分支或 detached）".to_string())?;
    let head = if name == "(detached)" {
        WorkspaceGitHead::Detached { oid: base_oid }
    } else {
        WorkspaceGitHead::Branch {
            name,
            oid: base_oid,
        }
    };
    Ok(Projection {
        head,
        upstream,
        ahead,
        behind,
        counts,
    })
}

/// 取出记录行的 `(XY, path)`：`expected` 是固定字段数，`path_idx` 是 path 起始下标。
fn record_fields<'a>(
    fields: &[&'a str],
    expected: usize,
    path_idx: usize,
) -> Result<(&'a str, &'a str), String> {
    if fields.len() < expected {
        return Err(format!(
            "记录字段不足（期望 {expected}，实际 {}）",
            fields.len()
        ));
    }
    let xy = fields
        .get(1)
        .copied()
        .ok_or_else(|| "记录缺少 XY 字段".to_string())?;
    let path = fields.get(path_idx).copied().unwrap_or_default();
    Ok((xy, path))
}

/// 解析 `XY`（index 状态 + worktree 状态），校验长度与字符集。
fn parse_xy(xy: &str) -> Result<(char, char), String> {
    let mut chars = xy.chars();
    let x = chars.next().ok_or_else(|| "XY 为空".to_string())?;
    let y = chars.next().ok_or_else(|| format!("XY 不足 2 字符：{xy}"))?;
    if chars.next().is_some() {
        return Err(format!("XY 超过 2 字符：{xy}"));
    }
    if !XY_CHARS.contains(x) || !XY_CHARS.contains(y) {
        return Err(format!("XY 含未知状态字符：{xy}"));
    }
    Ok((x, y))
}

/// 按 `XY` 累计分类计数；工作区根之外的记录不增任何计数。
fn count_change(xy: &str, path: &str, counts: &mut WorkspaceGitCounts) -> Result<(), String> {
    if !in_workspace(path) {
        return Ok(());
    }
    let (x, y) = parse_xy(xy)?;
    if x != '.' {
        counts.staged += 1;
    }
    if y != '.' {
        counts.unstaged += 1;
    }
    counts.total += 1;
    Ok(())
}

/// 记录是否落在工作区根内。
///
/// git 在 cwd 下运行时把 path 转成相对 cwd：根内为 `x` / `./x`，根外为 `../x`（实测 git 2.x）。
/// 只把 `../` 开头判为根外——这是「不把工作区根之外的路径回给客户端」的唯一判据。
fn in_workspace(path: &str) -> bool {
    !(path == ".." || path.starts_with("../"))
}

/// 解析 `+<ahead> -<behind>`。
fn parse_ahead_behind(rest: &str) -> Result<(u32, u32), String> {
    let mut parts = rest.split_whitespace();
    let ahead = parts
        .next()
        .and_then(|s| s.strip_prefix('+'))
        .and_then(|s| s.parse::<u32>().ok());
    let behind = parts
        .next()
        .and_then(|s| s.strip_prefix('-'))
        .and_then(|s| s.parse::<u32>().ok());
    match (ahead, behind) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(format!("无法解析 ahead/behind：{rest}")),
    }
}

/// 同步探测：先判仓库，再取 `--porcelain=v2 --branch`。
pub(crate) fn probe(root: &Path) -> ProbeOutcome {
    let rev = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output();
    let rev = match rev {
        Ok(o) => o,
        Err(e) => return spawn_failure(&e),
    };
    if !rev.status.success() {
        return ProbeOutcome::NotARepo;
    }
    let rev_out = String::from_utf8_lossy(&rev.stdout);
    // `false`（bare 仓库等「在 git 里但没有工作树」）与 `true` 是稳定机器值，不受 locale 影响。
    if rev_out.lines().next().unwrap_or_default().trim() != "true" {
        return ProbeOutcome::NotARepo;
    }

    let status = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(root)
        .output();
    let status = match status {
        Ok(o) => o,
        Err(e) => return spawn_failure(&e),
    };
    if !status.status.success() {
        return ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_FAILED,
            message: "git status 执行失败".to_string(),
        };
    }
    let stdout = String::from_utf8_lossy(&status.stdout);
    match parse_porcelain_v2(&stdout) {
        Ok(p) => ProbeOutcome::Ok(p),
        Err(e) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_UNPARSABLE,
            message: e,
        },
    }
}

/// 子进程起不来：`NotFound` 归「git 不可用」，其余归执行失败。
fn spawn_failure(e: &std::io::Error) -> ProbeOutcome {
    if e.kind() == ErrorKind::NotFound {
        ProbeOutcome::GitUnavailable
    } else {
        ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_FAILED,
            message: format!("无法执行 git：{e}"),
        }
    }
}

/// 当前 unix 秒（探测完成时刻）。
pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 把探测结论组装为响应。`source` / `computed_at_unix` 由调用方按缓存命中情况给出。
pub(crate) fn response_from_outcome(
    outcome: ProbeOutcome,
    computed_at_unix: u64,
    source: WorkspaceGitSource,
) -> WorkspaceGitStatusResponse {
    match outcome {
        ProbeOutcome::Ok(p) => WorkspaceGitStatusResponse {
            state: WorkspaceGitState::Ok,
            reason_code: None,
            message: None,
            dirty: Some(p.dirty()),
            head: Some(p.head),
            upstream: p.upstream,
            ahead: p.ahead,
            behind: p.behind,
            counts: Some(p.counts),
            computed_at_unix,
            source,
        },
        ProbeOutcome::NotARepo => unavailable(
            WorkspaceGitState::NotARepo,
            None,
            "当前工作区不是 Git 仓库",
            computed_at_unix,
            source,
        ),
        ProbeOutcome::GitUnavailable => unavailable(
            WorkspaceGitState::GitUnavailable,
            None,
            "未找到可用的 git 可执行文件",
            computed_at_unix,
            source,
        ),
        ProbeOutcome::Failed {
            reason_code,
            message,
        } => unavailable(
            WorkspaceGitState::Failed,
            Some(reason_code.to_string()),
            &message,
            computed_at_unix,
            source,
        ),
    }
}

fn unavailable(
    state: WorkspaceGitState,
    reason_code: Option<String>,
    message: &str,
    computed_at_unix: u64,
    source: WorkspaceGitSource,
) -> WorkspaceGitStatusResponse {
    WorkspaceGitStatusResponse {
        state,
        reason_code,
        message: Some(message.to_string()),
        head: None,
        upstream: None,
        ahead: None,
        behind: None,
        dirty: None,
        counts: None,
        computed_at_unix,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD_MAIN: &str = "# branch.oid 1a2b3c4d5e6f\n# branch.head main\n";

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
        parse_porcelain_v2(stdout).expect("parse ok")
    }

    #[test]
    fn detached_head_has_no_branch_name() {
        let p = parse_ok(
            "# branch.oid 1a2b3c4d5e6f\n# branch.head (detached)\n",
        );
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
        let p = parse_ok("# branch.oid (initial)\n# branch.head main\n");
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
            "# branch.oid aa\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n",
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
    }

    #[test]
    fn untracked_only() {
        let p = parse_ok(&format!("{HEAD_MAIN}? a.txt\n"));
        assert_eq!(p.counts.untracked, 1);
        assert_eq!(p.counts.total, 1);
        assert_eq!((p.counts.staged, p.counts.unstaged), (0, 0));
        assert!(p.dirty());
    }

    #[test]
    fn staged_only() {
        let p = parse_ok(&format!("{HEAD_MAIN}1 A. N... 000000 100644 100644 abc def a.txt\n"));
        assert_eq!(p.counts.staged, 1);
        assert_eq!(p.counts.unstaged, 0);
        assert_eq!(p.counts.total, 1);
    }

    #[test]
    fn same_file_counts_in_both_buckets_but_one_total() {
        let p = parse_ok(&format!("{HEAD_MAIN}1 MM N... 100644 100644 100644 abc def a.txt\n"));
        assert_eq!(p.counts.staged, 1);
        assert_eq!(p.counts.unstaged, 1);
        assert_eq!(p.counts.total, 1, "同一文件只算一行");
    }

    #[test]
    fn rename_record_counts_once_and_drops_orig_path() {
        let p = parse_ok(
            &format!("{HEAD_MAIN}2 R. N... 100644 100644 100644 abc def R100 new name.txt\told name.txt\n"),
        );
        assert_eq!(p.counts.staged, 1);
        assert_eq!(p.counts.total, 1);
    }

    #[test]
    fn conflicted_record() {
        let p = parse_ok(&format!("{HEAD_MAIN}u UU N... 100644 100644 100644 100644 a b c a.txt\n"));
        assert_eq!(p.counts.conflicted, 1);
        assert_eq!(p.counts.total, 1);
    }

    #[test]
    fn path_with_spaces_kept_whole() {
        let p = parse_ok(&format!("{HEAD_MAIN}? my file.txt\n"));
        assert_eq!(p.counts.untracked, 1);
        assert_eq!(p.counts.total, 1);
    }

    #[test]
    fn crlf_and_trailing_blank_lines_tolerated() {
        let p = parse_ok("# branch.oid aa\r\n# branch.head main\r\n? a.txt\r\n\r\n");
        assert_eq!(p.counts.untracked, 1);
    }

    #[test]
    fn missing_branch_head_is_error() {
        let e = parse_porcelain_v2("# branch.oid aa\n").expect_err("must fail");
        assert!(e.contains("branch.head"), "{e}");
    }

    #[test]
    fn garbage_line_is_error() {
        assert!(parse_porcelain_v2(&format!("{HEAD_MAIN}zzz\n")).is_err());
    }

    #[test]
    fn short_or_unknown_xy_is_error() {
        assert!(parse_porcelain_v2(&format!("{HEAD_MAIN}1 M N... 1 2 3 4 5 a.txt\n")).is_err());
        assert!(
            parse_porcelain_v2(&format!("{HEAD_MAIN}1 ZZ N... 1 2 3 4 5 a.txt\n")).is_err()
        );
    }

    #[test]
    fn record_with_missing_fields_is_error() {
        assert!(parse_porcelain_v2(&format!("{HEAD_MAIN}1 A.\n")).is_err());
        assert!(parse_porcelain_v2(&format!("{HEAD_MAIN}u UU N...\n")).is_err());
    }

    #[test]
    fn path_with_newline_fails() {
        // 含 `\n` 的 path 会把记录切成两行，第二行首字符不是记录类型 ⇒ 明确失败，不猜。
        assert!(parse_porcelain_v2(&format!("{HEAD_MAIN}? a\nb.txt\n")).is_err());
    }

    #[test]
    fn records_outside_workspace_root_are_skipped() {
        // git 在子目录 cwd 下把根外路径写成 `../x`（实测），这类记录必须丢弃。
        let p = parse_ok(&format!("{HEAD_MAIN}? ../outside.txt\n"));
        assert_eq!(p.counts.total, 0);
        assert!(!p.dirty());
    }

    #[test]
    fn records_inside_workspace_root_are_counted() {
        // 根内路径是相对 cwd 的 `x` 或 `./`（整个 untracked 目录会被折叠成一条 `? ./`）。
        for line in ["? inside.txt\n", "? ./\n"] {
            let p = parse_ok(&format!("{HEAD_MAIN}{line}"));
            assert_eq!(p.counts.untracked, 1, "{line}");
            assert_eq!(p.counts.total, 1, "{line}");
            assert!(p.dirty());
        }
    }

    #[test]
    fn outcome_mapping_keeps_states_distinct() {
        let not_repo = response_from_outcome(
            ProbeOutcome::NotARepo,
            7,
            WorkspaceGitSource::Probe,
        );
        let unavailable = response_from_outcome(
            ProbeOutcome::GitUnavailable,
            7,
            WorkspaceGitSource::Probe,
        );
        let failed = response_from_outcome(
            ProbeOutcome::Failed {
                reason_code: error_codes::GIT_PROBE_TIMEOUT,
                message: "超时".to_string(),
            },
            7,
            WorkspaceGitSource::StaleCache,
        );
        assert_eq!(not_repo.state, WorkspaceGitState::NotARepo);
        assert_eq!(unavailable.state, WorkspaceGitState::GitUnavailable);
        assert_eq!(failed.state, WorkspaceGitState::Failed);
        assert_eq!(
            failed.reason_code.as_deref(),
            Some(error_codes::GIT_PROBE_TIMEOUT)
        );
        // 三种不可用态都不带 head / counts，避免客户端把空值当成「干净」。
        for r in [&not_repo, &unavailable, &failed] {
            assert!(r.head.is_none());
            assert!(r.counts.is_none());
            assert!(r.dirty.is_none());
        }
    }

    #[test]
    fn ok_outcome_carries_dirty_and_counts() {
        let p = parse_ok(&format!("{HEAD_MAIN}? a.txt\n"));
        let r = response_from_outcome(ProbeOutcome::Ok(p), 42, WorkspaceGitSource::Cache);
        assert_eq!(r.state, WorkspaceGitState::Ok);
        assert_eq!(r.dirty, Some(true));
        assert_eq!(r.counts.map(|c| c.untracked), Some(1));
        assert_eq!(r.computed_at_unix, 42);
        assert_eq!(r.source, WorkspaceGitSource::Cache);
    }
}
