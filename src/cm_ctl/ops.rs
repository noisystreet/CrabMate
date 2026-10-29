//! 编排层：把记录 / 端口 / spawn / 信号 / 观测拼成各子命令的动作。
//!
//! 这里不含 clap 类型，也不直接打印（打印在 [`super::render`]），便于单测。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::identity;
use super::lock;
use super::paths::{self, CtlPaths};
use super::port_pool::PortPool;
use super::preflight;
use super::probe::{self, Observed, Probe};
use super::profiles;
use super::readiness::{self, WaitOutcome};
use super::record::{self, DesiredState, InstanceRecord, InstanceState};
use super::signal;
use super::spawn::{self, SpawnSpec};
use super::CtlError;

/// 轮询间隔（readiness 与信号等待共用）。
const POLL: Duration = Duration::from_millis(100);
/// 默认就绪超时。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
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
    pub workspace: String,
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
    workspace: &Path,
    port: u16,
) -> SpawnSpec {
    let log_path = ctx.paths.log_path(&args.name);
    SpawnSpec {
        workspace: workspace.to_path_buf(),
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
    workspace: &Path,
    port: u16,
) -> Result<InstanceRecord, CtlError> {
    let rec_path = ctx.record_path(&args.name)?;
    let _guard = lock::acquire(&ctx.lock_path())?;

    // 锁内二次确认，避免两个 ctl 同时通过前置检查
    if let Some(rec) = existing_if_ready(ctx, &args.name)? {
        return Ok(rec);
    }

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
    rec.workspace = Some(workspace.display().to_string());
    rec.config = args.serve_config.clone();
    rec.log = Some(spec.log_path.display().to_string());
    rec.env_keys = spawned.env_keys;
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

/// 单次 `up` 尝试（指定端口）。
fn up_once(
    ctx: &Ctx,
    args: &UpArgs,
    workspace: &Path,
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
            // 进程活着但 TCP 未就绪：**不杀**，置 starting + last_error 交由用户判断
            let mut rec = rec;
            rec.last_error = Some(format!(
                "TCP 未在 {:?} 内就绪，但进程仍存活（pid {}）；可能需要更长的 --timeout",
                args.timeout, pid
            ));
            let path = ctx.record_path(&rec.name)?;
            record::write_atomic(&path, &rec)?;
            Ok(UpOutcome {
                record: rec,
                already_running: false,
                observed: Observed::Starting,
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
        workspace: PathBuf,
        port: u16,
        /// 预检提示，透传到 `UpOutcome` 由输出层打印。
        warnings: Vec<String>,
    },
}

/// 校验参数、判幂等、选定端口（不含 spawn；拆出来压 `up` 的复杂度）。
fn prepare_up(ctx: &Ctx, args: &UpArgs) -> Result<Launch, CtlError> {
    paths::validate_name(&args.name)?;
    let workspace = paths::absolutize(&args.workspace)?;
    if !workspace.is_dir() {
        return Err(CtlError::usage(
            "bad_workspace",
            format!("workspace 不存在或不是目录：{}", workspace.display()),
        ));
    }
    ctx.paths.ensure_dirs()?;

    if let Some(rec) = existing_if_ready(ctx, &args.name)? {
        return Ok(Launch::Existing(Box::new(rec)));
    }

    let port = match args.port {
        Some(p) => p,
        None => ctx.pool.first_free()?,
    };
    // 友好预检：越界才阻断（`workspace_out_of_roots`），未给 `--serve-config` 只提示。
    // 这不是授权判断——预检通过不等于 serve 会接受该 workspace，且绝不自动加白。
    let pf = preflight::check(args.serve_config.as_deref(), &workspace)?;
    Ok(Launch::New {
        workspace,
        port,
        warnings: pf.warnings,
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

    match up_once(ctx, args, &workspace, port, &warnings) {
        Ok(out) => Ok(out),
        // 端口竞争：池内顺延重试一次，仍失败则把两个原因都说清
        Err(e) if e.error_code == "readiness_failed" && args.port.is_none() => {
            let retry = ctx.pool.next_free_from(port)?;
            up_once(ctx, args, &workspace, retry, &warnings).map_err(|e2| {
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

/// 按实例记录里的 profile 名重新解析预设（`restart` 用）。
///
/// 这段语义值得一个函数名而不只是内联：**重启必须按当前 `profiles.toml` 重建
/// 权限集合**，否则 `--no-tools` / `allowed_commands` 会在重启后消失——
/// 而那是向**放松**方向漂移，比报错更危险。
fn resolve_recorded_profile(
    ctx: &Ctx,
    rec: &InstanceRecord,
) -> Result<Option<profiles::Resolved>, CtlError> {
    let Some(name) = rec.profile.as_deref() else {
        return Ok(None);
    };
    let path = ctx.paths.profiles_file();
    let file = profiles::load_optional(&path)?;
    Ok(Some(profiles::resolve(
        &file,
        name,
        &BTreeMap::new(),
        &path,
    )?))
}

/// 重启：复用原有 workspace / 端口 / 配置，先 down 再 up。
pub fn restart(ctx: &Ctx, name: &str, grace: Duration, timeout: Duration) -> Result<UpOutcome, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let Some(rec) = record::read_optional(&path)? else {
        return Err(CtlError::general(
            "instance_not_found",
            format!("实例 {name} 不存在；请先 `crabmatectl up {name} ...`"),
        ));
    };
    let workspace = rec.workspace.clone().ok_or_else(|| {
        CtlError::general(
            "record_incomplete",
            format!("实例 {name} 的记录缺少 workspace，无法重启；请重新 up"),
        )
    })?;
    // profile 存在时按**当前** profiles.toml 重新解析：`restart` 是「按声明重建」，
    // 若把 `--no-tools` / `allowed_commands` 丢掉，实例会在重启后悄悄变成另一种权限集合
    // （而方向可能是放松）。这也正是「profile 变更需 restart 才生效」的落点。
    let resolved = resolve_recorded_profile(ctx, &rec)?;
    let port = rec.port;
    let serve_config = resolved
        .as_ref()
        .and_then(|r| r.serve_config.clone())
        .or_else(|| rec.config.clone());
    let env = resolved.as_ref().map(|r| r.env.clone()).unwrap_or_default();
    let no_tools = resolved.as_ref().map(|r| r.no_tools).unwrap_or(false);
    let fingerprint = resolved.as_ref().map(|r| r.fingerprint());

    // 沿用原持有者：`supervise` 拉起的实例被人工 restart 后仍归 `supervise` 回收，
    // 否则它会变成「无主的 supervise 实例」——既不被声明回收，也不被声明接管。
    let origin = Origin::from_record(&rec);

    down(ctx, name, grace)?;

    let args = UpArgs {
        name: name.to_string(),
        workspace,
        serve_config,
        port,
        yes: false,
        env,
        timeout,
        origin,
        profile: rec.profile.clone(),
        profile_fingerprint: fingerprint,
        no_tools,
    };
    up(ctx, &args)
}

/// 列出全部实例（含派生观测态）。
pub fn ls(ctx: &Ctx) -> Result<Vec<(InstanceRecord, Probe)>, CtlError> {
    let mut out = Vec::new();
    for path in ctx.paths.all_record_paths()? {
        let rec = match record::read_optional(&path) {
            Ok(Some(r)) => r,
            // 坏记录不应让整个 ls 失败
            Ok(None) | Err(_) => continue,
        };
        let p = probe::observe(&rec);
        out.push((rec, p));
    }
    out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    Ok(out)
}

/// 单实例详情。
pub fn status(ctx: &Ctx, name: &str) -> Result<(InstanceRecord, Probe), CtlError> {
    paths::validate_name(name)?;
    let path = ctx.record_path(name)?;
    let rec = record::read_required(&path)?;
    let p = probe::observe(&rec);
    Ok((rec, p))
}

/// 读日志尾部。
pub fn logs(ctx: &Ctx, name: &str, tail: usize) -> Result<String, CtlError> {
    paths::validate_name(name)?;
    let path = ctx.paths.log_path(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CtlError::general(
                "log_not_found",
                format!("实例 {name} 尚无日志文件：{}", path.display()),
            ));
        }
        Err(e) => {
            return Err(CtlError::general(
                "log_unreadable",
                format!("读取日志失败 {}：{e}", path.display()),
            ));
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail.max(1));
    Ok(lines[start..].join("\n"))
}

/// 清理 stale / 已退出 / 失败的记录。
pub fn prune(ctx: &Ctx, dry_run: bool) -> Result<Vec<(String, Observed)>, CtlError> {
    let mut removed = Vec::new();
    for (rec, p) in ls(ctx)? {
        if !p.observed.is_prunable() {
            continue;
        }
        removed.push((rec.name.clone(), p.observed));
        if !dry_run {
            let path = ctx.record_path(&rec.name)?;
            record::remove(&path)?;
        }
    }
    Ok(removed)
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
    fn ls_on_empty_state_dir_returns_empty() {
        let tmp = tempdir("ls_empty");
        let ctx = ctx_at(&tmp);
        assert!(ls(&ctx).expect("应成功").is_empty());
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

    #[test]
    fn status_reports_missing() {
        let tmp = tempdir("status_missing");
        let ctx = ctx_at(&tmp);
        let err = status(&ctx, "ghost").expect_err("应报不存在");
        assert_eq!(err.error_code, "instance_not_found");
    }

    #[test]
    fn prune_removes_only_terminal_states() {
        let tmp = tempdir("prune");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");

        let mut stopped = InstanceRecord::new("stopped-one");
        stopped.state = InstanceState::Stopped;
        let p1 = ctx.paths.record_path("stopped-one").expect("路径");
        record::write_atomic(&p1, &stopped).expect("写");

        // 该记录指向不存在的 pid ⇒ stopped
        let mut dead = InstanceRecord::new("dead-one");
        dead.state = InstanceState::Ready;
        dead.pid = Some(u32::MAX);
        let p2 = ctx.paths.record_path("dead-one").expect("路径");
        record::write_atomic(&p2, &dead).expect("写");

        let dry = prune(&ctx, true).expect("dry-run");
        assert_eq!(dry.len(), 2, "dry-run 应列出但不动文件：{dry:?}");
        assert!(p1.exists() && p2.exists());

        let real = prune(&ctx, false).expect("实删");
        assert_eq!(real.len(), 2);
        assert!(!p1.exists() && !p2.exists());
    }

    #[test]
    fn logs_reports_missing_file() {
        let tmp = tempdir("logs_missing");
        let ctx = ctx_at(&tmp);
        let err = logs(&ctx, "nope", 10).expect_err("应报无日志");
        assert_eq!(err.error_code, "log_not_found");
    }

    #[test]
    fn up_rejects_missing_workspace() {
        let tmp = tempdir("up_badws");
        let ctx = ctx_at(&tmp);
        let args = UpArgs {
            name: "x".to_string(),
            workspace: tmp.join("definitely-not-here").display().to_string(),
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
