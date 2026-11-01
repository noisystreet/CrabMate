//! 「记录指向的进程是否仍归我们管」——**删记录 / 覆盖记录之前**的最后一道闸门。
//!
//! `down` 发信号前有 `identity` 的三重校验；但 `prune` 删记录、`up` 覆盖记录同样能
//! 让一个**还活着**的进程失去唯一地址：状态文件里的 `pid` 是 ctl 找到那个进程的唯一
//! 入口，记录一旦被删或被改成新 pid，进程就仍在后台跑，而 `ls` 看不见、`down` 报
//! 「实例不存在」、`supervise` 也不会回收它 —— 正是 CHANGELOG 里「读不到事实 ≠
//! 进程已死」那条想避免的孤儿，只是从另一条路径进来。
//!
//! 判据不能只落在 `Observed` 上：`Observed::Stale`（见 `probe.rs` 的映射矩阵）把两件
//! 性质相反的事混成一类：
//!
//! - **身份不符**（PID 被复用，指向无关进程）：记录已无价值，**应当清掉**；
//! - **身份匹配 + 进程活着 + 端口不通**：记录是那个活进程的唯一地址，**不能清**。
//!
//! 所以这里的谓词是 `pid_alive && identity.is_match()`，与 `down` 用的是同一套事实。

use super::ops::Ctx;
use super::probe::{self, Observed, Probe};
use super::record::{self, InstanceRecord};
use super::CtlError;

/// 记录指向的进程仍活着且身份校验通过时，返回**拒绝理由**（反之 `None`）。
///
/// 文本直接面向用户，且被 `prune` 的跳过行与 `up` 的错误信息复用，所以要说清
/// 「发生了什么」和「为什么不能动」。
pub(crate) fn live_process_reason(rec: &InstanceRecord, p: &Probe) -> Option<String> {
    if !p.process_is_live_and_ours() {
        return None;
    }
    Some(format!(
        "记录里的 pid {} 仍在运行、身份校验通过，但观测态为 {}（端口不通）；\
         记录是它唯一的地址，删掉或覆盖它只会留下一个谁都管不到的孤儿进程",
        rec.pid.unwrap_or_default(),
        p.observed.as_str()
    ))
}

/// `up` / `restart` 的前置闸门：记录指向活进程时**拒绝启动**，不覆盖记录。
///
/// 覆盖的后果比报错严重得多：新记录指向新 pid，旧进程继续占着端口与实例目录，而
/// 用户看到的只是「新实例 `failed`」（同目录的 `.serve.lock` 会挡住第二个 serve），
/// 于是那个真正在跑的实例从此谁也不认识。
pub(crate) fn ensure_not_live(ctx: &Ctx, name: &str) -> Result<(), CtlError> {
    let path = ctx.record_path(name)?;
    let Some(rec) = record::read_optional(&path)? else {
        return Ok(());
    };
    let p = probe::observe(&rec);
    let Some(reason) = live_process_reason(&rec, &p) else {
        return Ok(());
    };
    // 退出码走 `conflict`（与 `port_in_use` 同类）：重跑 `up` 不会有不同结果。
    Err(CtlError::conflict(
        "instance_already_live",
        format!("拒绝启动实例 {name}：{reason}。\n{}", next_step(name, p.observed)),
    ))
}

/// 拒绝之后该做什么。
///
/// 按观测态分开，因为 `starting` 是**进行中**而不是「该被清掉的东西」：并发 `up` 或
/// 上一个 ctl 刚被 Ctrl-C 时，正确动作是等它就绪；此时提示「先 `down`」会让人把一个
/// 正在正常启动的实例白杀一遍。
fn next_step(name: &str, observed: Observed) -> String {
    if observed == Observed::Starting {
        return format!(
            "它看起来正在启动中（并发 `up`，或上一个 ctl 没等到就绪就退出了）；\
             先 `crabmatectl status {name}` 看进展，要重来再 `crabmatectl down {name}`"
        );
    }
    format!(
        "请先 `crabmatectl down {name}`（serve 优雅关停较慢时用 `--timeout <SECS>` 放宽宽限），\
         确认进程已退出后重试"
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::cm_ctl::identity;
    use crate::cm_ctl::ops::{self, Ctx, Origin, UpArgs};
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;
    use crate::cm_ctl::query;
    use crate::cm_ctl::record::{self, InstanceRecord, InstanceState};
    use crate::cm_ctl::signal;

    use crate::test_fixtures::tempdir;

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    /// 替身「长得像 serve」的进程。
    ///
    /// `identity::verify` 只认**独立** `serve` token 与 workspace / port（本用例的记录
    /// 两者皆空，该项被跳过），所以这个替身会被判成「本人」—— 正是守卫要拦的情形：
    /// 进程确实活着、身份也对得上，但那个端口永远连不通。
    ///
    /// `sleep 300; :` 两个命令是刻意的：只写 `sleep 300` 时 shell 会用 `exec` 把自己
    /// 替换掉，argv 随之丢掉 `serve` token，身份校验立刻变成 `Mismatch`，用例就测不到
    /// 本意。时长取 300s 而非 30s：用例只需要它活过几十毫秒，但 CI 上的调度停顿不该让
    /// 「替身自己睡醒」变成一次莫名其妙的失败。
    struct Lookalike {
        child: Child,
        /// 替身的 stderr。它若被环境掐掉（`fork` 失败、OOM），原因写在这里；
        /// 少了这个文件，用例只能看到「记录像是死的」，无从下手（#1216）。
        stderr_path: PathBuf,
    }

    impl Lookalike {
        fn spawn(dir: &Path) -> Self {
            let stderr_path = dir.join("lookalike.stderr");
            let stderr = std::fs::File::create(&stderr_path).expect("建替身 stderr 文件");
            let child = Command::new("sh")
                .arg("-c")
                .args(["sleep 300; :", "serve"])
                // 自成一组：收尾时连同 `sleep` 一起结束，不把残骸留给后面几分钟
                .process_group(0)
                .stderr(stderr)
                .spawn()
                .expect("起替身进程");
            Self {
                child,
                stderr_path,
            }
        }

        fn pid(&self) -> u32 {
            self.child.id()
        }

        /// 替身自己说过的话（尾部 400 字符）；正常情形为空。
        fn stderr_tail(&self) -> String {
            let text = std::fs::read_to_string(&self.stderr_path).unwrap_or_default();
            let chars: Vec<char> = text.chars().collect();
            let start = chars.len().saturating_sub(400);
            chars[start..].iter().collect::<String>().trim().to_string()
        }
    }

    impl Drop for Lookalike {
        /// 用生产代码里的**整组**终止，而不是 `Child::kill`：后者只杀 `sh`，真正的
        /// `sleep` 会活到计时结束，测试跑完机器上还留着一堆睡着的进程。
        ///
        /// 还必须真的 reap：`identity::pid_alive` 把僵尸算作「不活」，不 wait 会让
        /// 后续观测静默变成「进程已死」，用例因此变成假绿。
        fn drop(&mut self) {
            let pid = self.child.id();
            let _ = signal::terminate(
                pid,
                Some(pid as i32),
                Duration::from_millis(500),
                Duration::from_millis(20),
            );
            let _ = self.child.wait();
        }
    }

    /// 「进程活着 + 身份校验通过 + 端口不通」的记录（观测态为 `stale`）。
    fn record_of_live(name: &str, pid: u32) -> InstanceRecord {
        let mut rec = InstanceRecord::new(name);
        rec.state = InstanceState::Ready;
        rec.pid = Some(pid);
        rec.pgid = Some(pid as i32);
        rec.boot_id = identity::boot_id();
        rec.pid_start_ticks = identity::pid_start_ticks(pid);
        // workspace / port 留空：身份校验因此跳过值比对，而 TCP 探测必然不通
        rec
    }

    fn write_record(ctx: &Ctx, rec: &InstanceRecord) -> PathBuf {
        let path = ctx.record_path(&rec.name).expect("记录路径");
        record::write_atomic(&path, rec).expect("写记录");
        path
    }

    /// 起一个**可观测为活着**的 serve 替身，等到观测成立才返回。
    ///
    /// 单次 `spawn` 之后立刻读 `/proc` 并不稳：CI 上高负载时替身可能刚起来就被环境
    /// 掐掉（`fork` 失败、内存压力），那一刻记录会被观测成 `stopped` / `stale`，于是
    /// 「守卫必须拒绝」的断言以一句「必须给出拒绝理由」失败 —— 结论没错，但看不出是
    /// 替身没起来（#1216）。所以这里轮询到 `process_is_live_and_ours()` 成立；三次都
    /// 起不来才 panic，并把观测事实与替身 stderr 一并打出来。
    fn live_lookalike(dir: &Path) -> (Lookalike, InstanceRecord) {
        let mut last = String::new();
        for attempt in 1..=3 {
            let live = Lookalike::spawn(dir);
            let rec = record_of_live("live", live.pid());
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let p = probe::observe(&rec);
                if p.process_is_live_and_ours() {
                    return (live, rec);
                }
                last = format!(
                    "第 {attempt} 次：{} / pid_alive={} / identity={:?} / 替身 stderr={:?}",
                    p.observed.as_str(),
                    p.pid_alive,
                    p.identity,
                    live.stderr_tail()
                );
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        panic!("替身进程始终不像一个活着的 serve（记录会因此被判成可清）：{last}");
    }

    /// 活进程的记录**默认不能**被 `prune` 清掉：它是那个进程唯一的地址。
    #[test]
    fn prune_refuses_record_of_live_process() {
        let tmp = tempdir("prune_live");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");
        let (live, rec) = live_lookalike(&tmp);
        let path = write_record(&ctx, &rec);

        let items = query::prune(&ctx, false, false).expect("prune 应成功");
        assert_eq!(items.len(), 1, "记录应被列出（只是被跳过）：{items:?}");
        let Some(reason) = items[0].refused.as_deref() else {
            panic!(
                "必须给出拒绝理由：替身 pid {} 仍活着（观测为 {}），prune 却判成可清：{items:?}",
                live.pid(),
                probe::observe(&rec).observed.as_str()
            );
        };
        assert!(
            reason.contains("仍在运行"),
            "理由要说清进程还活着：{reason}"
        );
        assert!(path.exists(), "活进程的记录不能被清掉");

        // 显式 `--force` 才清：那时孤儿是用户自己确认承担的结果
        let forced = query::prune(&ctx, false, true).expect("prune --force");
        assert!(forced[0].refused.is_none(), "force 时不再拒绝");
        assert!(!path.exists(), "force 应真的删掉记录");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 对照组：身份**不符**（PID 被复用、指向无关进程）的记录仍可清。
    ///
    /// 少了这条，本守卫会退化成「stale 永远清不掉」——而那种记录指向的根本不是我们的
    /// 进程，留住它只会让 `down` 一直报 `identity_mismatch`。
    #[test]
    fn prune_still_removes_identity_mismatch() {
        let tmp = tempdir("prune_mismatch");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");

        // 指向自身（本测试二进制，非 serve）：身份校验必然不符
        let mut rec = InstanceRecord::new("reused");
        rec.state = InstanceState::Ready;
        rec.pid = Some(std::process::id());
        rec.boot_id = identity::boot_id();
        rec.pid_start_ticks = identity::pid_start_ticks(std::process::id());
        rec.workspace = Some("/nope".to_string());
        rec.port = Some(1);
        let path = write_record(&ctx, &rec);

        let items = query::prune(&ctx, false, false).expect("prune 应成功");
        assert_eq!(items.len(), 1);
        assert!(
            items[0].refused.is_none(),
            "身份不符的记录不该被拦：{items:?}"
        );
        assert!(!path.exists(), "身份不符的记录应被清掉");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `up` 不得覆盖活进程的记录：覆盖之后旧进程继续占着端口与实例目录，用户只会看到
    /// 新实例 `failed`，而真正在跑的那个从此无人可管。
    #[test]
    fn up_refuses_to_overwrite_record_of_live_process() {
        let tmp = tempdir("up_live");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");
        let ws = tmp.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let (_live, rec) = live_lookalike(&tmp);
        let path = write_record(&ctx, &rec);
        let before = std::fs::read(&path).expect("读记录");

        let args = UpArgs {
            name: "live".to_string(),
            workspace: Some(ws.display().to_string()),
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
        let err = ops::up(&ctx, &args).expect_err("活进程的记录不能被覆盖");
        assert_eq!(err.error_code, "instance_already_live");
        assert_eq!(
            err.exit_code,
            crate::EXIT_USAGE,
            "与 `port_in_use` 同类：重试 `up` 没有意义，脚本据此判断「别重试」"
        );
        assert!(
            err.message.contains("仍在运行") && err.message.contains("down"),
            "错误信息要既说清现状又给出下一步：{}",
            err.message
        );
        assert_eq!(
            std::fs::read(&path).expect("读记录"),
            before,
            "拒绝时不得改写记录"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 对照组：记录里的进程**确实没了**时必须放行。
    ///
    /// 少了这条，闸门可以退化成「有记录就拒绝启动」——而「上一次实例崩了，再 `up`
    /// 一次」恰恰是最常见的用法，那样就成了回归。
    #[test]
    fn ensure_not_live_passes_when_recorded_process_is_gone() {
        let tmp = tempdir("up_dead");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");
        let mut rec = InstanceRecord::new("dead");
        rec.state = InstanceState::Failed;
        rec.pid = Some(u32::MAX); // 不存在的 pid
        write_record(&ctx, &rec);

        ensure_not_live(&ctx, "dead").expect("进程已不在，应放行");
        // 连记录都没有时同样放行（`up` 的首个实例）
        ensure_not_live(&ctx, "never-existed").expect("无记录应放行");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 拒绝之后的建议按观测态分开：`starting` 是**进行中**，此时让人「先 `down`」
    /// 等于让他把一个正在正常启动的实例白杀一遍。
    #[test]
    fn next_step_advises_waiting_for_starting_instance() {
        let starting = next_step("a", Observed::Starting);
        assert!(
            starting.contains("正在启动") && starting.contains("status a"),
            "starting 应建议先看进展：{starting}"
        );
        assert!(
            !starting.contains("`crabmatectl down a`（serve"),
            "starting 不该把 down 当成首选动作：{starting}"
        );

        let stale = next_step("a", Observed::Stale);
        assert!(
            stale.contains("down") && !stale.contains("status a"),
            "非 starting 应直接指向 down：{stale}"
        );
    }

    /// 顺序是刻意的：**参数**先于**状态**。workspace 不存在时先报 `bad_workspace`，
    /// 而不是让用户先 `down` 掉一个活实例、再发现路径本来就不对。
    #[test]
    fn up_reports_bad_workspace_before_live_record() {
        let tmp = tempdir("precedence");
        let ctx = ctx_at(&tmp);
        ctx.paths.ensure_dirs().expect("建目录");
        let (_live, rec) = live_lookalike(&tmp);
        write_record(&ctx, &rec);

        let args = UpArgs {
            name: "live".to_string(),
            workspace: Some(tmp.join("not-a-dir").display().to_string()),
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
        let err = ops::up(&ctx, &args).expect_err("不存在的 workspace 应被拒绝");
        assert_eq!(
            err.error_code, "bad_workspace",
            "参数错误应排在状态检查之前：{}",
            err.message
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
