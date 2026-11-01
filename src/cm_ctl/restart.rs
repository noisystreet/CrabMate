//! 退避重启：`supervise --restart on-failure` 的策略与健康巡检。
//!
//! 三条边界，都与「收敛不得失控」有关：
//!
//! 1. **只重启、不回收**：巡检只执行 `Step::Start`。`Step::Stop` / `Skipped` 仍由
//!    「声明变化」那条路径独占——让周期性动作顺手杀进程，等于把回收语义从
//!    「声明变更时」偷偷扩成「每 N 秒一次」。
//! 2. **判据只有一处**：巡检复用 [`supervise::scoped_plan`]（`plan` + 范围裁剪），不另写「什么算不健康」。
//!    `enabled` / `desired_state` / profile 变更的判定都在那里，第二处必然漂移。
//! 3. **记账落在状态记录里**：`restart_attempts` / `next_restart_at` 写盘，所以
//!    ctl 重启后仍在退避窗口内——立刻崩溃的实例不该因为守护进程重启就白得一次即时重试。

use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use super::instances::{self, InstancesFile};
use super::ops::Ctx;
use super::probe::{Observed, Probe};
use super::query;
use super::record::{self, DesiredState, InstanceRecord};
use super::supervise::{self, Opts, Step};
use super::CtlError;

/// 重启策略（CLI 的 `--restart` 翻译后的形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Restart {
    /// 默认：只在声明变化时收敛，绝不因崩溃重启。
    #[default]
    Never,
    /// 观测到 `stopped` / `failed` / `stale` 时按退避重启。
    OnFailure,
}

/// 退避曲线：第 `n` 次尝试之后的等待为 `min(initial * 2^(n-1), max)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(Duration::from_secs(5), Duration::from_secs(300))
    }
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        // 0 会退化成忙循环；初始也不该超过封顶（否则「封顶」名不副实）
        let initial = initial.max(Duration::from_secs(1));
        Self {
            initial,
            max: max.max(initial),
        }
    }

    pub fn delay(&self, attempt: u32) -> Duration {
        // `2^n` 在 attempt 变大时会溢出：先夹住指数，`saturating_mul` 再兜一层
        let shift = attempt.saturating_sub(1).min(16);
        self.initial
            .saturating_mul(1u32 << shift)
            .min(self.max)
    }

    /// 第 `attempt` 次尝试之后的下一次允许时间。
    pub fn next_from(self, now: DateTime<Utc>, attempt: u32) -> DateTime<Utc> {
        now + chrono::Duration::seconds(self.delay(attempt).as_secs() as i64)
    }
}

/// 巡检对单个实例的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// 现在就试；`attempt` 已含本次（从 1 起）。
    Go { attempt: u32 },
    /// 退避未到期：静默跳过（`status` 里能看到 `next_restart_at`）。
    Wait,
}

/// 退避门：是否允许此刻再试一次。
///
/// 时间戳解析失败时**放行**而不是卡住：宁可多试一次，也不要让一条坏时间戳
/// 把实例永久锁在「等待」里。
pub fn gate(rec: Option<&InstanceRecord>, now: DateTime<Utc>) -> Gate {
    let until = rec
        .and_then(|r| r.next_restart_at.as_deref())
        .and_then(parse_rfc3339);
    if let Some(until) = until
        && now < until
    {
        return Gate::Wait;
    }
    Gate::Go {
        attempt: rec.map_or(1, |r| r.restart_attempts.saturating_add(1)),
    }
}

fn parse_rfc3339(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 巡检唯一会重启的观测态。
///
/// `starting` 被**刻意排除**：那是启动中的中间态，`ops::up` 自己的就绪等待才是
/// 它的裁判；巡检去重启它等于把「启动慢」判成「崩溃」。
fn is_failure(observed: Option<Observed>) -> bool {
    matches!(
        observed,
        Some(Observed::Stopped) | Some(Observed::Failed) | Some(Observed::Stale)
    )
}

/// 一轮健康巡检：探测 → 只读计划 → 只对「失败且退避已到期」的实例执行启动。
///
/// 返回本轮真正执行过的步骤（为空是最常见的情况，调用方据此保持安静）。
pub fn sweep(
    ctx: &Ctx,
    file: &InstancesFile,
    opts: &Opts,
    now: DateTime<Utc>,
) -> Result<Vec<Step>, CtlError> {
    // 一次探测：`plan` 内部还会再探一次，但这里必须先拿到观测态才能筛掉
    // 「启动中 / 健康」——把这两类交给 `plan` 的 `Start` 分支会导致
    // 正在启动的实例被当成崩溃重启。
    let observed = query::ls(ctx)?;
    let mut run = Vec::new();
    let mut booked: Vec<(String, u32)> = Vec::new();

    // 巡检与一次性收敛必须用同一处范围裁剪，否则 `supervise <name>` 在常驻模式下
    // 会去重启别的实例。
    for step in supervise::scoped_plan(ctx, file, opts)? {
        match step {
            // 健康：清零退避（这是「重启成功后算恢复」的唯一信号）
            Step::Running { name } => reset(ctx, &name)?,
            Step::Start {
                name,
                spec,
                restart,
                reason,
            } => {
                if !is_failure(observed_of(&observed, &name)) {
                    continue;
                }
                let rec = read_record(ctx, &name)?;
                let Gate::Go { attempt } = gate(rec.as_ref(), now) else {
                    continue;
                };
                booked.push((name.clone(), attempt));
                run.push(Step::Start {
                    name,
                    spec,
                    restart,
                    reason,
                });
            }
            // 回收 / 跳过 / 结果态：不是巡检的职责
            _ => {}
        }
    }

    let done = supervise::execute(ctx, run, opts);
    // 记账放在执行**之后**：`ops::up` 会写一份新记录（`attempts` 归零），
    // 先记会被它覆盖掉，退避就永远不生效。
    for (name, attempt) in booked {
        book(ctx, &name, attempt, now, opts.backoff)?;
    }
    Ok(done)
}

fn observed_of(observed: &[(InstanceRecord, Probe)], name: &str) -> Option<Observed> {
    observed
        .iter()
        .find(|(rec, _)| rec.name == name)
        .map(|(_, probe)| probe.observed)
}

fn read_record(ctx: &Ctx, name: &str) -> Result<Option<InstanceRecord>, CtlError> {
    record::read_optional(&ctx.record_path(name)?)
}

/// 记一次尝试：次数 +1，并把下一次允许时间推到 `now + delay(n)`。
///
/// **失败同样记账**：否则「永远起不来的实例」每轮巡检都会被重试一次，
/// 退避形同不存在。
fn book(
    ctx: &Ctx,
    name: &str,
    attempt: u32,
    now: DateTime<Utc>,
    backoff: Backoff,
) -> Result<(), CtlError> {
    let Some(mut rec) = read_record(ctx, name)? else {
        return Ok(());
    };
    rec.restart_attempts = attempt;
    rec.next_restart_at = Some(backoff.next_from(now, attempt).to_rfc3339());
    // 重启流程内部会先 `ops::down`，而 `down` 会写下 `desired_state = stopped`
    // 作为「用户显式停止」的标记。那次 down 是本模块发起的动作、不是用户意图，
    // 必须恢复——否则**一次失败的重启会把实例永久钉成「已被 down」，退避重试
    // 再也走不到**（这正是本用例在实现时抓到的）。期望态仍归声明所有。
    rec.desired_state = DesiredState::Running;
    record::write_atomic(&ctx.record_path(name)?, &rec)
}

/// 观测到健康：清零退避。无变化时不写盘（巡检每轮都会走到这里）。
fn reset(ctx: &Ctx, name: &str) -> Result<(), CtlError> {
    let Some(mut rec) = read_record(ctx, name)? else {
        return Ok(());
    };
    if rec.restart_attempts == 0 && rec.next_restart_at.is_none() {
        return Ok(());
    }
    rec.restart_attempts = 0;
    rec.next_restart_at = None;
    record::write_atomic(&ctx.record_path(name)?, &rec)
}

/// 巡检节拍：常驻循环每轮调用一次，内部按 `--health-interval` 自行节流。
///
/// 与声明指纹轮询（`--interval`）分开是刻意的：后者每秒都要 stat + 读文件，
/// 而探测每轮都要发 TCP 连接，频率不该相同。
#[derive(Debug)]
pub struct Health {
    every: Duration,
    last: Option<Instant>,
}

impl Health {
    pub fn new(every: Duration) -> Self {
        Self {
            every: every.max(Duration::from_secs(1)),
            last: None,
        }
    }

    /// 到点则跑一轮巡检，返回是否真的执行过重启。
    ///
    /// 策略为 `Never` 时**直接返回**：默认行为必须与加这个开关之前逐字一致。
    pub fn tick(&mut self, ctx: &Ctx, path: &Path, opts: &Opts) -> bool {
        if opts.restart != Restart::OnFailure || !self.due() {
            return false;
        }
        // 声明不可用：交给 `--watch` 的暂停告警，巡检不重复报同一个错
        let Ok(file) = instances::load(path) else {
            return false;
        };
        match sweep(ctx, &file, opts, Utc::now()) {
            Ok(steps) => {
                super::render::restart(&steps);
                !steps.is_empty()
            }
            Err(e) => {
                super::render::supervise_refused(&format!("{}（{}）", e.message, e.error_code));
                false
            }
        }
    }

    fn due(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last
            && now.duration_since(last) < self.every
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_ctl::instances::Declaration;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::record::{DesiredState, InstanceState};
    use std::collections::BTreeMap;

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_restart_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    fn opts(backoff: Backoff) -> Opts {
        Opts {
            timeout: Duration::from_millis(50),
            grace: Duration::from_millis(10),
            dry_run: false,
            restart: Restart::OnFailure,
            backoff,
            health: Duration::from_secs(30),
            scope: None,
        }
    }

    fn file_with(items: &[(&str, &str)]) -> InstancesFile {
        let mut instances = BTreeMap::new();
        for (name, workspace) in items {
            instances.insert(
                (*name).to_string(),
                Declaration {
                    workspace: Some((*workspace).to_string()),
                    config: None,
                    profile: None,
                    port: None,
                    enabled: true,
                    env: BTreeMap::new(),
                },
            );
        }
        InstancesFile { instances }
    }

    /// 写一条「指向自身 pid」的记录：进程活着但身份不匹配（`stale`），
    /// 因此既不会真的 spawn，也不会被误判成健康。
    fn write_record(ctx: &Ctx, name: &str, state: InstanceState, desired: DesiredState) -> String {
        use crate::cm_ctl::identity;
        ctx.paths.ensure_dirs().expect("建目录");
        let mut rec = InstanceRecord::new(name);
        rec.state = state;
        rec.desired_state = desired;
        rec.pid = Some(std::process::id());
        rec.pgid = Some(std::process::id() as i32);
        rec.boot_id = identity::boot_id();
        rec.pid_start_ticks = identity::pid_start_ticks(std::process::id());
        rec.managed_by = Some("supervise".to_string());
        let path = ctx.paths.record_path(name).expect("记录路径");
        record::write_atomic(&path, &rec).expect("写记录");
        path.display().to_string()
    }

    fn read(ctx: &Ctx, name: &str) -> InstanceRecord {
        record::read_optional(&ctx.paths.record_path(name).expect("路径"))
            .expect("读记录")
            .expect("记录应存在")
    }

    #[test]
    fn delay_grows_then_caps() {
        let b = Backoff::new(Duration::from_secs(5), Duration::from_secs(300));
        assert_eq!(b.delay(1), Duration::from_secs(5));
        assert_eq!(b.delay(2), Duration::from_secs(10));
        assert_eq!(b.delay(3), Duration::from_secs(20));
        assert_eq!(b.delay(7), Duration::from_secs(300), "封顶生效");
        assert_eq!(b.delay(u32::MAX), Duration::from_secs(300), "不得溢出");
    }

    /// 初始值非法（0 / 大于封顶）时收敛成可用曲线，而不是把实例锁死或忙循环。
    #[test]
    fn backoff_clamps_initial() {
        let b = Backoff::new(Duration::ZERO, Duration::from_secs(30));
        assert_eq!(b.delay(1), Duration::from_secs(1));
        let b = Backoff::new(Duration::from_secs(600), Duration::from_secs(30));
        assert_eq!(b.delay(1), Duration::from_secs(600), "封顶不得低于初始");
        assert_eq!(b.delay(9), Duration::from_secs(600));
    }

    #[test]
    fn gate_waits_inside_window_and_allows_after() {
        let now = Utc::now();
        let mut rec = InstanceRecord::new("a");
        rec.restart_attempts = 2;
        rec.next_restart_at = Some((now + chrono::Duration::seconds(60)).to_rfc3339());
        assert_eq!(gate(Some(&rec), now), Gate::Wait);

        // 窗口过期 ⇒ 第 3 次
        let later = now + chrono::Duration::seconds(61);
        assert_eq!(gate(Some(&rec), later), Gate::Go { attempt: 3 });
    }

    /// 坏时间戳不得把实例永久卡在「等待」里。
    #[test]
    fn gate_allows_when_timestamp_is_unparsable() {
        let mut rec = InstanceRecord::new("a");
        rec.restart_attempts = 4;
        rec.next_restart_at = Some("不是时间".to_string());
        assert_eq!(gate(Some(&rec), Utc::now()), Gate::Go { attempt: 5 });
    }

    /// 显式 `down` 与「正在启动」都不该被巡检重启。
    #[test]
    fn sweep_skips_downed_and_starting() {
        let tmp = tempdir("skip");
        let ctx = ctx_at(&tmp);
        write_record(&ctx, "downed", InstanceState::Stopped, DesiredState::Stopped);
        write_record(&ctx, "booting", InstanceState::Starting, DesiredState::Running);
        let file = file_with(&[("downed", "/tmp/nope-downed"), ("booting", "/tmp/nope-boot")]);
        let steps = sweep(&ctx, &file, &opts(Backoff::default()), Utc::now()).expect("巡检");
        assert!(steps.is_empty(), "不该有任何动作：{steps:?}");
        assert_eq!(read(&ctx, "downed").restart_attempts, 0);
        assert_eq!(read(&ctx, "booting").restart_attempts, 0);
    }

    /// 失败尝试同样记账，且下一轮被退避挡住。
    ///
    /// workspace 不存在 ⇒ `ops::up` 在预检阶段就失败，不会真的 spawn `serve`。
    #[test]
    fn failed_attempt_books_backoff_and_blocks_next_sweep() {
        let tmp = tempdir("book");
        let ctx = ctx_at(&tmp);
        write_record(&ctx, "a", InstanceState::Stopped, DesiredState::Running);
        let file = file_with(&[("a", "/tmp/definitely-not-here-a")]);
        let now = Utc::now();
        let backoff = Backoff::new(Duration::from_secs(5), Duration::from_secs(300));

        let steps = sweep(&ctx, &file, &opts(backoff), now).expect("巡检");
        assert!(
            matches!(steps.as_slice(), [Step::Failed { .. }]),
            "应尝试一次并失败：{steps:?}"
        );
        let rec = read(&ctx, "a");
        assert_eq!(rec.restart_attempts, 1, "失败也要记账");
        assert!(rec.next_restart_at.is_some(), "应写下下次允许时间");
        assert_eq!(
            rec.desired_state,
            DesiredState::Running,
            "巡检不得改动期望态"
        );

        // 同一时刻再来一轮：退避未到期，必须安静
        let again = sweep(&ctx, &file, &opts(backoff), now).expect("巡检");
        assert!(again.is_empty(), "退避窗口内不该重试：{again:?}");
        assert_eq!(read(&ctx, "a").restart_attempts, 1);

        // 窗口过后必须**再试**：内部 `down` 留下的 `desired_state = stopped`
        // 不得把实例永久钉成「用户已停止」
        let later = now + chrono::Duration::seconds(6);
        let third = sweep(&ctx, &file, &opts(backoff), later).expect("巡检");
        assert!(
            matches!(third.as_slice(), [Step::Failed { .. }]),
            "窗口过后应重试：{third:?}"
        );
        assert_eq!(read(&ctx, "a").restart_attempts, 2, "次数应递增");
    }

    #[test]
    fn reset_clears_backoff_state() {
        let tmp = tempdir("reset");
        let ctx = ctx_at(&tmp);
        write_record(&ctx, "a", InstanceState::Stopped, DesiredState::Running);
        let path = ctx.paths.record_path("a").expect("路径");
        let mut rec = read(&ctx, "a");
        rec.restart_attempts = 3;
        rec.next_restart_at = Some(Utc::now().to_rfc3339());
        record::write_atomic(&path, &rec).expect("写");

        reset(&ctx, "a").expect("清零");
        let rec = read(&ctx, "a");
        assert_eq!(rec.restart_attempts, 0);
        assert!(rec.next_restart_at.is_none());
    }

    /// 策略 `Never` 时巡检绝不动作——默认行为必须与加开关之前一致。
    #[test]
    fn never_policy_never_touches_instances() {
        let tmp = tempdir("never");
        let ctx = ctx_at(&tmp);
        write_record(&ctx, "a", InstanceState::Stopped, DesiredState::Running);
        let decl_path = tmp.join("instances.toml");
        std::fs::write(
            &decl_path,
            "[instances.a]\nworkspace = \"/tmp/definitely-not-here-a\"\n",
        )
        .expect("写声明");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&decl_path, std::fs::Permissions::from_mode(0o600)).expect("权限");

        let mut health = Health::new(Duration::from_secs(1));
        let mut never = opts(Backoff::default());
        never.restart = Restart::Never;
        assert!(!health.tick(&ctx, &decl_path, &never), "Never 不该有动作");
        assert_eq!(read(&ctx, "a").restart_attempts, 0);
    }
}
