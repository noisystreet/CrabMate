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
//! 3. **不静默重启**：需要重启时先在步骤里说明原因（观测态），失败则报 `Failed`，
//!    不重试、不循环——退避重启属于后续切片。

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use super::instances::{Declaration, InstancesFile};
use super::ops::{self, Ctx, Origin, UpArgs};
use super::probe::{Observed, Probe};
use super::record::{DesiredState, InstanceRecord};
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
}

/// 启动一个实例所需的全部声明信息（与 [`UpArgs`] 一一对应，但不含 `origin`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSpec {
    pub workspace: String,
    pub serve_config: Option<String>,
    pub port: Option<u16>,
    pub env: BTreeMap<String, String>,
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
            | Self::Stopped { name }
            | Self::Skipped { name, .. }
            | Self::Failed { name, .. } => name,
        }
    }

    pub fn has_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// 读声明 + 观测现状，产出计划（**只读**：不 spawn、不发信号）。
pub fn plan(ctx: &Ctx, file: &InstancesFile) -> Result<Vec<Step>, CtlError> {
    let observed = ops::ls(ctx)?;
    let mut steps = Vec::with_capacity(file.instances.len() + observed.len());

    for (name, decl) in &file.instances {
        steps.push(step_for_decl(name, decl, &observed)?);
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
) -> Result<Step, CtlError> {
    let workspace = decl.workspace_path()?.display().to_string();
    let current = find_current(observed, name);

    if !decl.enabled {
        return Ok(match current {
            Some((_, p)) if p.pid_alive => Step::Stop {
                name: name.to_string(),
                reason: "声明为 enabled = false".to_string(),
            },
            _ => Step::Skipped {
                name: name.to_string(),
                reason: "声明为 enabled = false，且未在运行".to_string(),
            },
        });
    }

    let Some((rec, probe)) = current else {
        return Ok(Step::Start {
            name: name.to_string(),
            spec: start_spec(decl, &workspace, None),
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

    if probe.observed == Observed::Ready {
        return Ok(Step::Running {
            name: name.to_string(),
        });
    }
    // 其余观测态（starting / stale / failed / stopped）都按「重启」处理：
    // 端口优先沿用记录里的值，`base_url` 才不会在每次收敛后漂移。
    Ok(Step::Start {
        name: name.to_string(),
        spec: start_spec(decl, &workspace, rec.port),
        restart: true,
        reason: format!("观测态为 {}，需要重新拉起", probe.observed.as_str()),
    })
}

/// 端口优先级：声明 > 记录（仅重启时） > 端口池。
fn start_spec(decl: &Declaration, workspace: &str, recorded_port: Option<u16>) -> StartSpec {
    StartSpec {
        workspace: workspace.to_string(),
        serve_config: decl.config.clone(),
        port: decl.port.or(recorded_port),
        env: decl.env.clone(),
    }
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
            ..
        } => exec_start(ctx, name, spec, restart, opts),
        Step::Stop { name, .. } => exec_stop(ctx, name, opts),
        // 已是结果态 / 有意跳过：原样保留
        other => other,
    }
}

fn exec_start(ctx: &Ctx, name: String, spec: StartSpec, restart: bool, opts: &Opts) -> Step {
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
    };
    match ops::up(ctx, &args) {
        Ok(_) => Step::Running { name },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::record::{self, InstanceState};

    fn ctx_at(dir: &std::path::Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_supervise_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    fn decl(workspace: &str, enabled: bool, port: Option<u16>) -> Declaration {
        Declaration {
            workspace: workspace.to_string(),
            config: None,
            port,
            enabled,
            env: BTreeMap::new(),
        }
    }

    fn file_with(items: &[(&str, Declaration)]) -> InstancesFile {
        let mut instances = BTreeMap::new();
        for (name, d) in items {
            instances.insert((*name).to_string(), d.clone());
        }
        InstancesFile { instances }
    }

    /// 写一条「指向自身 pid」的记录：进程活着，但身份不匹配（不会误杀）。
    fn write_live_record(ctx: &Ctx, name: &str, origin: Origin, desired: DesiredState) {
        ctx.paths.ensure_dirs().expect("建目录");
        let mut rec = InstanceRecord::new(name);
        rec.state = InstanceState::Ready;
        rec.desired_state = desired;
        rec.pid = Some(std::process::id());
        rec.pgid = Some(std::process::id() as i32);
        rec.port = Some(1);
        rec.boot_id = crate::cm_ctl::identity::boot_id();
        rec.pid_start_ticks = crate::cm_ctl::identity::pid_start_ticks(std::process::id());
        rec.managed_by = Some(origin.as_str().to_string());
        let path = ctx.paths.record_path(name).expect("记录路径");
        record::write_atomic(&path, &rec).expect("写记录");
    }

    #[test]
    fn declared_without_record_plans_start() {
        let tmp = tempdir("plan_start");
        let ctx = ctx_at(&tmp);
        let file = file_with(&[("a", decl("/tmp/a", true, Some(8080)))]);
        let steps = plan(&ctx, &file).expect("计划");
        assert_eq!(steps.len(), 1);
        match &steps[0] {
            Step::Start {
                spec,
                restart,
                reason,
                ..
            } => {
                assert!(!restart, "无记录时不是重启");
                assert_eq!(spec.port, Some(8080));
                assert!(reason.contains("尚无实例记录"));
            }
            other => panic!("期望 Start，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cli_owned_instance_outside_declarations_is_never_stopped() {
        // 关键安全断言：删声明不得杀掉人工 up 的实例
        let tmp = tempdir("keep_cli");
        let ctx = ctx_at(&tmp);
        write_live_record(&ctx, "manual", Origin::Cli, DesiredState::Running);
        let file = file_with(&[]);
        let steps = plan(&ctx, &file).expect("计划");
        assert!(steps.is_empty(), "不应产生任何步骤：{steps:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn supervise_owned_instance_outside_declarations_is_stopped() {
        let tmp = tempdir("stop_orphan");
        let ctx = ctx_at(&tmp);
        write_live_record(&ctx, "ghost", Origin::Supervise, DesiredState::Running);
        let file = file_with(&[]);
        let steps = plan(&ctx, &file).expect("计划");
        assert_eq!(steps.len(), 1, "{steps:?}");
        assert!(matches!(steps[0], Step::Stop { .. }), "{steps:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn disabled_declaration_plans_stop_when_alive() {
        let tmp = tempdir("disabled_stop");
        let ctx = ctx_at(&tmp);
        write_live_record(&ctx, "a", Origin::Supervise, DesiredState::Running);
        let file = file_with(&[("a", decl("/tmp/a", false, None))]);
        let steps = plan(&ctx, &file).expect("计划");
        assert!(matches!(steps[0], Step::Stop { .. }), "{steps:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn down_marked_instance_is_not_resurrected() {
        // enabled = true 也不能越过用户显式 down
        let tmp = tempdir("respect_down");
        let ctx = ctx_at(&tmp);
        write_live_record(&ctx, "a", Origin::Supervise, DesiredState::Stopped);
        let file = file_with(&[("a", decl("/tmp/a", true, None))]);
        let steps = plan(&ctx, &file).expect("计划");
        match &steps[0] {
            Step::Skipped { reason, .. } => assert!(reason.contains("desired_state"), "{reason}"),
            other => panic!("期望 Skipped，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unimplemented_decl_dry_run_makes_no_side_effects() {
        let tmp = tempdir("dry_run");
        let ctx = ctx_at(&tmp);
        let file = file_with(&[("a", decl("/tmp/definitely-not-here-a", true, None))]);
        let steps = plan(&ctx, &file).expect("计划");
        let opts = Opts {
            timeout: Duration::from_millis(50),
            grace: Duration::from_millis(10),
            dry_run: true,
        };
        let out = execute(&ctx, steps, &opts);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Step::Start { .. }), "{out:?}");
        // dry-run 不应留下任何记录
        assert!(
            !ctx.paths.record_path("a").expect("路径").exists(),
            "dry-run 不得写状态文件"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
