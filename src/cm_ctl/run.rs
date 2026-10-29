//! CLI 分派：把 `cm_config::cli::ctl` 的 clap 类型翻译成 `ops` 动作并输出。
//!
//! 分层：clap 定义在 `cm_config`（配置层），本模块只做「翻译 + 调用 + 打印」。
//! 阻塞式实现是刻意的：`ctl` 是短命命令行工具，不存在并发命令需要推进。

use std::path::PathBuf;
use std::time::Duration;

use crate::config::cli::ctl::{
    CtlCmd, CtlSubCmd, DownCmd, LogsCmd, RunCmd, SuperviseCmd, UpCmd,
};

use super::control;
use super::identity;
use super::instances;
use super::ops::{self, Ctx, Origin, UpArgs, DEFAULT_GRACE};
use super::paths;
use super::preflight;
use super::record::{self, DesiredState, InstanceRecord, InstanceState};
use super::render;
use super::spawn::{self, SpawnSpec};
use super::supervise;
use super::CtlError;

/// `ctl` 入口：`workspace_cli` 来自全局 `--workspace`。
pub fn run(cmd: &CtlCmd, workspace_cli: Option<&str>) -> Result<(), CtlError> {
    let ctx = Ctx::resolve()?;
    match &cmd.sub {
        CtlSubCmd::Up(c) => {
            let out = ops::up(&ctx, &up_args(c, workspace_cli)?)?;
            render::up(&out);
            Ok(())
        }
        CtlSubCmd::Down(c) => down_cmd(&ctx, c),
        CtlSubCmd::Restart(c) => {
            let out = ops::restart(
                &ctx,
                &c.name,
                DEFAULT_GRACE,
                Duration::from_secs(c.timeout_secs),
            )?;
            render::up(&out);
            Ok(())
        }
        CtlSubCmd::Run(c) => run_foreground(&ctx, c, workspace_cli),
        CtlSubCmd::Ls(c) => {
            let rows = ops::ls(&ctx)?;
            render::ls(&rows, c.json);
            Ok(())
        }
        CtlSubCmd::Status(c) => {
            let (rec, p) = ops::status(&ctx, &c.name)?;
            render::status(&rec, &p, c.json);
            Ok(())
        }
        CtlSubCmd::Logs(c) => logs_cmd(&ctx, c),
        CtlSubCmd::Prune(c) => {
            let items = ops::prune(&ctx, c.dry_run)?;
            render::prune(&items, c.dry_run);
            Ok(())
        }
        CtlSubCmd::Supervise(c) => supervise_cmd(&ctx, c),
    }
}

fn require_workspace(workspace_cli: Option<&str>, what: &str) -> Result<String, CtlError> {
    workspace_cli.map(str::to_string).ok_or_else(|| {
        CtlError::usage(
            "missing_workspace",
            format!("{what} 需要 `--workspace <PATH>`（全局选项，写在子命令前后均可）"),
        )
    })
}

fn up_args(c: &UpCmd, workspace_cli: Option<&str>) -> Result<UpArgs, CtlError> {
    Ok(UpArgs {
        name: c.name.clone(),
        workspace: require_workspace(workspace_cli, "ctl up")?,
        serve_config: c.serve_config.clone(),
        port: c.port,
        yes: c.yes,
        env: Default::default(),
        timeout: Duration::from_secs(c.timeout_secs),
        origin: Origin::Cli,
        // 人工 `up` 不走 profile：预设只由声明式收敛（`supervise`）解析
        profile: None,
        profile_fingerprint: None,
        no_tools: false,
    })
}

/// `supervise`：读声明 → 计划 → 执行（`--watch` 则循环）。
fn supervise_cmd(ctx: &Ctx, c: &SuperviseCmd) -> Result<(), CtlError> {
    require_from_config(c)?;
    let path = match c.instances_file.as_deref() {
        Some(p) => paths::absolutize(p)?,
        None => ctx.paths.instances_file(),
    };
    let opts = supervise::Opts {
        timeout: Duration::from_secs(c.timeout_secs),
        grace: Duration::from_secs(c.grace_secs),
        dry_run: c.dry_run,
    };
    if let Some(spec) = c.socket.as_deref() {
        return socket_mode(ctx, c, &path, &opts, spec);
    }
    if c.watch {
        return watch_mode(ctx, c, &path, &opts);
    }
    let file = instances::load(&path)?;
    let steps = supervise::execute(ctx, supervise::plan(ctx, &file)?, &opts);
    render::supervise(&steps, c.dry_run);

    let failed = steps.iter().filter(|s| s.has_failed()).count();
    if failed == 0 {
        return Ok(());
    }
    Err(CtlError::general(
        "supervise_partial_failure",
        format!("{failed} 个实例未能收敛到期望态（详见上方输出）"),
    ))
}

fn watch_mode(
    ctx: &Ctx,
    c: &SuperviseCmd,
    path: &std::path::Path,
    opts: &supervise::Opts,
) -> Result<(), CtlError> {
    if c.dry_run {
        return Err(CtlError::usage(
            "conflicting_flags",
            "`--watch` 是持续收敛，不能与 `--dry-run` 同时使用",
        ));
    }
    // 0 会退化成忙轮询：每次都重新 stat + load
    let interval = Duration::from_secs(c.interval_secs.max(1));
    println!(
        "监听 {}（间隔 {}s）；声明变化即收敛，Ctrl-C 退出",
        path.display(),
        interval.as_secs()
    );
    supervise::watch(ctx, path, opts, interval)
}

/// `--from-config` 是当前唯一模式（`--watch` 仍需它）。
fn require_from_config(c: &SuperviseCmd) -> Result<(), CtlError> {
    if c.from_config {
        return Ok(());
    }
    Err(CtlError::usage(
        "missing_from_config",
        "`supervise` 需要 `--from-config`（按 instances.toml 收敛）",
    ))
}

/// `--socket`：常驻并接受本地控制面请求（`add` / `remove` / `list`）。
///
/// 主循环独占写盘：请求先改声明、再由本线程收敛，因此不存在「socket 与
/// `--watch` 同时改文件」的交错；配 `--watch` 时额外保留文件指纹轮询。
fn socket_mode(
    ctx: &Ctx,
    c: &SuperviseCmd,
    path: &std::path::Path,
    opts: &supervise::Opts,
    spec: &str,
) -> Result<(), CtlError> {
    if c.dry_run {
        return Err(CtlError::usage(
            "conflicting_flags",
            "`--socket` 是常驻服务，不能与 `--dry-run` 同时使用",
        ));
    }
    let sock = resolve_socket_path(ctx, spec)?;
    let server = control::Server::bind(&sock)?;
    println!(
        "控制面 {}（仅同用户可连）；{}，Ctrl-C 退出",
        sock.display(),
        if c.watch {
            "声明变化与 socket 请求都会收敛"
        } else {
            "仅在收到请求后收敛"
        }
    );
    let interval = Duration::from_secs(c.interval_secs.max(1));
    let mut cursor = supervise::Cursor::new();
    loop {
        let mut changed = false;
        while let Some(incoming) = server.try_next() {
            let (resp, dirty) = control::handle(ctx, path, incoming.request);
            changed |= dirty;
            // 写盘在 `handle` 内已完成，这里才回响应：客户端收到 `ok` 时声明已落盘
            if incoming.reply.send(resp).is_err() {
                eprintln!("控制面请求的响应通道已关闭（客户端可能提前断开）");
            }
        }
        if changed {
            supervise::reconcile_once(ctx, path, opts);
            // 同步指纹：否则 `--watch` 会把本进程刚写的变化再收敛一遍
            cursor.sync(path);
        } else if c.watch {
            cursor.tick(ctx, path, opts);
        }
        std::thread::sleep(interval);
    }
}

/// socket 路径：省略值时用 `$XDG_RUNTIME_DIR/crabmate/ctl.sock`（建 `0700` 目录）；
/// 显式给出则只要求父目录已存在 —— 绝不去替用户改 `/tmp` 这类目录的权限。
fn resolve_socket_path(ctx: &Ctx, spec: &str) -> Result<PathBuf, CtlError> {
    if spec.trim().is_empty() {
        return ctx.paths.prepare_control_socket();
    }
    let p = paths::absolutize(spec)?;
    match p.parent() {
        Some(dir) if dir.is_dir() => Ok(p),
        _ => Err(CtlError::usage(
            "bad_socket_path",
            format!("socket 所在目录不存在：{}", p.display()),
        )),
    }
}

fn down_cmd(ctx: &Ctx, c: &DownCmd) -> Result<(), CtlError> {
    let grace = Duration::from_secs(c.timeout_secs);
    if c.all {
        let rows = ops::ls(ctx)?;
        if rows.is_empty() {
            println!("（没有实例）");
            return Ok(());
        }
        let mut failed = Vec::new();
        for (rec, _) in rows {
            match ops::down(ctx, &rec.name, grace) {
                Ok(out) => render::down(&out),
                Err(e) => {
                    eprintln!("实例 {} 停止失败：{e}", rec.name);
                    failed.push(rec.name);
                }
            }
        }
        if failed.is_empty() {
            return Ok(());
        }
        return Err(CtlError::general(
            "down_partial_failure",
            format!("以下实例未能停止：{}", failed.join(", ")),
        ));
    }
    let name = c.name.as_deref().ok_or_else(|| {
        CtlError::usage("missing_name", "需要实例名，或使用 `--all` 停止全部实例")
    })?;
    let out = ops::down(ctx, name, grace)?;
    render::down(&out);
    Ok(())
}

/// `run` 的 workspace 与端口准备（拆出来压 `run_foreground` 的复杂度）。
fn prepare_run(
    ctx: &Ctx,
    c: &RunCmd,
    workspace_cli: Option<&str>,
) -> Result<(PathBuf, u16, Vec<String>), CtlError> {
    paths::validate_name(&c.name)?;
    let workspace = paths::absolutize(&require_workspace(workspace_cli, "ctl run")?)?;
    if !workspace.is_dir() {
        return Err(CtlError::usage(
            "bad_workspace",
            format!("workspace 不存在或不是目录：{}", workspace.display()),
        ));
    }
    ctx.paths.ensure_dirs()?;

    let port = match c.port {
        Some(p) => p,
        None => ctx.pool.first_free()?,
    };
    // 与 `up` 同一套友好预检（越界才阻断）；`run` 是前台模式，预检同样适用。
    let pf = preflight::check(c.serve_config.as_deref(), &workspace)?;
    Ok((workspace, port, pf.warnings))
}

fn foreground_spec(
    ctx: &Ctx,
    c: &RunCmd,
    workspace: &std::path::Path,
    port: u16,
) -> SpawnSpec {
    SpawnSpec {
        workspace: workspace.to_path_buf(),
        serve_config: c.serve_config.as_ref().map(PathBuf::from),
        port,
        yes: c.yes,
        no_tools: false,
        env: Default::default(),
        log_path: ctx.paths.log_path(&c.name),
        home: ctx.paths.instance_home(&c.name),
        ready_json: false,
        // 前台模式：日志直接进调用方的 stdout/stderr
        inherit_io: true,
        // 前台模式：与 ctl 同组，Ctrl-C / SIGTERM 天然同时到达两者
        own_process_group: false,
    }
}

/// 写 `starting` 记录，返回记录路径供收尾使用。
fn write_run_record(
    ctx: &Ctx,
    c: &RunCmd,
    workspace: &std::path::Path,
    port: u16,
    pid: u32,
    env_keys: Vec<String>,
) -> Result<PathBuf, CtlError> {
    let mut rec = InstanceRecord::new(&c.name);
    rec.state = InstanceState::Starting;
    rec.desired_state = DesiredState::Running;
    rec.port = Some(port);
    rec.pid = Some(pid);
    rec.pgid = Some(pid as i32);
    rec.boot_id = identity::boot_id();
    rec.pid_start_ticks = identity::pid_start_ticks(pid);
    rec.started_at = Some(chrono::Utc::now().to_rfc3339());
    rec.workspace = Some(workspace.display().to_string());
    rec.config = c.serve_config.clone();
    rec.env_keys = env_keys;
    // 前台模式同样归 `cli`：`supervise` 不接管人工起的实例。
    rec.managed_by = Some(Origin::Cli.as_str().to_string());
    let path = ctx.record_path(&c.name)?;
    record::write_atomic(&path, &rec)?;
    Ok(path)
}

/// 收尾落 `stopped`；记录读不到时不影响退出码。
fn mark_run_stopped(path: &std::path::Path) -> Result<(), CtlError> {
    let Ok(Some(mut rec)) = record::read_optional(path) else {
        return Ok(());
    };
    rec.state = InstanceState::Stopped;
    rec.pid = None;
    rec.pgid = None;
    rec.base_url = None;
    record::write_atomic(path, &rec)
}

/// 前台运行：与 `up` 的区别是**不脱管**——ctl 阻塞到子进程退出。
fn run_foreground(ctx: &Ctx, c: &RunCmd, workspace_cli: Option<&str>) -> Result<(), CtlError> {
    let (workspace, port, warnings) = prepare_run(ctx, c, workspace_cli)?;
    render::warn(&warnings);
    let spec = foreground_spec(ctx, c, &workspace, port);
    let mut spawned = spawn::spawn_serve(&spec)?;
    let pid = spawned.pid();
    let rec_path = write_run_record(ctx, c, &workspace, port, pid, spawned.env_keys)?;

    println!("前台运行实例 {}（pid {pid}）；Ctrl-C 结束", c.name);

    let status = spawned
        .child
        .wait()
        .map_err(|e| CtlError::general("wait_failed", format!("等待子进程退出失败：{e}")))?;

    // 无论成功与否都落 stopped，避免留下「ready 但进程已没了」的记录
    mark_run_stopped(&rec_path)?;

    if status.success() {
        Ok(())
    } else {
        Err(CtlError::general(
            "child_failed",
            format!("实例 {} 退出码 {:?}", c.name, status.code()),
        ))
    }
}

fn logs_cmd(ctx: &Ctx, c: &LogsCmd) -> Result<(), CtlError> {
    let text = ops::logs(ctx, &c.name, c.tail)?;
    if !text.is_empty() {
        println!("{text}");
    }
    if !c.follow {
        return Ok(());
    }
    follow_file(&ctx.paths.log_path(&c.name));
}

/// 简单 `tail -f`：轮询文件增长（不引 inotify，容器 overlayfs 上更可靠）。
fn follow_file(path: &std::path::Path) -> ! {
    use std::io::{Read, Seek, SeekFrom};

    let mut offset = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let len = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            // 文件暂时不可读（被删/轮转）：下次再试
            Err(_) => continue,
        };
        if len < offset {
            // 被截断或轮转：从头读
            offset = 0;
        }
        if len <= offset {
            continue;
        }
        if let Ok(mut f) = std::fs::File::open(path) {
            if f.seek(SeekFrom::Start(offset)).is_err() {
                offset = 0;
                continue;
            }
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                print!("{buf}");
                offset = len;
            }
        }
    }
}
