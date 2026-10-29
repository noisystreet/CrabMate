//! `crabmate ctl` 真进程验收：起真实 `serve` 子进程，验证多实例、幂等、整组终止与脱管。
//!
//! 两条形态约束：
//!
//! 1. **只能走 CLI 二进制**：`cm_ctl` 全是 `pub(crate)`、`CtlPaths::for_test` 也是
//!    `#[cfg(test)]`，集成测试摸不到内部 API；因此驱动 `crabmate ctl ...`，
//!    并把 `XDG_STATE_HOME` / `XDG_CONFIG_HOME` / `HOME` 指向临时目录做隔离。
//! 2. **`down` 一律 `--timeout 1`**：`serve` 收到 `SIGTERM` 后约 30s 才退出（根因另行跟踪），
//!    用默认 10s 宽限会让每个用例白等 10 秒。
//!
//! 端口显式指定在 181xx 段，避开端口池 8080-8089 与本机真实服务。
#![cfg(all(unix, feature = "server"))]

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_crabmatectl");

/// 隔离沙箱：状态目录、配置目录、HOME 与 workspace 全部落在 tempdir，`Drop` 时兜底清理。
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("创建 tempdir"),
        }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    /// 跑一条 `crabmate ...` 命令。`CM_*` 全部剔除：本机配置不应影响子实例判定。
    fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env("XDG_STATE_HOME", self.root().join("state"))
            .env("XDG_CONFIG_HOME", self.root().join("config"))
            .env("HOME", self.root().join("home"))
            .env_remove("API_KEY");
        for (k, _) in std::env::vars() {
            if k.starts_with("CM_") {
                cmd.env_remove(&k);
            }
        }
        cmd.output().expect("crabmate 应可执行")
    }

    fn workspace(&self, name: &str) -> PathBuf {
        let p = self.root().join("ws").join(name);
        std::fs::create_dir_all(&p).expect("创建 workspace");
        p
    }

    /// `ctl up <name>`，就绪失败时把实例日志一起带进 panic，避免只看到「退出码 1」。
    fn up(&self, name: &str, ws: &Path, port: u16) {
        let out = self.run(&[
            "up",
            name,
            "--workspace",
            &ws.display().to_string(),
            "--port",
            &port.to_string(),
            "--timeout",
            "15",
        ]);
        assert!(
            out.status.success(),
            "ctl up {name} 失败：\nstdout:\n{}\nstderr:\n{}\n日志:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            self.log(name)
        );
    }

    /// `$XDG_STATE_HOME/crabmate/instances/`（`CtlPaths::resolve` 会在 XDG 根下再拼 `crabmate`）。
    fn instance_dir(&self) -> PathBuf {
        self.root().join("state").join("crabmate").join("instances")
    }

    fn record(&self, name: &str) -> serde_json::Value {
        let path = self.instance_dir().join(format!("{name}.json"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读取状态文件 {} 失败：{e}", path.display()));
        serde_json::from_str(&text).expect("状态文件应是合法 JSON")
    }

    fn log(&self, name: &str) -> String {
        let path = self.instance_dir().join(format!("{name}.log"));
        std::fs::read_to_string(path).unwrap_or_else(|_| "(无日志)".to_string())
    }

    fn down_all(&self) {
        let _ = self.run(&["down", "--all", "--timeout", "1"]);
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // 用例 panic 时也不能把 serve 留在机器上：端口占用会污染后续用例
        self.down_all();
    }
}

fn tcp_ok(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// 轮询等待条件成立。
///
/// `down --timeout 1` 是「SIGTERM → 1s 宽限 → SIGKILL」，但 SIGKILL 之后进程被回收、
/// 监听套接字被内核关闭仍有可观延迟；双核 CI runner 上足以让「立即断言」偶发翻车。
/// 这里等待的是「最终状态」，不降低断言语义，只给它一点时间。
fn wait_until(what: &str, timeout: Duration, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cond() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("等待超时（{}s）：{what}", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `up` 之后 TCP 可达，且 `ls --json` 与实际监听端口一致。
#[test]
fn up_reaches_ready_and_ls_reports_matching_base_url() {
    let sb = Sandbox::new();
    let ws = sb.workspace("proj-a");
    let port = 18131u16;
    sb.up("a", &ws, port);

    let rec = sb.record("a");
    assert_eq!(rec["state"], "ready", "记录应落到 ready：{}", sb.log("a"));
    assert_eq!(rec["port"], port);
    assert_eq!(rec["base_url"], format!("http://127.0.0.1:{port}/"));
    assert!(tcp_ok(port), "up 后端口 {port} 应可连");

    let out = sb.run(&["ls", "--json"]);
    assert!(out.status.success(), "ls --json 失败");
    let rows: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("ls --json 输出应是合法 JSON");
    let first = &rows[0];
    assert_eq!(first["name"], "a");
    assert_eq!(first["state"], "ready");
    assert_eq!(first["port"], port);
    assert_eq!(first["base_url"], format!("http://127.0.0.1:{port}/"));
}

/// 两个实例能**同时** ready —— 多实例功能的存在性前提（`serve` 的会话库与锁按 cwd 解析，
/// 没有每实例独立 cwd 时第二个进程会直接启动失败）。
#[test]
fn two_instances_are_ready_at_the_same_time() {
    let sb = Sandbox::new();
    let ws_a = sb.workspace("a");
    let ws_b = sb.workspace("b");
    let (pa, pb) = (18132u16, 18133u16);
    sb.up("a", &ws_a, pa);
    sb.up("b", &ws_b, pb);

    assert_eq!(sb.record("a")["state"], "ready", "{}", sb.log("a"));
    assert_eq!(sb.record("b")["state"], "ready", "{}", sb.log("b"));
    assert!(tcp_ok(pa) && tcp_ok(pb), "两个实例都应可连");

    let rec_a = sb.record("a");
    let rec_b = sb.record("b");
    assert_ne!(rec_a["pid"], rec_b["pid"], "两个实例必须是两个进程");
}

/// 同名重复 `up` 幂等：返回既有实例，不产生第二个进程。
#[test]
fn duplicate_up_is_idempotent() {
    let sb = Sandbox::new();
    let ws = sb.workspace("a");
    let port = 18134u16;
    sb.up("a", &ws, port);
    let pid_first = sb.record("a")["pid"].clone();

    sb.up("a", &ws, port);
    assert_eq!(sb.record("a")["pid"], pid_first, "重复 up 不应换进程");

    let out = sb.run(&[
        "up",
        "a",
        "--workspace",
        &ws.display().to_string(),
        "--port",
        &port.to_string(),
    ]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("已在运行"),
        "重复 up 应提示已在运行，实际：{text}"
    );
}

/// ctl 是短命命令：它退出后 serve 仍在跑（脱管）；`down --all` 之后两者都停。
#[test]
fn down_all_stops_instances_and_ctl_leaves_serve_running() {
    let sb = Sandbox::new();
    let ws_a = sb.workspace("a");
    let ws_b = sb.workspace("b");
    let (pa, pb) = (18135u16, 18136u16);
    sb.up("a", &ws_a, pa);
    sb.up("b", &ws_b, pb);

    // 此时两次 `ctl up` 进程都已退出，端口仍可连 ⇒ 子 serve 未被 ctl 带走
    assert!(tcp_ok(pa) && tcp_ok(pb), "ctl 退出后 serve 应继续运行");

    let out = sb.run(&["down", "--all", "--timeout", "1"]);
    assert!(
        out.status.success(),
        "down --all 失败：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // 强杀之后进程回收与端口关闭都不是瞬时的，轮询等待「最终状态」。
    wait_until(
        "down 后两个端口都应关闭",
        Duration::from_secs(10),
        || !tcp_ok(pa) && !tcp_ok(pb),
    );
    wait_until(
        "状态应转为 stopped 且 pid 清空",
        Duration::from_secs(10),
        || {
            sb.record("a")["state"] == "stopped"
                && sb.record("b")["state"] == "stopped"
                && sb.record("a")["pid"].is_null()
        },
    );
}
