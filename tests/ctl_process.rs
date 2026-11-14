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
use std::process::{Command, Output, Stdio};
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
        self.command(args).output().expect("crabmate 应可执行")
    }

    /// 与 `run` 同一套隔离环境，供需要长期持有的子进程（`supervise`）使用。
    fn command(&self, args: &[&str]) -> Command {
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
        cmd
    }

    /// `$XDG_CONFIG_HOME/crabmate`（声明文件与 profile 的落地目录）。
    fn config_dir(&self) -> PathBuf {
        let d = self.root().join("config").join("crabmate");
        std::fs::create_dir_all(&d).expect("建配置目录");
        d
    }

    /// 写 `instances.toml` 并置 `0600`（声明文件等同进程启动清单，权限门是硬要求）。
    fn write_declaration(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.config_dir().join("instances.toml");
        std::fs::write(&path, body).expect("写声明");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("设 0600");
    }

    fn workspace(&self, name: &str) -> PathBuf {
        let p = self.root().join("ws").join(name);
        std::fs::create_dir_all(&p).expect("创建 workspace");
        p
    }

    /// `ctl up <name>`，就绪失败时把实例日志一起带进 panic，避免只看到「退出码 1」。
    fn up(&self, name: &str, ws: &Path, port: u16) {
        let out = self.run(&[
            "start",
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

    /// 读状态文件；尚未创建时给 `None`（`wait_until` 的谓词里不能 panic）。
    fn record_opt(&self, name: &str) -> Option<serde_json::Value> {
        let path = self.instance_dir().join(format!("{name}.json"));
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn log(&self, name: &str) -> String {
        let path = self.instance_dir().join(format!("{name}.log"));
        std::fs::read_to_string(path).unwrap_or_else(|_| "(无日志)".to_string())
    }

    fn down_all(&self) {
        let _ = self.run(&["stop", "--all", "--timeout", "1"]);
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

    let out = sb.run(&["list", "--json"]);
    assert!(out.status.success(), "ls --json 失败");
    let rows: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("ls --json 输出应是合法 JSON");
    let first = &rows[0];
    assert_eq!(first["name"], "a");
    assert_eq!(first["state"], "ready");
    assert_eq!(first["port"], port);
    assert_eq!(first["base_url"], format!("http://127.0.0.1:{port}/"));
    // 字段集是契约（`crabmate-client` 直接消费）：`cm_ctl::tests` 已在单元层钉住
    // `row_json`，这里再钉一次**真实输出**，防止有人给 `ls` 换一条序列化路径。
    assert_eq!(
        sorted_keys(first),
        contract_keys("list_row"),
        "ls --json 的字段集与 fixtures/ctl_contract.json 不一致"
    );
}

/// `supervise --dry-run --json` 的计划是对外契约，且**不含 env 值**。
#[test]
fn supervise_dry_run_json_reports_the_plan() {
    let sb = Sandbox::new();
    let ws = sb.workspace("proj-a");
    sb.write_declaration(&format!(
        "[instances.a]\nworkspace = \"{}\"\nport = 18141\n\n[instances.a.env]\nRUST_LOG = \"debug\"\n",
        ws.display()
    ));

    // `--json` 不配 `--dry-run` 必须被 clap 拦下：一次性收敛的**结果态**不在契约里，
    // 放行只会让脚本以为拿到的是同一份形状。
    let bare = sb.run(&["supervise", "--from-config", "--json"]);
    assert!(!bare.status.success(), "`--json` 未配 `--dry-run` 应报错");
    assert!(
        String::from_utf8_lossy(&bare.stderr).contains("dry-run"),
        "错误信息应指向缺失的 `--dry-run`：{}",
        String::from_utf8_lossy(&bare.stderr)
    );

    let out = sb.run(&["supervise", "--from-config", "--dry-run", "--json"]);
    assert!(
        out.status.success(),
        "supervise --dry-run --json 失败：\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let steps: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("计划应是合法 JSON 数组");
    let first = &steps[0];
    assert_eq!(first["name"], "a");
    assert_eq!(first["action"], "start");
    assert_eq!(first["port"], 18141);
    // 与 `ls --json` 同一理由：字段集是契约。单元层已钉 `step_json`，
    // 这里再钉**真实输出**，防止有人给 `--json` 换一条序列化路径。
    assert_eq!(
        sorted_keys(first),
        contract_keys("supervise_step"),
        "supervise --dry-run --json 的字段集与 fixtures/ctl_contract.json 不一致"
    );
    // 值不进输出（只有键名）：dry-run 的输出经常被贴进 issue 或写进脚本日志。
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("\"debug\""),
        "env 值不得出现在计划输出里：{stdout}"
    );
    assert!(
        stdout.contains("RUST_LOG"),
        "env 的**名**应当可见，否则用户不知道计划里带了哪些变量：{stdout}"
    );
}

/// 读 `fixtures/ctl_contract.json` 里的一组字段（已排序）；与单元测试读同一份 golden。
fn contract_keys(field: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ctl_contract.json");
    let raw = std::fs::read_to_string(&path).expect("golden 应可读");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("golden 应是合法 JSON");
    let mut keys: Vec<String> = v[field]
        .as_array()
        .unwrap_or_else(|| panic!("golden 缺少数组字段 `{field}`"))
        .iter()
        .map(|k| k.as_str().expect("字段名应是字符串").to_string())
        .collect();
    keys.sort();
    keys
}

fn sorted_keys(v: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = v
        .as_object()
        .expect("契约对象应是 JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// 端口被占用时必须**明确报错**，而不是把别人的进程认成自己的（#1177）。
///
/// 这条钉的是错误**行为**，不只是错误码：修好之前 `up b` 会「成功」，写出
/// `state: ready` 且 `base_url` 指向 **a** 的端口 —— 客户端连 b 全落到 a 上，
/// 两个 lane 共用一份会话库，正是 ctl 存在的理由被绕过。
#[test]
fn up_on_taken_port_fails_instead_of_faking_ready() {
    let sb = Sandbox::new();
    let ws_a = sb.workspace("busy-a");
    let ws_b = sb.workspace("busy-b");
    let port = 18138u16;
    sb.up("a", &ws_a, port);

    let out = sb.run(&[
        "start",
        "b",
        "--workspace",
        &ws_b.display().to_string(),
        "--port",
        &port.to_string(),
    ]);
    assert!(!out.status.success(), "端口被占用时 up 应失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("port_in_use"),
        "应报 port_in_use，实际：{stderr}"
    );

    // 关键断言：不得留下记录。留下 `ready` 记录就是原来的 bug。
    assert!(
        sb.record_opt("b").is_none(),
        "失败的 up 不应写出记录，实际：{:?}",
        sb.record_opt("b")
    );
    // a 不受影响，仍可连
    assert!(tcp_ok(port), "a 应仍在监听 {port}");
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
        "start",
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

    let out = sb.run(&["stop", "--all", "--timeout", "1"]);
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

/// `supervise --watch --restart on-failure`：**真进程**崩溃后被按退避拉起来。
///
/// 这条非走真 `serve` 不可：退避重启的全部价值在「进程真的没了，再起一个」。
/// 单测只能用「起不来的实例」验证记账与退避门，证明不了它能把活拉回来。
///
/// 崩溃用 `kill -9` 而不是 `down`：`down` 会写下 `desired_state = stopped`，
/// 那是「用户显式停止」，被守护进程复活才是错的。
#[test]
fn supervise_restarts_crashed_instance() {
    let sb = Sandbox::new();
    let ws = sb.workspace("proj-restart");
    let port = 18137u16;
    let name = "r";
    sb.write_declaration(&format!(
        "[instances.{name}]\nworkspace = \"{}\"\nport = {port}\n",
        ws.display()
    ));

    let mut sup = sb
        .command(&[
            "supervise",
            "--from-config",
            "--watch",
            "--restart",
            "on-failure",
            "--interval",
            "1",
            "--health-interval",
            "1",
            "--backoff-initial",
            "1",
            "--backoff-max",
            "5",
            // readiness 余量要按 CI 的冷启动给：`up` 用默认 10s 才过得了，这条测试
            // 原来给 5s，双核 runner 上 serve 初始化（SQLite、目录、会话库锁）常常
            // 超过它 → 每次尝试都 timeout → 永远不 ready。超时是**每次尝试**的上限，
            // 放宽它不会让「起不来」蒙混过关，只是不再把「慢」当成「坏」。
            "--timeout",
            "30",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 supervise");

    // 1) 首次收敛把实例拉起来
    //
    // 等待上限要覆盖 `--timeout`：readiness 超时后实例落 `failed` 并回收进程（见 #1181），
    // 后续重试交给 `--restart on-failure` 的退避；这里只等首轮结果，不是重试预算。
    wait_until(
        "声明收敛后实例应 ready",
        Duration::from_secs(120),
        || sb.record_opt(name).is_some_and(|r| r["state"] == "ready") && tcp_ok(port),
    );
    let first_pid = sb.record(name)["pid"].clone();

    // 2) kill -9：进程没了，但期望态仍是 running
    let pid = first_pid.as_u64().expect("ready 的实例应有 pid");
    let killed = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("执行 kill");
    assert!(killed.success(), "kill -9 {pid} 失败");

    // 3) 巡检应在退避窗口过后换成新进程
    wait_until(
        "崩溃的实例应被重新拉起（pid 变化且 ready）",
        Duration::from_secs(120),
        || {
            sb.record_opt(name)
                .is_some_and(|r| r["state"] == "ready" && r["pid"] != first_pid)
        },
    );

    // 先收 supervise，再让 `Drop` 的 `down --all` 收 serve：
    // 反过来的话，守护进程可能在清理之后又拉起一个新进程。
    let _ = sup.kill();
    let _ = sup.wait();
}

/// 往控制面 socket 发一行 JSON，读回一行（`supervise --socket` 的线协议）。
fn socket_call(sock: &Path, req: &serde_json::Value) -> serde_json::Value {
    use std::io::{BufRead, BufReader, Write};

    let mut stream = std::os::unix::net::UnixStream::connect(sock)
        .unwrap_or_else(|e| panic!("连不上控制面 socket {}：{e}", sock.display()));
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .expect("设读超时");
    stream
        .write_all(format!("{req}\n").as_bytes())
        .expect("应能写请求");
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .expect("应能读响应");
    serde_json::from_str(&line).unwrap_or_else(|e| panic!("控制面响应应是合法 JSON：{e}／{line}"))
}

/// 控制面的 `status` / `restart`：客户端只经这条 socket 管 lane，所以「拿单实例详情」
/// 与「重启单条 lane」必须真的可用。
///
/// 非走真进程不可：`restart` 的全部价值在「旧进程真的没了、新进程真的起来了」，
/// 单测只够证明它走到了 `ops::restart`（且报错分支）。
#[test]
fn socket_status_and_restart_drive_a_lane() {
    let sb = Sandbox::new();
    let ws = sb.workspace("sock");
    let port = 18139u16;
    let name = "s";
    sb.write_declaration(&format!(
        "[instances.{name}]\nworkspace = \"{}\"\nport = {port}\n",
        ws.display()
    ));
    let sock = sb.root().join("ctl.sock");

    let mut sup = sb
        .command(&[
            "supervise",
            "--from-config",
            "--watch",
            "--socket",
            &sock.display().to_string(),
            "--interval",
            "1",
            "--timeout",
            "30",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 supervise --socket");

    wait_until(
        "首轮收敛后实例应 ready",
        Duration::from_secs(120),
        || sb.record_opt(name).is_some_and(|r| r["state"] == "ready") && tcp_ok(port),
    );
    let first_pid = sb.record(name)["pid"].clone();

    // 1) status：形状必须是 `status --json` + `ok`，不多不少
    let v = socket_call(&sock, &serde_json::json!({"op": "status", "name": name}));
    assert_eq!(v["ok"], serde_json::json!(true), "status 失败：{v}");
    assert_eq!(v["name"], serde_json::json!(name));
    assert_eq!(v["state"], serde_json::json!("ready"), "{v}");
    assert_eq!(
        v["base_url"],
        serde_json::json!(format!("http://127.0.0.1:{port}/"))
    );
    let mut expected = contract_keys("status_row");
    expected.push("ok".to_string());
    expected.sort();
    assert_eq!(
        sorted_keys(&v),
        expected,
        "status op 的字段集必须与 status --json 一致"
    );

    // 2) restart：换进程、仍 ready，并把 CLI 打到 stderr 的提示放进 notes
    let v = socket_call(
        &sock,
        &serde_json::json!({"op": "restart", "name": name, "grace": 1, "timeout": 30}),
    );
    assert_eq!(v["ok"], serde_json::json!(true), "restart 失败：{v}");
    assert_eq!(v["restarted"], serde_json::json!(true));
    assert_eq!(
        v["state"],
        serde_json::json!("ready"),
        "重启后应已就绪：{v}"
    );
    assert!(v["notes"].is_array(), "notes 应恒为数组：{v}");
    assert_ne!(v["pid"], first_pid, "重启必须换进程");
    wait_until("重启后端口应可连", Duration::from_secs(30), || {
        tcp_ok(port)
    });

    let _ = sup.kill();
    let _ = sup.wait();
}

/// 收一条 `READY=1`（中间那些 `STATUS=` 跳过：真实 systemd 也只认 `READY`）。
fn recv_ready(receiver: &std::os::unix::net::UnixDatagram, budget: Duration) -> Option<String> {
    let mut buf = [0u8; 512];
    let deadline = std::time::Instant::now() + budget;
    while std::time::Instant::now() < deadline {
        // 读超时（1s）之后继续等：`serve` 冷启动要几秒，`READY` 之前不会有数据报
        let Ok(got) = receiver.recv(&mut buf) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buf[..got]).into_owned();
        if text.lines().any(|l| l.trim() == "READY=1") {
            return Some(text);
        }
    }
    None
}

/// `Type=notify` 的核心承诺：`READY=1` **不早于**首轮收敛完成。
///
/// 非走真进程不可：要证的是「发出 READY 的那一刻实例已经 TCP 可连」这条**时序**，
/// 而 `Type=simple` 缺的正是它。单测只能证明编码正确、以及「发了没」。
#[test]
fn supervise_notifies_ready_after_first_round() {
    let sb = Sandbox::new();
    let ws = sb.workspace("proj-notify");
    let port = 18151u16;
    let name = "n";
    sb.write_declaration(&format!(
        "[instances.{name}]\nworkspace = \"{}\"\nport = {port}\n",
        ws.display()
    ));

    // 接收端就是 systemd 侧那个 socket 的等价物：一个普通的 Unix datagram
    let sock = sb.root().join("notify.sock");
    let receiver = std::os::unix::net::UnixDatagram::bind(&sock).expect("绑定 notify socket");
    receiver
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("设读超时");

    let mut sup = sb
        .command(&[
            "supervise",
            "--from-config",
            "--watch",
            "--interval",
            "1",
            "--timeout",
            "30",
        ])
        .env("NOTIFY_SOCKET", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 supervise");

    let text = recv_ready(&receiver, Duration::from_secs(120))
        .expect("首轮收敛后应收到 READY=1（stderr 已丢弃）");
    assert!(
        tcp_ok(port),
        "READY=1 到达时实例必须已可连 —— 这正是 `Type=simple` 表达不了的那件事：{text}"
    );
    assert!(
        text.contains("STATUS="),
        "READY 必须带一行 STATUS，否则 `systemctl --user status` 看不到收敛结果：{text}"
    );

    let _ = sup.kill();
    let _ = sup.wait();
}

/// `NOTIFY_SOCKET` 指向没人监听的路径：通知全程失败，但**收敛照常**。
///
/// 这是刻意的边界——notify 是「告诉别人我好了」，不是启动的前提条件。
/// 若把它当硬依赖，一个残留的环境变量就能让整个 supervisor 起不来。
#[test]
fn notify_failure_does_not_block_reconciliation() {
    let sb = Sandbox::new();
    let ws = sb.workspace("proj-notify-fail");
    let port = 18152u16;
    let name = "f";
    sb.write_declaration(&format!(
        "[instances.{name}]\nworkspace = \"{}\"\nport = {port}\n",
        ws.display()
    ));

    let mut sup = sb
        .command(&[
            "supervise",
            "--from-config",
            "--watch",
            "--interval",
            "1",
            "--timeout",
            "30",
        ])
        // 不 bind 这个路径：每次发送都会失败
        .env("NOTIFY_SOCKET", sb.root().join("nobody-listens.sock"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 supervise");

    wait_until(
        "notify 发不出去也要照常收敛到 ready",
        Duration::from_secs(120),
        || sb.record_opt(name).is_some_and(|r| r["state"] == "ready") && tcp_ok(port),
    );

    let _ = sup.kill();
    let _ = sup.wait();
}
