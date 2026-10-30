//! `supervise --from-config`：把 `instances.toml` 的声明收敛成真实进程。
//!
//! 分成**计划**（[`plan`]，只读）与**执行**（[`execute`]）两段：`--dry-run` 只跑到
//! 计划为止，既能给用户看清「将要做什么」，也让计划逻辑可以脱离真实 `serve` 单测。
//!
//! 三条语义约束，都是为了避免「收敛」变成「失控」：
//!
//! 1. **只动自己拉起的实例**：回收范围限 [`Origin::Supervise`]。人工 `up` 的实例
//!    即便不在声明里也不会被碰——删一条声明不该杀掉用户手工起的进程。
//! 2. **不覆盖用户的显式停止**：`desired_state = stopped`（`down` 留下的）优先于
//!    声明里的 `enabled = true`，否则用户刚停掉的实例会被常驻的 supervise 立刻拉起。
//! 3. **不静默重启**：声明变化引发的（重新）启动都会在步骤里说明原因（观测态），
//!    失败则报 `Failed`，不重试、不循环。崩溃后的重试是**显式开关**
//!    （`--restart on-failure`，见 `restart` 模块），默认关闭。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use super::instances::{self, Declaration, InstancesFile};
use super::ops::{self, Ctx, Origin, UpArgs};
use super::probe::{Observed, Probe};
use super::profiles::{self, ProfilesFile};
use super::record::{DesiredState, InstanceRecord};
use super::restart;
use super::CtlError;

/// 一次收敛的参数。
#[derive(Debug, Clone)]
pub struct Opts {
    /// 启动时等待 TCP 就绪的墙钟。
    pub timeout: Duration,
    /// 回收时的终止宽限（超时后 SIGKILL）。
    pub grace: Duration,
    /// 只出计划，不 spawn、不发信号。
    pub dry_run: bool,
    /// 崩溃重启策略（`--restart`）。默认 `Never`，即维持「只在声明变化时收敛」。
    pub restart: restart::Restart,
    /// 退避曲线（`--backoff-initial` / `--backoff-max`）。
    pub backoff: restart::Backoff,
    /// 健康巡检间隔（`--health-interval`），与声明指纹轮询分开。
    pub health: Duration,
    /// 监督范围：`None` = 整份声明；`Some(name)` = 只认这一个实例（`supervise <name>`）。
    ///
    /// 裁剪统一走 [`scoped`]，不要在单个消费点各写一遍。
    pub scope: Option<String>,
}

/// 按监督范围裁剪计划。
///
/// 限定范围时，**别的实例的 `Stop` / 孤儿回收一并丢弃**：「守护这一个」不能变成
/// 「顺手停掉没让我停的东西」——与 `up --all` 同一条边界。目标实例自己的步骤
/// （包括声明判给它的 `Stop`）保留：声明仍是事实。
///
/// 所有消费计划的地方都必须经过这里。目前有两处：本模块的 [`apply`]（一次性 /
/// `--watch` / `--socket`）与 `restart::sweep`（健康巡检）。漏掉任何一处，
/// 「常驻」与「一次性」的行为就会分叉。
pub(crate) fn scoped(steps: Vec<Step>, scope: Option<&str>) -> Vec<Step> {
    match scope {
        None => steps,
        Some(name) => steps.into_iter().filter(|s| s.name() == name).collect(),
    }
}

/// `plan` + 范围裁剪的**唯一入口**。
///
/// 四条路径都必须走这里：一次性收敛（`run.rs`）、`--watch`（[`apply`]）、
/// `--socket`（同一 `apply`）、健康巡检（`restart::sweep`）。
///
/// 这不是洁癖：加范围时第一版就漏了一次性收敛那条（它当时直接调 `plan`），
/// 表现是 `supervise alpha --dry-run` 照旧列出了 `beta`。多一个调用点就多一次
/// 漏掉的机会，所以只留一个入口。
pub fn scoped_plan(ctx: &Ctx, file: &InstancesFile, opts: &Opts) -> Result<Vec<Step>, CtlError> {
    Ok(scoped(plan(ctx, file)?, opts.scope.as_deref()))
}

/// 启动一个实例所需的全部声明信息（与 [`UpArgs`] 一一对应，但不含 `origin`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSpec {
    /// `None` = 待绑定工作区（声明里省略 `workspace`，由 client 运行时设置）。
    pub workspace: Option<String>,
    pub serve_config: Option<String>,
    pub port: Option<u16>,
    pub env: BTreeMap<String, String>,
    /// 引用的 profile 名（写进状态记录，供下一轮比对）。
    pub profile: Option<String>,
    /// 生效 profile 的指纹（只比名字会漏掉「改了内容但没改名」）。
    pub profile_fingerprint: Option<String>,
    /// profile 派生的 `--no-tools`。
    pub no_tools: bool,
}

/// 收敛步骤：既是计划条目，也是执行结果（执行后计划态被替换为结果态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// 需要（重新）启动；`restart = true` 表示先回收旧进程。
    Start {
        name: String,
        spec: StartSpec,
        restart: bool,
        reason: String,
    },
    /// 计划：需要回收进程。
    Stop { name: String, reason: String },
    /// 已处于期望态（健康运行），无需动作。
    Running { name: String },
    /// **执行结果**：启动（或重启）成功。
    ///
    /// 与 [`Step::Running`] 分开是必须的：若成功后就地塌成 `Running`，
    /// 「因为 profile 变了而重启」会在输出里显示成「已就绪（无动作）」——
    /// 用户看不到刚刚发生过一次重启，也看不到为什么。
    Started {
        name: String,
        restarted: bool,
        reason: String,
    },
    /// 已回收。
    Stopped { name: String },
    /// 有意跳过（disabled / 用户已 down）。
    Skipped { name: String, reason: String },
    /// 执行失败。
    Failed { name: String, error: String },
}

impl Step {
    pub fn name(&self) -> &str {
        match self {
            Self::Start { name, .. }
            | Self::Stop { name, .. }
            | Self::Running { name }
            | Self::Started { name, .. }
            | Self::Stopped { name }
            | Self::Skipped { name, .. }
            | Self::Failed { name, .. } => name,
        }
    }

    pub fn has_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// 读声明 + profiles + 观测现状，产出计划（**只读**：不 spawn、不发信号）。
pub fn plan(ctx: &Ctx, file: &InstancesFile) -> Result<Vec<Step>, CtlError> {
    let profiles_path = ctx.paths.profiles_file();
    let profiles_file = profiles::load_optional(&profiles_path)?;
    plan_with(ctx, file, &profiles_file, &profiles_path)
}

/// 同 [`plan`]，但 profiles 由调用方提供（便于单测，也避免 `--watch` 重复读盘）。
pub fn plan_with(
    ctx: &Ctx,
    file: &InstancesFile,
    profiles_file: &ProfilesFile,
    profiles_path: &Path,
) -> Result<Vec<Step>, CtlError> {
    let observed = ops::ls(ctx)?;
    let mut steps = Vec::with_capacity(file.instances.len() + observed.len());

    for (name, decl) in &file.instances {
        steps.push(step_for_decl(
            name,
            decl,
            &observed,
            profiles_file,
            profiles_path,
        )?);
    }

    // 声明里没有、但由 supervise 拉起的实例：声明被删 ⇒ 回收。
    let declared: BTreeSet<&str> = file.instances.keys().map(String::as_str).collect();
    for (rec, probe) in &observed {
        if declared.contains(rec.name.as_str()) {
            continue;
        }
        if let Some(step) = step_for_orphan(rec, probe) {
            steps.push(step);
        }
    }
    Ok(steps)
}

fn find_current<'a>(
    observed: &'a [(InstanceRecord, Probe)],
    name: &str,
) -> Option<&'a (InstanceRecord, Probe)> {
    observed.iter().find(|(rec, _)| rec.name == name)
}

fn step_for_decl(
    name: &str,
    decl: &Declaration,
    observed: &[(InstanceRecord, Probe)],
    profiles_file: &ProfilesFile,
    profiles_path: &Path,
) -> Result<Step, CtlError> {
    let workspace = decl
        .workspace_path()?
        .map(|p| p.display().to_string());
    // profile 在**计划阶段**就解析：引用不到立刻报错，而不是等到 spawn 才发现
    let resolved = match decl.profile.as_deref() {
        Some(profile) => Some(profiles::resolve(
            profiles_file,
            profile,
            &decl.env,
            profiles_path,
        )?),
        None => None,
    };
    let current = find_current(observed, name);
    // 指纹要单独算一次：它同时用于「计划是否需要重启」和写进启动规格
    let decl_fingerprint = resolved.as_ref().map(|r| r.fingerprint());

    if !decl.enabled {
        return Ok(disabled_step(name, current));
    }

    let Some((rec, probe)) = current else {
        return Ok(Step::Start {
            name: name.to_string(),
            spec: start_spec(decl, workspace.as_deref(), None, resolved.as_ref()),
            restart: false,
            reason: "尚无实例记录".to_string(),
        });
    };

    // 用户显式 `down` 过的实例：声明不得越过用户意图把它拉起来
    if rec.desired_state == DesiredState::Stopped {
        return Ok(Step::Skipped {
            name: name.to_string(),
            reason: "已被 `down` 标记为停止（desired_state = stopped）；\
                     请用 `crabmatectl restart` 或改声明后重新收敛"
                .to_string(),
        });
    }

    // profile 变更**不热改**运行中实例（#1165）：重启才生效，且必须先说清原因
    if let Some(reason) = profile_change_reason(decl, rec, probe.pid_alive, decl_fingerprint.as_deref()) {
        return Ok(Step::Start {
            name: name.to_string(),
            spec: start_spec(decl, workspace.as_deref(), rec.port, resolved.as_ref()),
            restart: true,
            reason,
        });
    }

    if probe.observed == Observed::Ready {
        return Ok(Step::Running {
            name: name.to_string(),
        });
    }
    // 其余观测态（starting / stale / failed / stopped）都按「重启」处理：
    // 端口优先沿用记录里的值，`base_url` 才不会在每次收敛后漂移。
    Ok(Step::Start {
        name: name.to_string(),
        spec: start_spec(decl, workspace.as_deref(), rec.port, resolved.as_ref()),
        restart: true,
        reason: format!("观测态为 {}，需要重新拉起", probe.observed.as_str()),
    })
}

/// `enabled = false` 的收敛步骤（停在声明上、但不该运行）。
///
/// 单独成函数有两个理由：压 `step_for_decl` 的分支数（CCN ≤ 10，见
/// `scripts/lizard-rust.sh`），以及把「只回收自己拉起的实例」这条边界放在一处说清。
fn disabled_step(name: &str, current: Option<&(InstanceRecord, Probe)>) -> Step {
    match current {
        // 与 `step_for_orphan` 同一条边界。软删除的语义是「这条声明别再跑了」，
        // **不是**「杀掉同名进程」：手工 `crabmatectl up` 起的同名实例不归声明管。
        // 控制面 `disable` 让这条路径从「手写 TOML」变成「一键触发」，不设这道闸
        // 就会点一下杀掉用户正在用的实例。
        Some((rec, p)) if p.pid_alive && Origin::from_record(rec) == Origin::Supervise => {
            Step::Stop {
                name: name.to_string(),
                reason: "声明为 enabled = false".to_string(),
            }
        }
        Some((rec, p)) if p.pid_alive => Step::Skipped {
            name: name.to_string(),
            reason: format!(
                "声明为 enabled = false，但该实例由 {} 拉起，不代为停止",
                Origin::from_record(rec).as_str()
            ),
        },
        _ => Step::Skipped {
            name: name.to_string(),
            reason: "声明为 enabled = false，且未在运行".to_string(),
        },
    }
}

/// 端口优先级：声明 > 记录（仅重启时） > 端口池；config / env / `--no-tools`
/// 由 profile 决定（无 profile 时退回实例自身的 `config` / `env`）。
fn start_spec(
    decl: &Declaration,
    workspace: Option<&str>,
    recorded_port: Option<u16>,
    resolved: Option<&profiles::Resolved>,
) -> StartSpec {
    StartSpec {
        workspace: workspace.map(str::to_string),
        serve_config: resolved
            .and_then(|r| r.serve_config.clone())
            .or_else(|| decl.config.clone()),
        port: decl.port.or(recorded_port),
        env: resolved
            .map(|r| r.env.clone())
            .unwrap_or_else(|| decl.env.clone()),
        profile: decl.profile.clone(),
        profile_fingerprint: resolved.map(|r| r.fingerprint()),
        no_tools: resolved.map(|r| r.no_tools).unwrap_or(false),
    }
}

/// profile 变更且进程仍在跑 ⇒ 需要重启（`None` = 无需动作）。
///
/// 两级判定：**名字变了**先报名字；名字相同但**内容指纹变了**（改了
/// `allowed_commands` / `permission_level` / `env`）同样要重启——否则收敛会
/// 一直显示 `ready`，而实际生效的是上一版权限集合。
fn profile_change_reason(
    decl: &Declaration,
    rec: &InstanceRecord,
    pid_alive: bool,
    decl_fingerprint: Option<&str>,
) -> Option<String> {
    if !pid_alive {
        return None;
    }
    if decl.profile.as_deref() != rec.profile.as_deref() {
        return Some(format!(
            "profile 从 `{}` 变为 `{}`；需重启才生效",
            rec.profile.as_deref().unwrap_or("（无）"),
            decl.profile.as_deref().unwrap_or("（无）")
        ));
    }
    // 没有 profile 就无从比较内容指纹
    decl.profile.as_ref()?;
    if decl_fingerprint != rec.profile_fingerprint.as_deref() {
        return Some(format!(
            "profile `{}` 的内容已变更（allowed_commands / permission_level / env 等）；需重启才生效",
            decl.profile.as_deref().unwrap_or("")
        ));
    }
    None
}

/// 声明里已不存在的实例：仅当它由 `supervise` 拉起**且**进程还活着时才回收。
///
/// 已停（进程不在）的记录留给 `prune`：收敛不该顺手删掉状态文件，
/// 否则用户就看不到「这个实例曾经存在、是被删声明回收的」。
fn step_for_orphan(rec: &InstanceRecord, probe: &Probe) -> Option<Step> {
    if Origin::from_record(rec) != Origin::Supervise || !probe.pid_alive {
        return None;
    }
    Some(Step::Stop {
        name: rec.name.clone(),
        reason: "声明中已不存在（该实例由 supervise 拉起）".to_string(),
    })
}

/// 执行计划（`opts.dry_run` 时原样返回，不产生副作用）。
pub fn execute(ctx: &Ctx, steps: Vec<Step>, opts: &Opts) -> Vec<Step> {
    if opts.dry_run {
        return steps;
    }
    steps.into_iter().map(|step| run_step(ctx, step, opts)).collect()
}

fn run_step(ctx: &Ctx, step: Step, opts: &Opts) -> Step {
    match step {
        Step::Start {
            name,
            spec,
            restart,
            reason,
        } => exec_start(ctx, name, spec, restart, reason, opts),
        Step::Stop { name, .. } => exec_stop(ctx, name, opts),
        // 已是结果态 / 有意跳过：原样保留
        other => other,
    }
}

fn exec_start(
    ctx: &Ctx,
    name: String,
    spec: StartSpec,
    restart: bool,
    reason: String,
    opts: &Opts,
) -> Step {
    if restart && let Err(e) = ops::down(ctx, &name, opts.grace) {
        return Step::Failed {
            name,
            error: format!("停止旧进程失败：{}（{}）", e.message, e.error_code),
        };
    }
    let args = UpArgs {
        name: name.clone(),
        workspace: spec.workspace,
        serve_config: spec.serve_config,
        port: spec.port,
        // `--yes` 必须由实例级显式决定，绝不由声明隐式给出（见 Issue #1163）
        yes: false,
        env: spec.env,
        timeout: opts.timeout,
        origin: Origin::Supervise,
        profile: spec.profile,
        profile_fingerprint: spec.profile_fingerprint,
        no_tools: spec.no_tools,
    };
    match ops::up(ctx, &args) {
        // 保留 `restarted` 与 `reason`：否则「因 profile 变更而重启」会被渲染成「无动作」
        Ok(_) => Step::Started {
            name,
            restarted: restart,
            reason,
        },
        Err(e) => Step::Failed {
            name,
            error: format!("{}（{}）", e.message, e.error_code),
        },
    }
}

fn exec_stop(ctx: &Ctx, name: String, opts: &Opts) -> Step {
    match ops::down(ctx, &name, opts.grace) {
        Ok(_) => Step::Stopped { name },
        Err(e) => Step::Failed {
            name,
            error: format!("{}（{}）", e.message, e.error_code),
        },
    }
}

/// 轮询间隔：容器 overlayfs 上 inotify 不可靠，与 `logs -f` 同策略。
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(1);

/// 声明文件的**指纹：内容 + 权限位**。
///
/// 权限必须进指纹。用户 `chmod 600` 修好一个 0644 的文件后内容一字未改，
/// 若只看内容，watch 会永远停在「已暂停收敛」而不重新加载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// `None` 表示文件不存在（被删 / 编辑器原子替换的瞬间）。
    mode: Option<u32>,
    text: String,
}

/// `--watch` 单轮的判定。
#[derive(Debug)]
pub enum Poll {
    /// 指纹未变：什么都不做（否则每秒都会重新 probe 一遍 TCP）。
    Unchanged,
    /// 指纹变了且声明可加载：收敛。
    Reconcile(InstancesFile),
    /// 指纹变了但声明不可用（权限过宽 / 属主不符 / 语法错 / 不存在）：
    /// **只告警，不收敛**。声明被改坏时「按它起进程」比「什么都不起」危险得多。
    Refused(String),
}

fn fingerprint(path: &Path) -> Fingerprint {
    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(path)
        .ok()
        .map(|m| m.mode() & 0o777);
    Fingerprint {
        mode,
        text: std::fs::read_to_string(path).unwrap_or_default(),
    }
}

/// 读一次声明并与上一轮的指纹比对。
pub fn poll_once(path: &Path, last: Option<&Fingerprint>) -> (Poll, Fingerprint) {
    let current = fingerprint(path);
    if last == Some(&current) {
        return (Poll::Unchanged, current);
    }
    let poll = match instances::load(path) {
        Ok(file) => Poll::Reconcile(file),
        Err(e) => Poll::Refused(format!("{}（{}）", e.message, e.error_code)),
    };
    (poll, current)
}

/// 按一份已加载的声明收敛。
///
/// 计划失败只告警不退出：常驻进程为一个半截 TOML 死去，比暂停收敛更糟。
fn apply(ctx: &Ctx, file: &InstancesFile, opts: &Opts) {
    match scoped_plan(ctx, file, opts) {
        Ok(steps) => {
            let done = execute(ctx, steps, opts);
            super::render::supervise(&done, false);
        }
        Err(e) => super::render::supervise_refused(&format!("{}（{}）", e.message, e.error_code)),
    }
}

/// 读一次声明并收敛（控制面收到 `add` / `remove` 后立即调用）。
pub fn reconcile_once(ctx: &Ctx, path: &Path, opts: &Opts) {
    match instances::load(path) {
        Ok(file) => apply(ctx, &file, opts),
        Err(e) => {
            super::render::supervise_refused(&format!("{}（{}）", e.message, e.error_code));
        }
    }
}

/// `--watch` 的收敛游标：记住上次看到的声明指纹。
///
/// 独立成类型是为了让「既要轮询声明、又要处理 socket 请求」的主循环
/// （`--socket` 模式）也能复用同一套判定，而不必把 `Fingerprint` 暴露出去。
#[derive(Debug, Default)]
pub struct Cursor(Option<Fingerprint>);

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 推进一轮：指纹变了就收敛。返回是否真的收敛过。
    pub fn tick(&mut self, ctx: &Ctx, path: &Path, opts: &Opts) -> bool {
        let (poll, fingerprint) = poll_once(path, self.0.as_ref());
        self.0 = Some(fingerprint);
        match poll {
            Poll::Unchanged => false,
            Poll::Refused(reason) => {
                super::render::supervise_refused(&reason);
                false
            }
            Poll::Reconcile(file) => {
                apply(ctx, &file, opts);
                true
            }
        }
    }

    /// 本进程刚改过声明：同步指纹，避免下一轮把它当成外部变化再收敛一遍。
    pub fn sync(&mut self, path: &Path) {
        self.0 = Some(fingerprint(path));
    }
}

/// `--watch`：声明变化即重新收敛，直到进程被信号终止。
///
/// 声明损坏时**暂停收敛并保留现场**（不 `exit`）：用户正在编辑的文件经常短暂失效，
/// 为一个半截 TOML 退出常驻进程比暂停更糟。修好之后指纹变化，自然恢复。
pub fn watch(ctx: &Ctx, path: &Path, opts: &Opts, interval: Duration) -> ! {
    let mut cursor = Cursor::new();
    // 声明指纹轮询（轻，每秒一次）与健康巡检（重，要发 TCP）共用这个循环，
    // 但节拍不同：巡检自己按 `--health-interval` 节流，策略为 `Never` 时直接返回。
    let mut health = restart::Health::new(opts.health);
    loop {
        cursor.tick(ctx, path, opts);
        health.tick(ctx, path, opts);
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
#[path = "supervise_tests.rs"]
mod tests;
