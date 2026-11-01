//! 编排层：把记录 / 端口 / spawn / 信号 / 观测拼成各子命令的动作。
//!
//! 这里不含 clap 类型，也不直接打印（打印在 [`super::render`]），便于单测。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::guard;
use super::identity;
use super::lock;
use super::paths::{self, CtlPaths};
use super::port_pool::PortPool;
use super::preflight;
use super::probe::{self, Observed};
use super::readiness::{self, WaitOutcome};
use super::record::{self, DesiredState, InstanceRecord, InstanceState};
use super::signal;
use super::spawn::{self, SpawnSpec};
use super::CtlError;

/// 轮询间隔（readiness 与信号等待共用）。
const POLL: Duration = Duration::from_millis(100);
/// 默认终止宽限。
pub const DEFAULT_GRACE: Duration = Duration::from_secs(10);

/// 动作执行上下文。
#[derive(Debug, Clone)]
pub struct Ctx {
    pub paths: CtlPaths,
    pub pool: PortPool,
}

impl Ctx {
    pub fn resolve() -> Result<Self, CtlError> {
        Ok(Self {
            paths: CtlPaths::resolve()?,
            pool: PortPool::default_pool(),
        })
    }

    pub(crate) fn record_path(&self, name: &str) -> Result<PathBuf, CtlError> {
        self.paths.record_path(name)
    }

    pub(crate) fn lock_path(&self) -> PathBuf {
        self.paths.instances_dir().join(".lock")
    }
}

/// 实例的持有者：谁把它拉起来的。
///
/// `supervise` 的回收范围只覆盖 [`Origin::Supervise`]：人工 `up` 的实例永远
/// 不由收敛逻辑决定生死（删声明 ≠ 杀掉用户手工起的进程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 人工 `up` / `run`（含 `restart`）。
    Cli,
    /// `supervise --from-config` 按声明拉起。
    Supervise,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Supervise => "supervise",
        }
    }

    /// 沿用既有记录的持有者（`restart` 用）：缺字段的历史记录按 `cli` 处理。
    pub fn from_record(rec: &InstanceRecord) -> Self {
        match rec.managed_by.as_deref() {
            Some("supervise") => Self::Supervise,
            _ => Self::Cli,
        }
    }
}

/// `up` 的入参。
#[derive(Debug, Clone)]
pub struct UpArgs {
    pub name: String,
    /// 工作区；`None` = 待绑定（子 serve 以 `--workspace-unset` 启动，等 client 设置）。
    pub workspace: Option<String>,
    /// 交给子 serve 的 `--config`（与 ctl 自身的 `--config` 无关）。
    pub serve_config: Option<String>,
    pub port: Option<u16>,
    pub yes: bool,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
    /// 谁在拉起这个实例（决定它将来会不会被 `supervise` 回收）。
    pub origin: Origin,
    /// 引用的 profile 名（写入状态记录，供 `supervise` 比对「声明变了但没重启」）。
    pub profile: Option<String>,
    /// 生效 profile 的指纹（同上；只比名字会漏掉「改了内容但没改名」）。
    pub profile_fingerprint: Option<String>,
    /// profile 派生的 `--no-tools`。
    pub no_tools: bool,
}

/// `up` 结果。
#[derive(Debug, Clone)]
pub struct UpOutcome {
    pub record: InstanceRecord,
    /// 已有健康实例、本次未重复启动。
    pub already_running: bool,
    pub observed: Observed,
    /// 预检提示（如「未指定 --serve-config」）：可继续，但值得让用户看见。
    /// 由输出层打印，`ops` 自身不打印。
    pub warnings: Vec<String>,
}

/// `down` 结果。
#[derive(Debug, Clone)]
pub struct DownOutcome {
    pub name: String,
    /// 实例本就不存在或本就未运行。
    pub already_stopped: bool,
    pub forced: bool,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 从子进程日志里取最后一次 `web_ready` 行（`--web-ready-json` 已开启）。
fn ready_json_from_log(log: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(log).ok()?;
    text.lines()
        .rev()
        .find(|l| l.contains("\"web_ready\""))
        .and_then(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
}

/// 已存在且健康时直接复用（`up` 的幂等语义）。
fn existing_if_ready(ctx: &Ctx, name: &str) -> Result<Option<InstanceRecord>, CtlError> {
    let path = ctx.record_path(name)?;
    let Some(rec) = record::read_optional(&path)? else {
        return Ok(None);
    };
    if probe::observe(&rec).observed == Observed::Ready {
        Ok(Some(rec))
    } else {
        Ok(None)
    }
}

/// 构造 spawn 规格。
fn build_spec(
    ctx: &Ctx,
    args: &UpArgs,
    workspace: Option<&Path>,
    port: u16,
) -> SpawnSpec {
    let log_path = ctx.paths.log_path(&args.name);
    SpawnSpec {
        workspace: workspace.map(Path::to_path_buf),
        serve_config: args.serve_config.as_ref().map(PathBuf::from),
        port,
        yes: args.yes,
        no_tools: args.no_tools,
        env: args.env.clone(),
        log_path,
        home: ctx.paths.instance_home(&args.name),
        ready_json: true,
        inherit_io: false,
        own_process_group: true,
    }
}

/// 在**短临界区**内 spawn 并预写 `starting` 记录。
fn spawn_and_record(
    ctx: &Ctx,
    args: &UpArgs,
    workspace: Option<&Path>,
    port: u16,
) -> Result<InstanceRecord, CtlError> {
    let rec_path = ctx.record_path(&args.name)?;
    let _guard = lock::acquire(&ctx.lock_path())?;

    // 锁内二次确认，避免两个 ctl 同时通过前置检查
    if let Some(rec) = existing_if_ready(ctx, &args.name)? {
        return Ok(rec);
    }
    // 同一道闸门在临界区内再查一次：前置检查与拿锁之间，另一个 ctl 可能刚起好一个
    // 还没就绪的实例，此时覆盖记录同样会把它变成孤儿。
    guard::ensure_not_live(ctx, &args.name)?;

    let spec = build_spec(ctx, args, workspace, port);
    let spawned = spawn::spawn_serve(&spec)?;
    let pid = spawned.pid();

    let mut rec = InstanceRecord::new(&args.name);
    rec.state = InstanceState::Starting;
    rec.desired_state = DesiredState::Running;
    rec.port = Some(port);
    rec.pid = Some(pid);
    // process_group(0) 使子进程成为新组的组长，故 pgid == pid
    rec.pgid = Some(pid as i32);
    rec.boot_id = identity::boot_id();
    rec.pid_start_ticks = identity::pid_start_ticks(pid);
    rec.started_at = Some(now_rfc3339());
    rec.workspace = workspace.map(|w| w.display().to_string());
    rec.config = args.serve_config.clone();
    rec.log = Some(spec.log_path.display().to_string());
    rec.env_keys = spawned.env_keys;
    // 落 `--yes`：`restart` 只凭记录重建规格，缺了它会把「跳过白名单」的实例
    // 在重启后悄悄变成「白名单生效」（见 `restart` 里的使用点）。
    rec.yes = args.yes;
    rec.managed_by = Some(args.origin.as_str().to_string());
    // `profile` 与 `profile_in_use` 在启动时必然相同：差异只出现在
    // 「声明改了 profile、实例还没 restart」，由 `supervise` 的计划发现。
    rec.profile = args.profile.clone();
    rec.profile_in_use = args.profile.clone();
    rec.profile_fingerprint = args.profile_fingerprint.clone();

    // 先落盘再等 readiness：ctl 若在此后崩溃，孤儿实例仍有记录可被 prune/ls 发现
    record::write_atomic(&rec_path, &rec)?;
    Ok(rec)
}

/// readiness 通过后补齐 `ready` 字段（`base_url` / `config_in_use`）。
fn finalize_ready(
    ctx: &Ctx,
    mut rec: InstanceRecord,
    port: u16,
) -> Result<InstanceRecord, CtlError> {
    rec.state = InstanceState::Ready;
    rec.last_error = None;
    rec.base_url = Some(format!("http://127.0.0.1:{port}/"));

    if let Some(log) = rec.log.as_deref()
        && let Some(j) = ready_json_from_log(Path::new(log))
    {
        if let Some(url) = j.get("url").and_then(|v| v.as_str()) {
            rec.base_url = Some(url.to_string());
        }
        // 只有 serve 真的加载了配置文件才对得上；未加载时为 null
        rec.config_in_use = j
            .get("config")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }

    let path = ctx.record_path(&rec.name)?;
    record::write_atomic(&path, &rec)?;
    Ok(rec)
}

/// readiness 超时的终态：**回收自己刚起的进程组**，并把记录落为 `failed`。
///
/// 停在 `starting` 是错的，代价很具体：`probe::observe` 对「`starting` + 进程活着 +
/// 端口不通」给的是 `Observed::Starting`，而 `restart::is_failure` 只认
/// `failed` / `stale` / `stopped` —— 巡检于是**永远跳过**它，实例既不重试也不回收
/// （即 #1177 / #1180 记录的「`starting` 永不自愈」），客户端也只会看到一个二义的
/// 「`starting` + `last_error`」，判不出该继续等还是该报错。
fn mark_readiness_timeout(
    ctx: &Ctx,
    mut rec: InstanceRecord,
    pid: u32,
    timeout: Duration,
) -> Result<InstanceRecord, CtlError> {
    let mut note = format!("TCP 未在 {timeout:?} 内就绪（pid {pid}），已回收该进程组");
    match signal::terminate(pid, rec.pgid, DEFAULT_GRACE, POLL) {
        Ok(s) if s.forced => {
            note.push_str("；宽限期不够，动用了 SIGKILL（serve 可能阻塞在关闭流程上）");
        }
        Ok(_) => {}
        // 回收失败不改变结论：这次启动确实没成功，记录必须如实反映
        Err(e) => note.push_str(&format!("；但回收失败：{}", e.message)),
    }
    rec.state = InstanceState::Failed;
    rec.pid = None;
    rec.pgid = None;
    rec.base_url = None;
    rec.last_error = Some(format!(
        "{note}；详见日志 {}",
        rec.log.as_deref().unwrap_or("(未知)")
    ));
    let path = ctx.record_path(&rec.name)?;
    record::write_atomic(&path, &rec)?;
    Ok(rec)
}

/// 单次 `up` 尝试（指定端口）。
fn up_once(
    ctx: &Ctx,
    args: &UpArgs,
    workspace: Option<&Path>,
    port: u16,
    warnings: &[String],
) -> Result<UpOutcome, CtlError> {
    let rec = spawn_and_record(ctx, args, workspace, port)?;

    // 已健康（锁内二次确认命中）：直接复用
    if rec.state == InstanceState::Ready {
        return Ok(UpOutcome {
            observed: Observed::Ready,
            record: rec,
            already_running: true,
            warnings: warnings.to_vec(),
        });
    }

    let pid = rec.pid.unwrap_or(0);
    let addr = readiness::parse_addr("127.0.0.1", port)?;
    match readiness::wait_tcp(addr, pid, args.timeout, POLL) {
        WaitOutcome::Ready => {
            let rec = finalize_ready(ctx, rec, port)?;
            Ok(UpOutcome {
                record: rec,
                already_running: false,
                observed: Observed::Ready,
                warnings: warnings.to_vec(),
            })
        }
        WaitOutcome::Timeout => {
            let rec = mark_readiness_timeout(ctx, rec, pid, args.timeout)?;
            Ok(UpOutcome {
                record: rec,
                already_running: false,
                observed: Observed::Failed,
                warnings: warnings.to_vec(),
            })
        }
        WaitOutcome::Exited => {
            let mut rec = rec;
            rec.state = InstanceState::Failed;
            rec.last_error = Some(format!(
                "serve 启动后立即退出（pid {pid}）；详见日志 {}",
                rec.log.as_deref().unwrap_or("(未知)")
            ));
            let path = ctx.record_path(&rec.name)?;
            record::write_atomic(&path, &rec)?;
            Err(CtlError::general(
                "readiness_failed",
                format!(
                    "实例 {} 启动失败：serve 在就绪前退出（日志 {}）",
                    args.name,
                    rec.log.as_deref().unwrap_or("(未知)")
                ),
            ))
        }
    }
}

/// `up` 的前置检查结果。
///
/// `Existing` 装箱：`InstanceRecord` 约 360 字节，而 `New` 只有几十字节，
/// 直接内联会让整个枚举每个实例都占 360 字节。
enum Launch {
    /// 已存在且健康：直接复用，不再 spawn。
    Existing(Box<InstanceRecord>),
    /// 需要启动。
    New {
        /// `None` = 待绑定（子 serve 以 `--workspace-unset` 启动，等 client 设置）。
        workspace: Option<PathBuf>,
        port: u16,
        /// 预检提示，透传到 `UpOutcome` 由输出层打印。
        warnings: Vec<String>,
    },
}

/// 选定本次要用的端口。
///
/// 显式 `--port` 必须先做占用检查。原因不是洁癖：readiness 只问「这个地址能不能
/// 连上」，端口被别人占着时连到的就是**别人**，于是自己的实例被判定 `ready`、
/// `base_url` 指向别人的进程（#1177 的复现：两个 lane 的 `base_url` 一模一样，
/// 客户端连过去全落到同一个 serve）。用户在这个场景里需要的是一句明确的话。
///
/// auto 分配不需要再查：`first_free` 已经跳过占用端口；从它到 `spawn` 之间的
/// TOCTOU 窗口由 `up` 的 `port_race` 顺延兜住（那是只有 auto 才能做的补救——
/// 显式端口若是被抢，顺延会违背用户的明确要求）。
fn select_port(ctx: &Ctx, args: &UpArgs) -> Result<u16, CtlError> {
    match args.port {
        Some(p) => {
            if !PortPool::is_free(p) {
                return Err(CtlError::conflict(
                    "port_in_use",
                    format!(
                        "端口 {p} 已被占用（127.0.0.1）；换一个 `--port`，或先 `crabmatectl down` 掉占用它的实例"
                    ),
                ));
            }
            Ok(p)
        }
        None => ctx.pool.first_free(),
    }
}

/// 解析并校验工作区：省略（或空白）= 待绑定，返回 `None`；给了就必须存在且是目录。
fn resolve_workspace(raw: Option<&str>) -> Result<Option<PathBuf>, CtlError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let p = paths::absolutize(raw)?;
    if !p.is_dir() {
        return Err(CtlError::usage(
            "bad_workspace",
            format!("workspace 不存在或不是目录：{}", p.display()),
        ));
    }
    Ok(Some(p))
}

/// 启动前的提示：有工作区才跑白名单预检；待绑定只给一条说明。
///
/// 预检不是授权判断——通过不等于 serve 会接受该 workspace，且绝不自动加白。
fn preflight_warnings(
    serve_config: Option<&str>,
    workspace: Option<&Path>,
) -> Result<Vec<String>, CtlError> {
    match workspace {
        Some(ws) => Ok(preflight::check(serve_config, ws)?.warnings),
        None => Ok(vec![
            "该实例以待绑定工作区启动：工具面在 client 设置工作区前不可用（`POST /workspace`）"
                .to_string(),
        ]),
    }
}

/// 校验参数、判幂等、选定端口（不含 spawn；拆出来压 `up` 的复杂度）。
fn prepare_up(ctx: &Ctx, args: &UpArgs) -> Result<Launch, CtlError> {
    paths::validate_name(&args.name)?;
    let workspace = resolve_workspace(args.workspace.as_deref())?;
    ctx.paths.ensure_dirs()?;

    if let Some(rec) = existing_if_ready(ctx, &args.name)? {
        return Ok(Launch::Existing(Box::new(rec)));
    }

    // 记录指向的进程若还活着（身份校验通过），覆盖它就是放生一个孤儿：
    // 新记录指向新 pid，旧进程继续占着端口与实例目录，而谁也不再认识它。
    guard::ensure_not_live(ctx, &args.name)?;

    let port = select_port(ctx, args)?;
    let warnings = preflight_warnings(args.serve_config.as_deref(), workspace.as_deref())?;
    Ok(Launch::New {
        workspace,
        port,
        warnings,
    })
}

/// 启动实例；同名且健康时幂等返回。
pub fn up(ctx: &Ctx, args: &UpArgs) -> Result<UpOutcome, CtlError> {
    let (workspace, port, warnings) = match prepare_up(ctx, args)? {
        Launch::Existing(rec) => {
            return Ok(UpOutcome {
                record: *rec,
                already_running: true,
                observed: Observed::Ready,
                warnings: Vec::new(),
            });
        }
        Launch::New {
            workspace,
            port,
            warnings,
        } => (workspace, port, warnings),
    };

    match up_once(ctx, args, workspace.as_deref(), port, &warnings) {
        Ok(out) => Ok(out),
        // 端口竞争：池内顺延重试一次，仍失败则把两个原因都说清
        Err(e) if e.error_code == "readiness_failed" && args.port.is_none() => {
            let retry = ctx.pool.next_free_from(port)?;
            up_once(ctx, args, workspace.as_deref(), retry, &warnings).map_err(|e2| {
                CtlError::general(
                    "port_race",
                    format!("端口 {port} 启动失败，顺延到 {retry} 仍失败：{}", e2.message),
                )
            })
        }
        Err(e) => Err(e),
    }
}

/// 停止实例：先验身份再整组终止，并写入 `desired_state = stopped`。
pub fn down(ctx: &Ctx, name: &str, grace: Duration) -> Result<DownOutcome, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let Some(mut rec) = record::read_optional(&path)? else {
        return Ok(already_stopped(name));
    };

    // 先落 desired_state：即使随后杀进程失败，supervise 也不会立刻把实例拉起来
    rec.desired_state = DesiredState::Stopped;
    record::write_atomic(&path, &rec)?;

    down_record(name, &path, rec, grace)
}

/// 「本就没有在跑」的幂等结果。
fn already_stopped(name: &str) -> DownOutcome {
    DownOutcome {
        name: name.to_string(),
        already_stopped: true,
        forced: false,
    }
}

/// 把记录的期望态改回 `running`：`down` 留下的「用户显式停止」由声明侧收回。
///
/// 存在的理由与 `restart::book` 里那段注释同源：`down` 会写
/// `desired_state = stopped`，而 `supervise` 把该标记当作**用户意图**，优先于声明
/// 里的 `enabled = true`（`step_for_decl`）。控制面的 `enable` 是本进程发起的动作、
/// 不是用户 `down`，所以必须先把标记清掉 —— 否则 `enable` 之后的收敛恒定 `Skipped`，
/// 实例永远起不来，而声明里 `enabled = true` 又是对的，排查会毫无线索。
///
/// 无记录 / 本就 `running` 时返回 `Ok(false)`（不写盘）。
pub fn mark_wanted_running(ctx: &Ctx, name: &str) -> Result<bool, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let Some(mut rec) = record::read_optional(&path)? else {
        return Ok(false);
    };
    if rec.desired_state == DesiredState::Running {
        return Ok(false);
    }
    rec.desired_state = DesiredState::Running;
    record::write_atomic(&path, &rec)?;
    Ok(true)
}

/// 真正发信号的部分：把「没有存活的 pid」收敛成一个分支，压 `down` 的复杂度。
fn down_record(
    name: &str,
    path: &Path,
    mut rec: InstanceRecord,
    grace: Duration,
) -> Result<DownOutcome, CtlError> {
    let Some(pid) = rec.pid.filter(|p| identity::pid_alive(*p)) else {
        finish_stopped(path, &mut rec)?;
        return Ok(already_stopped(name));
    };

    // 身份校验：PID 复用时会指到无关进程，必须拒绝而不是照杀
    let ident = identity::verify(&rec);
    if !ident.is_match() {
        return Err(CtlError::general(
            "identity_mismatch",
            format!(
                "拒绝终止实例 {name}：{}。记录可能与实际进程不符，请人工确认后执行 `crabmatectl prune`",
                ident.reason().unwrap_or("进程身份校验未通过")
            ),
        ));
    }

    let summary = signal::terminate(pid, rec.pgid, grace, POLL)?;
    finish_stopped(path, &mut rec)?;
    Ok(DownOutcome {
        name: name.to_string(),
        already_stopped: false,
        forced: summary.forced,
    })
}

fn finish_stopped(path: &Path, rec: &mut InstanceRecord) -> Result<(), CtlError> {
    rec.state = InstanceState::Stopped;
    rec.pid = None;
    rec.pgid = None;
    rec.base_url = None;
    // 显式停下就清零退避：下一次 `up` 不该被上一轮崩溃的等待时间挡住
    rec.restart_attempts = 0;
    rec.next_restart_at = None;
    record::write_atomic(path, rec)
}

/// 重启：**优先按声明**重建启动规格，记录只作未声明实例的回落；先 down 再 up。
///
/// 规格为什么必须按声明重建、以及记录这一路的 env 为什么恢复不了，见
/// [`super::relaunch::plan_restart`] 与 `docs/命令行与路由.md` 关键语义第 14 条。
pub fn restart(
    ctx: &Ctx,
    name: &str,
    grace: Duration,
    timeout: Duration,
) -> Result<UpOutcome, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let Some(rec) = record::read_optional(&path)? else {
        return Err(CtlError::general(
            "instance_not_found",
            format!("实例 {name} 不存在；请先 `crabmatectl up {name} ...`"),
        ));
    };
    // 规格必须在 `down` **之前**算完：声明里若出现敏感 env 或坏 profile 名，宁可这次
    // 什么都不做，也别先把实例停掉再报错。
    let plan = super::relaunch::load_restart_plan(
        &rec,
        &ctx.paths.instances_file(),
        &ctx.paths.profiles_file(),
        timeout,
    )?;

    down(ctx, name, grace)?;
    let mut out = up(ctx, &plan.args)?;
    out.warnings.extend(plan.notes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    #[test]
    fn down_missing_instance_is_idempotent_success() {
        let tmp = tempdir("down_missing");
        let ctx = ctx_at(&tmp);
        let out = down(&ctx, "nope", Duration::from_millis(50)).expect("应成功");
        assert!(out.already_stopped);
    }

    #[test]
    fn down_rejects_identity_mismatch_without_signalling() {
        let tmp = tempdir("down_mismatch");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");
        // 伪造一条指向自身（非 serve）进程的记录
        let mut rec = InstanceRecord::new("bad");
        rec.state = InstanceState::Ready;
        rec.pid = Some(std::process::id());
        rec.pgid = Some(std::process::id() as i32);
        rec.boot_id = identity::boot_id();
        rec.pid_start_ticks = identity::pid_start_ticks(std::process::id());
        rec.workspace = Some("/nope".to_string());
        rec.port = Some(1);
        let path = ctx.paths.record_path("bad").expect("路径");
        record::write_atomic(&path, &rec).expect("写记录");

        let err = down(&ctx, "bad", Duration::from_millis(50)).expect_err("身份不符应拒绝");
        assert_eq!(err.error_code, "identity_mismatch");
        // 自身进程必须仍活着（说明没有误杀）
        assert!(identity::pid_alive(std::process::id()));
        // desired_state 已置 stopped（避免 supervise 拉起）
        let after = record::read_required(&path).expect("读回");
        assert_eq!(after.desired_state, DesiredState::Stopped);
    }

    /// readiness 超时：记录落 `failed`（并通过 `signal::terminate` 回收进程）。
    ///
    /// 用**无效 pid** 当目标：`signal::terminate` 对超范围 pid 直接视为「已达成」，
    /// 于是这条用例只钉状态迁移，不会真的动进程。「真杀进程 / 整组回收」由
    /// `signal::terminates_child_process_group` 覆盖 —— 那里若用替身进程，
    /// 被杀后成僵尸会让 `wait_gone` 走满宽限期，测试白等 10s。
    ///
    /// 修好之前这条路径会把记录留在 `starting`，而 `restart::is_failure` 不认
    /// `starting` ⇒ 巡检永远不碰它（#1181 / #1177）。
    #[test]
    fn readiness_timeout_marks_record_failed() {
        let tmp = tempdir("up_timeout");
        let ctx = ctx_at(&tmp);
        // `CtlPaths::for_test` 不建状态目录，写记录前必须先建
        let rec_path = ctx.record_path("x").expect("记录路径");
        std::fs::create_dir_all(rec_path.parent().expect("父目录")).expect("建状态目录");
        let pid = u32::MAX;
        let mut rec = InstanceRecord::new("x");
        rec.state = InstanceState::Starting;
        rec.pid = Some(pid);
        // `pgid = None` 走 Pid 模式：真实路径上组长是 serve 自己
        let out = mark_readiness_timeout(&ctx, rec, pid, Duration::from_secs(1)).expect("应写入记录");

        assert_eq!(out.state, InstanceState::Failed);
        assert!(out.pid.is_none() && out.pgid.is_none());
        assert!(out.base_url.is_none());
        let last_error = out.last_error.clone().expect("应有 last_error");
        assert!(
            last_error.contains("TCP 未在"),
            "last_error 应说明超时：{last_error}"
        );

        let read = record::read_required(&rec_path).expect("应能读回记录");
        assert_eq!(read.state, InstanceState::Failed, "记录必须落 failed");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn up_rejects_missing_workspace() {
        let tmp = tempdir("up_badws");
        let ctx = ctx_at(&tmp);
        let args = UpArgs {
            name: "x".to_string(),
            workspace: Some(tmp.join("definitely-not-here").display().to_string()),
            serve_config: None,
            port: None,
            yes: false,
            env: BTreeMap::new(),
            timeout: Duration::from_millis(100),
            origin: Origin::Cli,
            profile: None,
            profile_fingerprint: None,
            no_tools: false,
        };
        let err = up(&ctx, &args).expect_err("不存在的 workspace 应被拒绝");
        assert_eq!(err.error_code, "bad_workspace");
    }

    /// 显式 `--port` 撞上已被监听的端口：必须在 spawn 之前就失败，报 `port_in_use`。
    /// 修好之前这条路径会「成功」，写出一份 `base_url` 指向别人进程的记录（#1177）。
    #[test]
    fn up_rejects_port_already_in_use() {
        let tmp = tempdir("up_portbusy");
        let ctx = ctx_at(&tmp);
        let ws = tmp.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        // 请求一个真实被监听的端口：bind 到 0 后**保持 listener 存活**，端口才算占用。
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("绑定随机端口");
        let port = held.local_addr().expect("取本地地址").port();
        let args = UpArgs {
            name: "x".to_string(),
            workspace: Some(ws.display().to_string()),
            serve_config: None,
            port: Some(port),
            yes: true,
            env: BTreeMap::new(),
            timeout: Duration::from_millis(100),
            origin: Origin::Cli,
            profile: None,
            profile_fingerprint: None,
            no_tools: false,
        };
        let err = up(&ctx, &args).expect_err("被占用的端口应被拒绝");
        assert_eq!(err.error_code, "port_in_use");
        assert_eq!(err.exit_code, crate::EXIT_USAGE);
        assert!(
            err.message.contains(&port.to_string()),
            "错误信息应点出端口号，便于定位：{}",
            err.message
        );
        drop(held);
    }

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cm_ctl_ops_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }
}
