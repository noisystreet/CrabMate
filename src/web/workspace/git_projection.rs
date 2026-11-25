//! `GET /workspace/git` 的纯逻辑：`--porcelain=v2` 解析、状态映射、探针与缓存键。
//!
//! **与工具卡判据的有意分歧**（见 `docs/design/workspace_git_status_projection.md` §3.2）：
//! `cm_tools::tools::structured_preview` 的 `git status` 判据是「拿不准就退回纯文本」的宽松启发式
//! （服务工具卡预览，认 human-readable 输出与本机 locale 文案）；本模块是确定性真源，只认
//! `--porcelain=v2 --branch`，解析不出来就明确失败，**不以 human-readable 输出兜底**。
//! 两者不共享**判据**是刻意的（一边要「像不像」，一边要「是或不是」）；可共享的只是
//! 「argv 钉死 + NUL 分帧 + 记录解析」这层机械工作。
//!
//! 仓库判据用 `git rev-parse --is-inside-work-tree`，与 `git::ensure_git_repo_try` **完全同源**
//! （同一命令、同一输出值）。将来若给工具侧加额外检查，请同步这里，
//! 否则两个面会得出不同的「是不是仓库」。
//!
//! **#1498**：本条投影要出文件清单，取数改成 `-z`（NUL 分帧 + 路径原样输出），并把影响输出
//! **形状**的 git 配置全部 `-c` 钉死（[`SHAPE_PINNING_ARGS`]）——宿主的 `core.quotePath` /
//! `status.relativePaths` / `status.renames` / `core.fileMode` 一个都不许进判据。
//! 计数与清单由同一条记录派生（[`Accumulator::push`]），「计数说 3、列表给 2」在结构上不可能。

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cm_api_contract::error_codes;
use crate::cm_api_contract::workspace::{
    WorkspaceGitCounts, WorkspaceGitFileEntry, WorkspaceGitFileKind, WorkspaceGitFileList,
    WorkspaceGitHead, WorkspaceGitSource, WorkspaceGitState, WorkspaceGitStatusResponse,
    WorkspaceGitUntrackedMode,
};

/// 单次探测的墙钟上限（`docs/design/workspace_git_status_projection.md` §10.4）。
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 缓存 TTL。**必须 ≥ [`PROBE_TIMEOUT`]**：否则慢探测的窗口长于复用窗口，
/// 同一 TTL 周期内的并发请求会各自发起新探测（fork 放大）。
pub(crate) const CACHE_TTL: Duration = Duration::from_secs(2);

/// `XY` 允许出现的字符（`--porcelain=v2` 的 index / worktree 状态位）。
const XY_CHARS: &str = "MADRCU?!.";

/// 文件清单的条数上限。
///
/// 超限时**计数照常累加**，只有列表停止增长并置 `truncated`——绝不允许静默截断后让 UI
/// 以为那就是全部。不做客户端可调的 `?limit`：那等于把响应体上限交给调用方。
pub(crate) const MAX_LISTED_FILES: u32 = 500;

/// 影响 `--porcelain=v2` 输出**形状**的 git 配置，全部显式钉死（不继承宿主 / 仓库配置）。
///
/// 逐条理由见设计文档 §12.1，实测 git 2.47.3：
/// - `core.quotePath=false`：默认值 `true` 会把非 ASCII 路径输出成 `"\344\270\255…"`
///   （C 风格引号 + 八进制转义）。只出计数时无害，出列表就是乱码。
/// - `status.renames=true`：`git mv` 保持一条 `2` 记录。设 `false` 会拆成 `1 AM` + `1 D.`
///   两条，`counts.total` 直接翻倍。
/// - `core.fileMode=true`：`chmod +x` 计为改动，与 Linux 默认一致（Windows 侧也钉死）。
///
/// **`status.relativePaths` 刻意不在此列**：加 `-z` 之后 porcelain v2 的 path 恒为**仓库根相对**
/// （实测：设 `true` 也不会回到 cwd 相对），所以根内/根外的裁剪改由 [`workspace_root_prefix`]
/// 剥前缀实现，不再依赖这个能被宿主改掉的配置。
pub(crate) const SHAPE_PINNING_ARGS: &[&str] = &[
    "-c",
    "core.quotePath=false",
    "-c",
    "status.renames=true",
    "-c",
    "core.fileMode=true",
];

/// 未跟踪目录的取数口径。
///
/// 当前只有折叠一种（与 `counts.untracked` 同口径）。将来若要「展开成文件」
/// （`--untracked-files=all`，会把 `counts` 从条目数变成文件数），在这里加变体 +
/// [`untracked_flag`] 加一个臂，并让缓存 key 带上它——那时的 `counts` 语义变化必须同步改
/// 契约与文档，不能只改 argv。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UntrackedMode {
    /// git 默认：整个未跟踪目录折叠成一条 `? dir/` 记录。
    Folded,
}

/// 内部取数模式 → 契约回显值。
pub(crate) fn contract_mode(mode: UntrackedMode) -> WorkspaceGitUntrackedMode {
    match mode {
        UntrackedMode::Folded => WorkspaceGitUntrackedMode::Folded,
    }
}

/// 内部取数模式 → `git status` 的 `--untracked-files=` 取值。
fn untracked_flag(mode: UntrackedMode) -> &'static str {
    match mode {
        UntrackedMode::Folded => "--untracked-files=normal",
    }
}

/// `git status` 的完整 argv —— **形状的唯一定义处**。
///
/// 探测与单测断言共用它：想改输出形状就必须改这里 **和** `status_argv_pins_shape_config`，
/// 不能靠谁顺手往 `Command::args()` 里塞一个参数。
pub(crate) fn status_argv(mode: UntrackedMode) -> Vec<&'static str> {
    let mut argv = SHAPE_PINNING_ARGS.to_vec();
    argv.extend([
        "status",
        "--porcelain=v2",
        "--branch",
        // `-z`：NUL 分帧 + 路径原样输出（含换行 / 引号的文件名也不会破帧、不会被转义）。
        // 副作用（实测）：path 变成**仓库根相对**，`status.relativePaths` 随之失效
        // —— 裁剪因此改由 [`workspace_root_prefix`] 剥前缀，见 SHAPE_PINNING_ARGS 的说明。
        // 另外它**不**隐含 `--untracked-files=all`，所以下一行显式钉住折叠口径。
        "-z",
        untracked_flag(mode),
    ]);
    argv
}

/// 一条 `--porcelain=v2` 输出的解析结果（不含可用性状态与时效字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Projection {
    pub head: WorkspaceGitHead,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub counts: WorkspaceGitCounts,
    /// 未提交改动的文件清单。缓存里**永远是全量**（见 `handlers_git::with_files_visibility`），
    /// 是否随响应返回由 `?files` 决定——这样 `counts` 与 `files` 必出自同一次探测。
    pub files: WorkspaceGitFileList,
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

/// 一条记录归一化的结果。
///
/// 四个布尔与 [`WorkspaceGitCounts`] 的四桶**一一对应**，且计数与清单由同一个
/// [`Accumulator::push`] 写入——「同源同口径」是结构保证，不是两次小心翼翼的对齐。
struct Record {
    path: String,
    kind: WorkspaceGitFileKind,
    staged: bool,
    unstaged: bool,
    untracked: bool,
    conflicted: bool,
}

impl Record {
    /// `1` / `2`（普通变更 / rename-copy）记录：`XY` 拆 staged 与 unstaged。
    fn change(xy: &str, raw_path: &str) -> Result<Self, String> {
        let (x, y) = parse_xy(xy)?;
        let (path, kind) = checked_path(raw_path);
        Ok(Self {
            path,
            kind,
            staged: x != '.',
            unstaged: y != '.',
            untracked: false,
            conflicted: false,
        })
    }

    /// `?` 记录。
    fn untracked(raw_path: &str) -> Self {
        let (path, kind) = checked_path(raw_path);
        Self {
            path,
            kind,
            staged: false,
            unstaged: false,
            untracked: true,
            conflicted: false,
        }
    }

    /// `u`（unmerged）记录。
    fn conflicted(raw_path: &str) -> Self {
        let (path, kind) = checked_path(raw_path);
        Self {
            path,
            kind,
            staged: false,
            unstaged: false,
            untracked: false,
            conflicted: true,
        }
    }
}

/// 解析过程中的累加器：头 / 上游信息 + 计数 + 清单，**计数与清单同步写入**。
#[derive(Default)]
struct Accumulator {
    head_oid: Option<String>,
    head_name: Option<String>,
    upstream: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    counts: WorkspaceGitCounts,
    files: Vec<WorkspaceGitFileEntry>,
    truncated: bool,
}

impl Accumulator {
    /// 累加一条记录：`counts` 与 `files` 的唯一写入口。
    ///
    /// 顺序很重要——**先计数、再决定是否入列**：超过 [`MAX_LISTED_FILES`] 之后计数照常走，
    /// 只有列表停止增长并置 `truncated`，于是 `counts.total > items.len()` 才成立。
    fn push(&mut self, rec: Record) {
        if rec.staged {
            self.counts.staged += 1;
        }
        if rec.unstaged {
            self.counts.unstaged += 1;
        }
        if rec.untracked {
            self.counts.untracked += 1;
        }
        if rec.conflicted {
            self.counts.conflicted += 1;
        }
        self.counts.total += 1;

        if self.files.len() < MAX_LISTED_FILES as usize {
            self.files.push(WorkspaceGitFileEntry {
                path: rec.path,
                kind: rec.kind,
                staged: rec.staged,
                unstaged: rec.unstaged,
                untracked: rec.untracked,
                conflicted: rec.conflicted,
            });
        } else {
            self.truncated = true;
        }
    }
}

/// 解析 `git status --porcelain=v2 --branch -z` 的 stdout。
///
/// **分帧（#1498）**：`-z` 下记录以 NUL 结尾，`2`（rename/copy）记录的两条路径也由 NUL 分隔
/// （不再是 `\t` 后缀），所以这里是「按 NUL 切 → 逐段解释」，不是按行。
/// 入参是 `&[u8]` 而非 `&str`：`-z` 下路径可能是任意字节，整段 lossy 会先在字节边界上赌运气，
/// 故只对单条记录做 lossy（非 UTF-8 文件名呈现为 U+FFFD，见 `docs/待办清单.md`）。
///
/// **路径基准（实测 git 2.47.3）**：加 `-z` 后 porcelain v2 的 path 恒为**仓库根相对**
/// （此时 `status.relativePaths` 不起作用，设 `true` 也不回到 cwd 相对），且**仍会列出
/// 工作区根之外的改动**。`prefix` 是工作区根在仓库内的前缀（[`workspace_root_prefix`]），
/// [`root_relative`] 用它剥出根相对路径、丢弃不以它为前缀的记录——否则工作区根之外的
/// 路径会泄露给客户端（与其它 `/workspace/*` 端点一致，返回内容不得越出工作区根）。
///
/// 解析不出来一律 `Err`：不猜、不降级、不认 human-readable 文案。
pub(crate) fn parse_porcelain_v2(stdout: &[u8], prefix: &str) -> Result<Projection, String> {
    let mut acc = Accumulator::default();
    let segments: Vec<&[u8]> = stdout.split(|b| *b == 0).collect();
    let mut idx = 0;

    while idx < segments.len() {
        let raw = segments[idx];
        idx += 1;
        // 末尾 NUL 会切出一个空段；空记录无害，跳过（非空垃圾仍走下面的 `_ =>` 失败）。
        if raw.is_empty() {
            continue;
        }
        let line = String::from_utf8_lossy(raw);

        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            // `(initial)` 表示尚无提交：字段仍存在，值为 null。
            let oid = rest.trim();
            acc.head_oid = if oid == "(initial)" {
                None
            } else {
                Some(oid.to_string())
            };
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            acc.head_name = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            acc.upstream = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let (a, b) = parse_ahead_behind(rest)?;
            acc.ahead = Some(a);
            acc.behind = Some(b);
            continue;
        }

        match line.chars().next().unwrap_or(' ') {
            '1' => {
                let fields: Vec<&str> = line.splitn(9, ' ').collect();
                let (xy, path) = record_fields(&fields, 9, 8)?;
                if let Some(rel) = root_relative(path, prefix)? {
                    acc.push(Record::change(xy, rel)?);
                }
            }
            '2' => {
                let fields: Vec<&str> = line.splitn(10, ' ').collect();
                let (xy, path) = record_fields(&fields, 10, 9)?;
                // `-z` 下原路径是**下一段**：吃掉它（列表只放目的地，`total` 也只 +1）。
                idx += 1;
                if let Some(rel) = root_relative(path, prefix)? {
                    acc.push(Record::change(xy, rel)?);
                }
            }
            'u' => {
                let fields: Vec<&str> = line.splitn(11, ' ').collect();
                let (_, path) = record_fields(&fields, 11, 10)?;
                if let Some(rel) = root_relative(path, prefix)? {
                    acc.push(Record::conflicted(rel));
                }
            }
            '?' => {
                let path = line
                    .strip_prefix("? ")
                    .ok_or_else(|| format!("无法解析 untracked 记录：{line}"))?;
                if let Some(rel) = root_relative(path, prefix)? {
                    acc.push(Record::untracked(rel));
                }
            }
            _ => return Err(format!("未识别的 porcelain=v2 记录：{line}")),
        }
    }

    let name = acc
        .head_name
        .take()
        .ok_or_else(|| "缺少 # branch.head（无法判定分支或 detached）".to_string())?;
    let head = if name == "(detached)" {
        WorkspaceGitHead::Detached {
            oid: acc.head_oid.take(),
        }
    } else {
        WorkspaceGitHead::Branch {
            name,
            oid: acc.head_oid.take(),
        }
    };

    // 排序放在**截断之后**：截断时子集 = git 输出序前 N 条（不承诺跨次稳定，但内存有界）；
    // 未截断时全量排序，结果完全确定。
    acc.files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Projection {
        head,
        upstream: acc.upstream,
        ahead: acc.ahead,
        behind: acc.behind,
        counts: acc.counts,
        files: WorkspaceGitFileList {
            items: acc.files,
            truncated: acc.truncated,
            limit: MAX_LISTED_FILES,
        },
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

/// 归一化（**已剥工作区前缀**的）记录路径：`kind` 由尾斜杠判定，`path` 去尾斜杠。
///
/// - `newdir/` → (`newdir`, dir)：git 把整个未跟踪目录折叠成一条带尾斜杠的记录
/// - `./` → (`.`, dir)
/// - 空串 → (`.`, dir)：这条记录就是工作区根自身（整根被折叠成一条）
///
/// 契约承诺 `path` 不带尾斜杠、类型判别交给 `kind`（见 `docs/命令行与路由.md` 的路由表）。
fn checked_path(raw: &str) -> (String, WorkspaceGitFileKind) {
    let (path, kind) = match raw.strip_suffix('/') {
        Some(base) => (base, WorkspaceGitFileKind::Dir),
        None => (raw, WorkspaceGitFileKind::File),
    };
    if path.is_empty() {
        return (".".to_string(), WorkspaceGitFileKind::Dir);
    }
    (path.to_string(), kind)
}

/// 工作区根在仓库内的前缀（= `git rev-parse --show-prefix` 的输出；工作区根就是仓库根时为空串）。
///
/// 只做行尾归一，**尾斜杠保留**——它正是剥前缀时的边界，去掉就会把 `ws/inner2/x`
/// 误判成 `ws/inner` 的子路径。
fn workspace_root_prefix(raw: &str) -> String {
    raw.trim_end_matches(['\r', '\n']).to_string()
}

/// 把「仓库根相对」的记录路径换成「工作区根相对」。
///
/// - `Ok(None)` ⇒ 该记录落在工作区根之外，**必须丢弃**（不得把根外路径回给客户端）。
/// - `Ok(Some(""))` ⇒ 记录就是工作区根自身，由 [`checked_path`] 归一为 `.`。
/// - `Err` ⇒ 记录自带空 path（畸形输入）：**不猜**，按 `GIT_PROBE_UNPARSABLE` 失败。
///   注意与上一条的区别：`Some("")` 是**剥完前缀**才空，`Err` 是**原始**就空。
///
/// `-z` 下 porcelain v2 的 path 恒为仓库根相对（`status.relativePaths` 不起作用），
/// 所以这是唯一的根内/根外判据：不依赖任何可被宿主改掉的 git 配置。
fn root_relative<'a>(path: &'a str, prefix: &str) -> Result<Option<&'a str>, String> {
    if path.is_empty() {
        return Err("记录 path 为空".to_string());
    }
    if prefix.is_empty() {
        return Ok(Some(path));
    }
    Ok(path.strip_prefix(prefix))
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

/// 同步探测：先判仓库，取工作区前缀，再取 [`status_argv`] 的输出。
///
/// 仓库判据那条命令**刻意不带** `-c` 钉死参数：它要与 `git::ensure_git_repo_try` 保持
/// 「同命令同输出值」，多一个参数就破了这条同源性（`rev-parse` 的输出也不受那些配置影响）。
pub(crate) fn probe(root: &Path, mode: UntrackedMode) -> ProbeOutcome {
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

    // 工作区根在仓库内的前缀：`-z` 下 path 恒为仓库根相对，裁剪靠剥它（见 `parse_porcelain_v2`）。
    let prefix_out = match git_stdout(root, &["rev-parse", "--show-prefix"]) {
        Ok(out) => out,
        Err(outcome) => return *outcome,
    };
    let prefix = workspace_root_prefix(&String::from_utf8_lossy(&prefix_out));

    let status = match git_stdout(root, &status_argv(mode)) {
        Ok(out) => out,
        Err(outcome) => return *outcome,
    };
    // 按字节交给解析器（不经整段 lossy）：`-z` 下路径可能是任意字节。
    match parse_porcelain_v2(&status, &prefix) {
        Ok(p) => ProbeOutcome::Ok(p),
        Err(e) => ProbeOutcome::Failed {
            reason_code: error_codes::GIT_PROBE_UNPARSABLE,
            message: e,
        },
    }
}

/// 跑一条只读 git 命令并取 stdout；非零退出与起不来都折成探测结论。
///
/// 错误用 `Box` 包一层：`ProbeOutcome::Ok` 带着整份投影，直接当 `Err` 会触发
/// `clippy::result_large_err`（每个 `Result` 都按最大变体占位）。
fn git_stdout(root: &Path, args: &[&str]) -> Result<Vec<u8>, Box<ProbeOutcome>> {
    match Command::new("git").args(args).current_dir(root).output() {
        Ok(o) if o.status.success() => Ok(o.stdout),
        Ok(o) => {
            // argv 与 stderr 只进日志：`message` 会**原样回给客户端**，用户文案必须笼统
            // （AGENTS §4.5），否则等于把内部命令行与 stderr 送到浏览器。
            let stderr = String::from_utf8_lossy(&o.stderr);
            let first_line: String = stderr
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            tracing::debug!(
                ?args,
                code = o.status.code(),
                stderr = %first_line,
                "workspace git: 子命令非零退出"
            );
            Err(Box::new(ProbeOutcome::Failed {
                reason_code: error_codes::GIT_PROBE_FAILED,
                message: "git 子命令执行失败".to_string(),
            }))
        }
        Err(e) => Err(Box::new(spawn_failure(&e))),
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

/// 把探测结论组装为响应。`source` / `computed_at_unix` 由调用方按缓存命中情况给出；
/// `mode` 原样回显（口径必须能追溯到取数处，不能由常量猜）。
pub(crate) fn response_from_outcome(
    outcome: ProbeOutcome,
    computed_at_unix: u64,
    source: WorkspaceGitSource,
    mode: UntrackedMode,
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
            // 列表**恒**填入：缓存里永远是全量，是否对外可见由 handler 按 `?files` 决定。
            files: Some(p.files),
            untracked_mode: contract_mode(mode),
            computed_at_unix,
            source,
        },
        ProbeOutcome::NotARepo => unavailable(
            WorkspaceGitState::NotARepo,
            None,
            "当前工作区不是 Git 仓库",
            computed_at_unix,
            source,
            mode,
        ),
        ProbeOutcome::GitUnavailable => unavailable(
            WorkspaceGitState::GitUnavailable,
            None,
            "未找到可用的 git 可执行文件",
            computed_at_unix,
            source,
            mode,
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
            mode,
        ),
    }
}

fn unavailable(
    state: WorkspaceGitState,
    reason_code: Option<String>,
    message: &str,
    computed_at_unix: u64,
    source: WorkspaceGitSource,
    mode: UntrackedMode,
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
        // 不可用态一律没有列表：字段缺失即「没有列表可给」，客户端不必猜。
        files: None,
        untracked_mode: contract_mode(mode),
        computed_at_unix,
        source,
    }
}
