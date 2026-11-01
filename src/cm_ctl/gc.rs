//! `gc`：回收**数据面**（实例目录与日志），与 `prune` 的「只删记录」互补 #1213。
//!
//! `prune` 删掉状态记录后，`<name>/`（会话库、上传目录）与 `<name>.log` 会永远
//! 留在磁盘上 —— 它不是笔误，是缺能力：删数据比删记录危险一个量级，所以当初只做
//! 了前者。这里补上后者，并把危险点逐条钉住：
//!
//! 1. **只碰已停的实例**：候选集就是 [`Observed::is_prunable`]，`ready` /
//!    `starting` / `held` 一律不进候选；
//! 2. **没有 `--force`**：记录指向的进程仍活着（身份校验通过）时无条件跳过。删记录
//!    只是让进程失去地址，删目录会把它的 cwd、会话库一起抽走；逃生路径是「先 `down`」；
//! 3. **不跟随符号链接**：统计与删除都走 `symlink_metadata`，目标是链接时只删链接。
//!    否则 `instances/<name>` 被换成指向 `$HOME` 的软链时，`remove_dir_all` 会连带
//!    别人的家目录一起清掉；
//! 4. **默认只打印**：要 `--yes` 才真删，输出的就是「将删哪些路径、能释放多少」。
//!
//! 记录已经被 `prune` 掉的残留（`<name>/` 没有对应 `<name>.json`）**只报告不删**：
//! 没有记录就无从判断里面是否还跑着一个 serve，那种情形交给人确认。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::guard;
use super::lock;
use super::logio;
use super::ops::Ctx;
use super::paths;
use super::probe::{Observed, Probe};
use super::query;
use super::record::InstanceRecord;
use super::CtlError;

/// `gc` 的筛选条件。
#[derive(Debug, Clone, Copy, Default)]
pub struct GcOpts {
    /// 只看空闲至少这么久的实例（按 `--older-than` 解析）。
    pub older_than: Option<Duration>,
    /// 保留 `<name>/`，只删日志。
    pub keep_data: bool,
}

/// 一次 `gc` 的完整结果（既含「已删/将删」，也含「有意跳过」与「只报告」）。
#[derive(Debug, Clone, Default)]
pub struct GcPlan {
    pub items: Vec<GcItem>,
    /// 没有状态记录的残留：**不删**，只提示。
    pub orphans: Vec<OrphanData>,
}

/// 单个实例的回收计划。
#[derive(Debug, Clone)]
pub struct GcItem {
    pub name: String,
    pub observed: Observed,
    pub targets: Vec<GcTarget>,
    /// `Some(原因)` = 有意跳过（活进程 / 未过 `--older-than` / 名字非法）。
    pub refused: Option<String>,
}

impl GcItem {
    /// 该实例可释放的字节数（目录按递归求和，不含符号链接目标）。
    pub fn bytes(&self) -> u64 {
        self.targets.iter().map(|t| t.bytes).sum()
    }
}

/// 一个待删除的路径。
#[derive(Debug, Clone)]
pub struct GcTarget {
    pub path: PathBuf,
    pub kind: GcTargetKind,
    pub bytes: u64,
}

/// 删除方式：目录递归删，其余（普通文件 / 符号链接本身）单个删。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcTargetKind {
    Dir,
    File,
}

/// 没有状态记录的残留（早前 `prune` 掉的那批）。
#[derive(Debug, Clone)]
pub struct OrphanData {
    pub name: String,
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
}

/// 收集候选；`apply` 为真时逐条删除（否则只算，不动磁盘）。
pub fn purge(ctx: &Ctx, opts: &GcOpts, apply: bool) -> Result<GcPlan, CtlError> {
    // 删除期间必须挡住并发的 `up`：否则它可能正在同一个 `<name>/` 里 spawn。
    // 只读的 dry-run 不需要锁（避免因为别人在做长操作而查询失败）。
    let _guard = lock_if_applying(ctx, apply)?;
    // 残留先扫：删完之后再扫，刚被回收的目录会被误报成「没有记录」
    let orphans = orphan_data(ctx)?;
    Ok(GcPlan {
        items: collect_items(ctx, opts, apply)?,
        orphans,
    })
}

/// `apply` 时取状态目录锁；dry-run 不取。
fn lock_if_applying(ctx: &Ctx, apply: bool) -> Result<Option<lock::DirLock>, CtlError> {
    match apply {
        true => Ok(Some(lock::acquire(&ctx.lock_path())?)),
        false => Ok(None),
    }
}

/// 逐条判定并（可选）删除。
fn collect_items(ctx: &Ctx, opts: &GcOpts, apply: bool) -> Result<Vec<GcItem>, CtlError> {
    let mut items = Vec::new();
    for (rec, p) in query::ls(ctx)? {
        let Some(item) = candidate(ctx, &rec, &p, opts)? else {
            continue;
        };
        if apply && item.refused.is_none() {
            for target in &item.targets {
                remove_target(&target.path)?;
            }
        }
        items.push(item);
    }
    Ok(items)
}

/// 把 `--older-than` 的取值解析成时长（`30m` / `24h` / `7d`；单位必填）。
///
/// 单位必填是刻意的：`--older-than 7` 到底是一周还是 7 秒，猜错一次就会删掉刚停的
/// 实例数据。
pub fn parse_age(raw: &str) -> Result<Duration, CtlError> {
    let trimmed = raw.trim();
    let unit = trimmed.chars().last().ok_or_else(|| bad_age(raw))?;
    let digits = trimmed
        .get(..trimmed.len() - unit.len_utf8())
        .ok_or_else(|| bad_age(raw))?;
    let n: u64 = digits.trim().parse().map_err(|_| bad_age(raw))?;
    let per_unit = unit_seconds(unit).ok_or_else(|| bad_age(raw))?;
    let secs = n.checked_mul(per_unit).ok_or_else(|| bad_age(raw))?;
    Ok(Duration::from_secs(secs))
}

/// 单位 → 秒。用查表而不是 `match` 分支：lizard 的 Rust 词法器在 `match` 里遇到
/// 字符字面量（`'s'` / `'d'`）会算错花括号范围，把后面整段代码并进同一个函数，
/// `lizard-rust` / `fn-nloc-ratchet` 两个钩子都会因此误报。
const AGE_UNITS: &[(char, u64)] = &[('s', 1), ('m', 60), ('h', 3600), ('d', 86400)];

fn unit_seconds(unit: char) -> Option<u64> {
    AGE_UNITS
        .iter()
        .find(|(symbol, _)| *symbol == unit)
        .map(|(_, secs)| *secs)
}

fn bad_age(raw: &str) -> CtlError {
    CtlError::usage(
        "bad_older_than",
        format!("无法解析 --older-than「{raw}」：请写成 <数字><单位>，单位限 s / m / h / d"),
    )
}

/// 单个实例的候选判定；`None` = 不进候选（健康实例、或压根没有数据可回收）。
fn candidate(
    ctx: &Ctx,
    rec: &InstanceRecord,
    p: &Probe,
    opts: &GcOpts,
) -> Result<Option<GcItem>, CtlError> {
    if !p.observed.is_prunable() {
        return Ok(None);
    }
    // 记录里的 name 会被拼成路径：不因为「记录是自己写的」就免检，
    // 一份被改过的状态文件不该能把删除指到别处。
    if let Err(e) = paths::validate_name(&rec.name) {
        return Ok(Some(GcItem {
            name: rec.name.clone(),
            observed: p.observed,
            targets: Vec::new(),
            refused: Some(e.message),
        }));
    }
    let targets = collect_targets(ctx, &rec.name, opts.keep_data)?;
    if targets.is_empty() {
        return Ok(None);
    }
    Ok(Some(GcItem {
        name: rec.name.clone(),
        observed: p.observed,
        refused: guard::live_process_reason(rec, p).or_else(|| age_gate(ctx, rec, &targets, opts)),
        targets,
    }))
}

/// 该实例名下将删除的路径：目录（除非 `keep_data`）+ 当前日志 + 上一份轮转日志。
fn collect_targets(ctx: &Ctx, name: &str, keep_data: bool) -> Result<Vec<GcTarget>, CtlError> {
    let mut out = Vec::new();
    // 不用嵌套 `if let`（clippy 的 `collapsible_if` 会要求改成 let-chain，
    // 而 let-chain 要 Rust 1.88+，本仓 MSRV 仍是 1.85）
    let home = match keep_data {
        true => None,
        false => measure(&ctx.paths.instance_home_checked(name)?)?,
    };
    out.extend(home);
    let log = ctx.paths.log_path(name);
    for path in [log.clone(), logio::rotated_path(&log)] {
        if let Some(t) = measure(&path)? {
            out.push(t);
        }
    }
    Ok(out)
}

/// 量一个路径：不存在返回 `None`（不报错 —— 只有日志没有目录是常态）。
fn measure(path: &Path) -> Result<Option<GcTarget>, CtlError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(CtlError::general(
                "gc_stat_failed",
                format!("读取路径信息失败 {}：{e}", path.display()),
            ));
        }
    };
    // `symlink_metadata` 不跟随链接：链接本身按「文件」处理（只删链接），
    // 其指向的目录既不计字节也不会被递归删除。
    if !meta.is_dir() {
        return Ok(Some(GcTarget {
            path: path.to_path_buf(),
            kind: GcTargetKind::File,
            bytes: meta.len(),
        }));
    }
    Ok(Some(GcTarget {
        path: path.to_path_buf(),
        kind: GcTargetKind::Dir,
        bytes: dir_bytes(path)?,
    }))
}

/// 递归统计目录占用（不跟随符号链接）。
fn dir_bytes(root: &Path) -> Result<u64, CtlError> {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            // 读不动的子目录不算致命：统计值不必精确到字节
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            let Ok(meta) = fs::symlink_metadata(ent.path()) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(ent.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

/// `--older-than` 闸门：按记录文件与待删路径的 mtime 判「空闲多久」。
///
/// 不新增 `stopped_at` 字段是刻意的：状态文件字段集是客户端契约（`fixtures/ctl_contract.json`），
/// 为了一个筛选条件去扩契约不值。代价是 mtime 会被 `rsync` / `touch` 干扰，故文档写明
/// 「按文件时间，不保证等于精确停机时刻」。
fn age_gate(ctx: &Ctx, rec: &InstanceRecord, targets: &[GcTarget], opts: &GcOpts) -> Option<String> {
    let min = opts.older_than?;
    let mut idle = UNIX_EPOCH_FALLBACK;
    let record = ctx.record_path(&rec.name).ok();
    for path in record.iter().chain(targets.iter().map(|t| &t.path)) {
        match fs::symlink_metadata(path).and_then(|m| m.modified()) {
            Ok(modified) if modified > idle => idle = modified,
            // 读不到 mtime 就当它很旧：宁可放行也不能因为一次 stat 失败把候选永远卡住
            _ => {}
        }
    }
    let age = SystemTime::now()
        .duration_since(idle)
        .unwrap_or(Duration::ZERO);
    if age < min {
        return Some(format!(
            "最近活动距今 {}，不足 --older-than {}",
            human_age(age),
            human_age(min)
        ));
    }
    None
}

/// 时间下限（取「很久以前」而不是 `now`：读不到 mtime 时宁可放行）。
const UNIX_EPOCH_FALLBACK: SystemTime = SystemTime::UNIX_EPOCH;

/// 粗粒度可读时长（只用于人读提示）。
fn human_age(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// 删一个路径：目录递归删，其余单个删；已不存在视为成功（幂等）。
///
/// 删除前**重新** lstat 一次：候选集与删除之间可能隔了几百毫秒，这个窗口里
/// `<name>/` 若被换成符号链接，按旧快照 `remove_dir_all` 就会跟进去。
fn remove_target(path: &Path) -> Result<(), CtlError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CtlError::general(
                "gc_remove_failed",
                format!("读取路径信息失败 {}：{e}", path.display()),
            ));
        }
    };
    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| {
        CtlError::general(
            "gc_remove_failed",
            format!("删除失败 {}：{e}", path.display()),
        )
    })
}

/// 扫描出「有数据但没有状态记录」的残留（**只报告，不删**）。
fn orphan_data(ctx: &Ctx) -> Result<Vec<OrphanData>, CtlError> {
    let dir = ctx.paths.instances_dir();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(CtlError::general(
                "state_dir_unreadable",
                format!("读取状态目录失败 {}：{e}", dir.display()),
            ));
        }
    };
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for ent in rd.flatten() {
        let path = ent.path();
        let Some(fname) = path.file_name().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        // `.lock` 之类
        if fname.starts_with('.') {
            continue;
        }
        // 状态记录本身归 `prune` 管，不是「残留数据」（否则每次都会把
        // `<name>.json` 报成没有记录的残留，提示立刻失去意义）
        if fname.ends_with(".json") {
            continue;
        }
        let Some(base) = instance_base_name(&fname) else {
            continue;
        };
        if ctx.paths.record_path(&base).is_ok_and(|p| p.exists()) {
            continue;
        }
        groups.entry(base).or_default().push(path);
    }
    let mut out = Vec::new();
    for (name, paths) in groups {
        let mut bytes = 0u64;
        for p in &paths {
            if let Some(t) = measure(p)? {
                bytes = bytes.saturating_add(t.bytes);
            }
        }
        out.push(OrphanData { name, paths, bytes });
    }
    Ok(out)
}

/// `a.log` / `a.log.1` / 目录 `a` → `a`；认不出的后缀原样当名字（记录查不到就成了残留项）。
fn instance_base_name(fname: &str) -> Option<String> {
    let base = fname
        .strip_suffix(".log.1")
        .or_else(|| fname.strip_suffix(".log"))
        .unwrap_or(fname);
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_ctl::identity;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::probe;
    use crate::cm_ctl::record::{self, InstanceState};
    use crate::cm_ctl::signal;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_gc_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    /// 停掉的实例 + 数据目录 + 日志。
    fn stopped_instance(ctx: &Ctx, name: &str) -> (PathBuf, PathBuf) {
        ctx.paths.ensure_dirs().expect("建状态目录");
        let mut rec = InstanceRecord::new(name);
        rec.state = InstanceState::Stopped;
        let record = ctx.record_path(name).expect("记录路径");
        record::write_atomic(&record, &rec).expect("写记录");

        let home = ctx.paths.instance_home(name);
        std::fs::create_dir_all(home.join("uploads")).expect("建实例目录");
        std::fs::write(home.join("uploads/blob.bin"), b"0123456789").expect("写数据文件");
        std::fs::write(ctx.paths.log_path(name), "启动日志\n").expect("写日志");
        (record, home)
    }

    #[test]
    fn parse_age_reads_units_and_rejects_garbage() {
        assert_eq!(parse_age("30m").expect("应解析"), Duration::from_secs(1800));
        assert_eq!(parse_age("24h").expect("应解析"), Duration::from_secs(86400));
        assert_eq!(parse_age("7d").expect("应解析"), Duration::from_secs(604800));
        assert_eq!(parse_age("5s").expect("应解析"), Duration::from_secs(5));
        // 单位必填：缺单位、单位非法、非数字都要报 usage
        for raw in ["7", "", "10x", "abc", "-3d"] {
            let err = parse_age(raw).expect_err("应拒绝");
            assert_eq!(err.error_code, "bad_older_than", "{raw} 应报解析失败");
        }
    }

    /// 默认（`apply=false`）只算不删：这是「删会话库」这种动作的最低门槛。
    #[test]
    fn dry_run_measures_without_deleting() {
        let tmp = tempdir("dry");
        let ctx = ctx_at(&tmp);
        let (record, home) = stopped_instance(&ctx, "a");

        let plan = purge(&ctx, &GcOpts::default(), false).expect("应成功");
        assert_eq!(plan.items.len(), 1);
        assert!(plan.items[0].refused.is_none());
        assert!(plan.items[0].bytes() > 0, "应报出可释放字节数");
        assert!(home.exists() && record.exists(), "dry-run 不得动磁盘");
    }

    #[test]
    fn apply_removes_instance_dir_and_logs_but_keeps_the_record() {
        let tmp = tempdir("apply");
        let ctx = ctx_at(&tmp);
        let (record, home) = stopped_instance(&ctx, "a");

        let plan = purge(&ctx, &GcOpts::default(), true).expect("应成功");
        assert_eq!(plan.items.len(), 1);
        assert!(!home.exists(), "实例目录应被回收");
        assert!(!ctx.paths.log_path("a").exists(), "日志应被回收");
        assert!(record.exists(), "记录归 prune 管，gc 不碰");
    }

    #[test]
    fn keep_data_only_reclaims_logs() {
        let tmp = tempdir("keep_data");
        let ctx = ctx_at(&tmp);
        let (_record, home) = stopped_instance(&ctx, "a");

        let opts = GcOpts {
            older_than: None,
            keep_data: true,
        };
        purge(&ctx, &opts, true).expect("应成功");
        assert!(home.exists(), "--keep-data 必须保住实例目录");
        assert!(!ctx.paths.log_path("a").exists(), "日志仍应回收");
    }

    /// 刚停的实例不该被 `--older-than` 放行：这条闸门失效时用户会误删正在排障的数据。
    #[test]
    fn older_than_refuses_freshly_stopped_instance() {
        let tmp = tempdir("older");
        let ctx = ctx_at(&tmp);
        let (_record, home) = stopped_instance(&ctx, "a");

        let opts = GcOpts {
            older_than: Some(Duration::from_secs(86400)),
            keep_data: false,
        };
        let plan = purge(&ctx, &opts, true).expect("应成功");
        assert_eq!(plan.items.len(), 1);
        let reason = plan.items[0].refused.as_deref().expect("应被跳过");
        assert!(reason.contains("older-than"), "理由应点明闸门：{reason}");
        assert!(home.exists(), "被跳过的条目不得删除");
    }

    /// gc **没有** `--force`：活进程的实例无条件跳过（删目录会抽走它的 cwd）。
    #[test]
    fn live_process_is_never_reclaimed() {
        let tmp = tempdir("live");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建状态目录");
        std::fs::write(ctx.paths.log_path("live"), "x\n").expect("写日志");
        std::fs::create_dir_all(ctx.paths.instance_home("live")).expect("建实例目录");
        let (_live, rec) = live_record("live");
        record::write_atomic(&ctx.record_path("live").expect("记录路径"), &rec).expect("写记录");

        let plan = purge(&ctx, &GcOpts::default(), true).expect("应成功");
        assert_eq!(plan.items.len(), 1);
        assert!(
            plan.items[0].refused.is_some(),
            "活进程必须被拒绝：{:?}",
            plan.items[0]
        );
        assert!(ctx.paths.instance_home("live").exists(), "数据不得被删");
    }

    /// `instances/<name>` 被换成软链时必须只删链接：跟进去会清空链接目标。
    #[test]
    fn symlinked_instance_dir_is_unlinked_not_traversed() {
        let tmp = tempdir("symlink");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建状态目录");
        let victim = tmp.join("victim");
        std::fs::create_dir_all(&victim).expect("建受害目录");
        std::fs::write(victim.join("keep.txt"), "别删我").expect("写受害文件");

        let home = ctx.paths.instance_home("link");
        std::os::unix::fs::symlink(&victim, &home).expect("建软链");

        let mut rec = InstanceRecord::new("link");
        rec.state = InstanceState::Stopped;
        record::write_atomic(&ctx.record_path("link").expect("记录路径"), &rec).expect("写记录");

        purge(&ctx, &GcOpts::default(), true).expect("应成功");
        assert!(!home.symlink_metadata().is_ok(), "软链本身应被删除");
        assert!(
            victim.join("keep.txt").exists(),
            "链接目标绝不能被跟随删除"
        );
    }

    /// 没有记录的残留只报告、不删：那种情形无法判断里面是否还跑着 serve。
    #[test]
    fn recordless_leftovers_are_reported_but_never_deleted() {
        let tmp = tempdir("orphan");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建状态目录");
        let leftover = ctx.paths.instance_home("old-lane");
        std::fs::create_dir_all(&leftover).expect("建残留目录");
        std::fs::write(leftover.join("conversations.db"), b"db").expect("写残留文件");
        std::fs::write(ctx.paths.log_path("old-lane"), "旧日志\n").expect("写残留日志");

        let plan = purge(&ctx, &GcOpts::default(), true).expect("应成功");
        assert!(plan.items.is_empty(), "没有记录就不进候选：{:?}", plan.items);
        assert_eq!(plan.orphans.len(), 1);
        assert_eq!(plan.orphans[0].name, "old-lane");
        assert_eq!(plan.orphans[0].paths.len(), 2, "目录与日志应归到同一组");
        assert!(leftover.exists(), "残留不得被自动删除");
        assert!(ctx.paths.log_path("old-lane").exists());
    }

    /// 状态记录本身不是「残留数据」：它归 `prune` 管，不该出现在 gc 的提示里。
    #[test]
    fn records_are_not_reported_as_orphans() {
        let tmp = tempdir("no_orphan_record");
        let ctx = ctx_at(&tmp);
        let (_record, _home) = stopped_instance(&ctx, "a");

        let plan = purge(&ctx, &GcOpts::default(), false).expect("应成功");
        assert!(
            plan.orphans.is_empty(),
            "记录不该被当成残留：{:?}",
            plan.orphans
        );
    }

    /// 替身「长得像 serve」的活进程（与 `guard` 测试同一手法：`sleep 300; :` 两个命令
    /// 是为了让 argv 里留住 `serve` token，否则身份校验会判成 mismatch）。
    struct Lookalike(Child);

    impl Drop for Lookalike {
        fn drop(&mut self) {
            let pid = self.0.id();
            let _ = signal::terminate(
                pid,
                Some(pid as i32),
                Duration::from_millis(500),
                Duration::from_millis(20),
            );
            let _ = self.0.wait();
        }
    }

    fn live_record(name: &str) -> (Lookalike, InstanceRecord) {
        let child = Command::new("sh")
            .arg("-c")
            .args(["sleep 300; :", "serve"])
            .process_group(0)
            .stderr(Stdio::null())
            .spawn()
            .expect("起替身进程");
        let live = Lookalike(child);
        let pid = live.0.id();
        let mut rec = InstanceRecord::new(name);
        rec.state = InstanceState::Ready;
        rec.pid = Some(pid);
        rec.pgid = Some(pid as i32);
        rec.boot_id = identity::boot_id();
        rec.pid_start_ticks = identity::pid_start_ticks(pid);

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let p = probe::observe(&rec);
            if p.process_is_live_and_ours() {
                return (live, rec);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("替身进程始终不像一个活着的 serve");
    }
}
