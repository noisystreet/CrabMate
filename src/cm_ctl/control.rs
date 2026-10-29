//! `supervise --socket`：本地 unix socket 控制面（`add` / `remove` / `list`）。
//!
//! 存在的理由很具体：TUI 要在界面里增删 lane（= 增删一个 serve 进程），但它被
//! 禁止 spawn 进程，也不该自己写 `instances.toml` —— 那会和 `--watch` 构成两个
//! 写入者争同一个文件。于是**客户端只表达「要什么」，ctl 是声明与进程的唯一写入者**。
//!
//! 边界（见 `docs/命令行与路由.md`）：
//!
//! 1. **不监听 TCP**：socket 只在 `$XDG_RUNTIME_DIR/crabmate/ctl.sock`，网络不可达。
//!    这是本方案与「让 `serve` 暴露 `POST /instances`」在攻击面上的主要差别。
//! 2. **鉴权靠文件权限**：目录 `0700`、socket `0600`，仅同用户可连；不做 token/TLS。
//! 3. **只接受受控字段**：`name` 过 [`paths::validate_name`]，`workspace` 走存在性
//!    校验（越界判定仍属收敛阶段的 `preflight`），`profile` 只接受名字不内联字段。
//! 4. **写入串行**：socket 线程只把请求投进通道，由主循环写盘并收敛，
//!    因此不存在「socket 与 `--watch` 同时改文件」的交错。

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::CtlError;
use super::instances::{self, Declaration};
use super::ops::{self, Ctx};
use super::paths;
use super::probe::Observed;
use super::record::{InstanceRecord, InstanceState};

/// 单行请求上限：请求体是几个短字段，64 KiB 已远超合理值，超出即拒绝。
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

/// 单连接的读写超时。
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// 等主循环处理完请求的上限（启动实例可能慢，故比读超时宽）。
const HANDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 控制面服务端：持有请求通道与 socket 路径。
pub struct Server {
    rx: Receiver<Incoming>,
}

/// 一条待主循环处理的请求。
pub struct Incoming {
    pub request: Request,
    pub reply: SyncSender<Response>,
}

impl Server {
    /// 绑定 socket 并启动接收线程。
    ///
    /// 路径已存在时区分两种情形：**能连上**说明另一个 ctl 正在服务（拒绝，避免两个
    /// 写入者），**连不上**则是上次崩溃的残留（清理后重建）。
    pub fn bind(path: &Path) -> Result<Self, CtlError> {
        clear_stale(path)?;
        let listener = UnixListener::bind(path).map_err(|e| {
            CtlError::general(
                "socket_bind_failed",
                format!("创建控制面 socket 失败 {}：{e}", path.display()),
            )
        })?;
        restrict_to_owner(path)?;
        let (tx, rx) = mpsc::channel();
        spawn_worker(listener, tx);
        Ok(Self { rx })
    }

    /// 非阻塞取一条请求；无请求时返回 `None`（主循环同时要推进文件轮询）。
    pub fn try_next(&self) -> Option<Incoming> {
        self.rx.try_recv().ok()
    }
}

fn clear_stale(path: &Path) -> Result<(), CtlError> {
    if !path.exists() {
        return Ok(());
    }
    if UnixStream::connect(path).is_ok() {
        return Err(CtlError::general(
            "socket_in_use",
            format!(
                "控制面已在使用：{}（已有 supervise 在提供服务，同一 socket 不能有两个写入者）",
                path.display()
            ),
        ));
    }
    std::fs::remove_file(path).map_err(|e| {
        CtlError::general(
            "socket_remove_failed",
            format!("清理陈旧 socket 失败 {}：{e}", path.display()),
        )
    })
}

fn restrict_to_owner(path: &Path) -> Result<(), CtlError> {
    use std::os::unix::fs::PermissionsExt;
    // 0600 是唯一鉴权：放宽等于同机任意用户都能借 ctl 起进程
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        CtlError::general(
            "socket_permission_failed",
            format!("设置 socket 权限 0600 失败 {}：{e}", path.display()),
        )
    })
}

/// 接收线程：一条连接读一行、交主循环、回一行。
///
/// 连接串行处理（不并发 accept）是刻意的：控制面只有短命的单请求连接，
/// 串行换来「一条请求一处写盘」的确定性，也避免连接数被滥用。
fn spawn_worker(listener: UnixListener, tx: Sender<Incoming>) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if let Err(e) = serve_conn(stream, &tx) {
                eprintln!("控制面连接处理失败：{e}");
            }
        }
    });
}

fn serve_conn(stream: UnixStream, tx: &Sender<Incoming>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = String::new();
    BufReader::new(&stream)
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut line)?;
    let resp = dispatch_line(&line, tx);
    let mut out = &stream;
    out.write_all(resp.to_line().as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

fn dispatch_line(line: &str, tx: &Sender<Incoming>) -> Response {
    if line.trim().is_empty() {
        return Response::err("empty_request", "请求为空：每行一个 JSON 对象");
    }
    let request = match Request::parse(line) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    if tx
        .send(Incoming {
            request,
            reply: reply_tx,
        })
        .is_err()
    {
        return Response::err("supervisor_gone", "收敛主循环已退出");
    }
    reply_rx.recv_timeout(HANDLE_TIMEOUT).unwrap_or_else(|_| {
        Response::err(
            "handle_timeout",
            format!("{} 秒内未处理完该请求", HANDLE_TIMEOUT.as_secs()),
        )
    })
}

/// 控制面请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Add(AddRequest),
    Remove(RemoveRequest),
    List(ListRequest),
}

/// `{"op":"list"}`：两个字段都可省略。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    /// 为真时并入「有状态记录但没有声明」的实例（命令行 `up` 起的）。
    ///
    /// 缺省 `false`：`list` 的视野仍是「声明里有什么」，与引入本字段前逐字一致；
    /// 客户端要做全量总览时才显式打开，避免与 `ctl ls` 的语义搅在一起。
    #[serde(default)]
    pub include_unmanaged: bool,
}

/// `{"op":"add",...}`：`workspace` 之外的字段都可省略。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRequest {
    pub name: String,
    pub workspace: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// `{"op":"remove","name":"proj-a"}`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveRequest {
    pub name: String,
}

fn default_enabled() -> bool {
    true
}

impl Request {
    fn parse(line: &str) -> Result<Self, Response> {
        let value: Value = serde_json::from_str(line)
            .map_err(|e| Response::err("bad_request", format!("请求不是合法 JSON：{e}")))?;
        let obj = value
            .as_object()
            .ok_or_else(|| Response::err("bad_request", "请求必须是 JSON 对象"))?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Response::err("bad_request", "缺少字符串字段 `op`"))?;
        // Serde 的 `deny_unknown_fields` 在 internally tagged enum（`tag = "op"`）上
        // **不生效**（serde 文档明确的限制），所以字段白名单必须在这里自己守。
        // 少了这一层，`{"op":"list","evil":1}` 会被静默接受。
        let allowed: &[&str] = match op {
            "add" => &["op", "name", "workspace", "profile", "port", "enabled"],
            "remove" => &["op", "name"],
            "list" => &["op", "include_unmanaged"],
            other => {
                return Err(Response::err(
                    "bad_request",
                    format!("未知 `op`：{other}（支持 add / remove / list）"),
                ));
            }
        };
        if let Some(extra) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(Response::err(
                "bad_request",
                format!(
                    "`{op}` 不接受字段 `{extra}`（允许：{}）",
                    allowed.join(" / ")
                ),
            ));
        }
        serde_json::from_value(value)
            .map_err(|e| Response::err("bad_request", format!("请求字段不合法：{e}")))
    }
}

/// 一行响应：`{"ok":true,...}` 或 `{"ok":false,"code":"...","error":"..."}`。
#[derive(Debug, Clone)]
pub struct Response(Value);

impl Response {
    pub fn ok(body: Value) -> Self {
        let mut obj = serde_json::Map::new();
        obj.insert("ok".to_string(), Value::Bool(true));
        if let Value::Object(extra) = body {
            obj.extend(extra);
        }
        Self(Value::Object(obj))
    }

    /// 错误的 `code` 稳定、可机读；`error` 只面向人，不承诺措辞。
    pub fn err(code: &str, message: impl Into<String>) -> Self {
        Self(json!({"ok": false, "code": code, "error": message.into()}))
    }

    pub fn to_line(&self) -> String {
        serde_json::to_string(&self.0)
            .unwrap_or_else(|_| r#"{"ok":false,"code":"serialize_failed"}"#.to_string())
    }
}

/// 处理一条请求。返回值第二项表示「声明文件已改，需要立刻收敛」。
pub fn handle(ctx: &Ctx, path: &Path, request: Request) -> (Response, bool) {
    match request {
        Request::List(l) => (list(ctx, path, l.include_unmanaged), false),
        Request::Add(a) => add(path, a),
        Request::Remove(r) => remove(path, r),
    }
}

fn add(path: &Path, a: AddRequest) -> (Response, bool) {
    if let Err(e) = paths::validate_name(&a.name) {
        return (Response::err(e.error_code, e.message), false);
    }
    let decl = Declaration {
        workspace: a.workspace,
        // 控制面不接受 `config`：否则 socket 就成了「按任意路径加载配置」的入口，
        // 与 profile「能力集合有界」的目的相悖。要换配置请改 profile 或无 add。
        config: None,
        profile: a.profile,
        port: a.port,
        enabled: a.enabled,
        env: BTreeMap::new(),
    };
    if let Err(resp) = reject_bad_workspace(&decl) {
        return (resp, false);
    }
    let name = a.name;
    // 幂等：已声明就不动它 —— 覆盖会悄悄丢掉手工在文件里写的 env
    match instances::load_optional(path) {
        Ok(file) if file.instances.contains_key(&name) => {
            return (
                Response::ok(json!({
                    "name": name,
                    "created": false,
                    "note": "已存在，未改动（要换 workspace 请先 remove 再无 add）",
                })),
                false,
            );
        }
        Ok(_) => {}
        Err(e) => return (Response::err(e.error_code, e.message), false),
    }
    match instances::upsert(path, &name, decl) {
        Ok(_) => (Response::ok(json!({"name": name, "created": true})), true),
        Err(e) => (Response::err(e.error_code, e.message), false),
    }
}

/// `workspace` 必须已存在：写进一条必然起不来的声明，比直接拒绝更难排查。
///
/// **越界**（不在子实例 `workspace_allowed_roots` 内）不在此判定 —— 那需要加载
/// 子实例配置，属收敛阶段 `preflight` 的职责，届时实例置 `failed` 并给出原因。
fn reject_bad_workspace(decl: &Declaration) -> Result<(), Response> {
    let p = decl
        .workspace_path()
        .map_err(|e| Response::err(e.error_code, e.message))?;
    if p.is_dir() {
        return Ok(());
    }
    Err(Response::err(
        "bad_workspace",
        format!("workspace 不存在或不是目录：{}", p.display()),
    ))
}

fn remove(path: &Path, r: RemoveRequest) -> (Response, bool) {
    if let Err(e) = paths::validate_name(&r.name) {
        return (Response::err(e.error_code, e.message), false);
    }
    match instances::remove_entry(path, &r.name) {
        Ok(true) => (Response::ok(json!({"name": r.name, "removed": true})), true),
        // 幂等：不存在也算成功，客户端不必先查
        Ok(false) => (
            Response::ok(json!({"name": r.name, "removed": false, "note": "声明中不存在该实例"})),
            false,
        ),
        Err(e) => (Response::err(e.error_code, e.message), false),
    }
}

/// 列出声明的实例及其状态记录。
///
/// 默认只列**声明中**的实例：人工 `up` 起的不在声明里，那是 `ctl ls` 的视野；
/// `include_unmanaged` 打开时才并入它们，并以 `declared` / `managed_by` 区分来源。
/// 缺记录时 `state` 给 `starting` —— 客户端要能立刻画出「已请求、尚未就绪」的 lane。
///
/// `observed` 是**读取时派生**的观测态（`probe::observe`），与落盘 `state` 并列：
/// `stale`（记录说在跑但连不上 / 进程身份不符）刻意不落盘，只看 `state` 会把一个
/// 死实例报成 `ready`，客户端据此连接会打到空端口。
fn list(ctx: &Ctx, path: &Path, include_unmanaged: bool) -> Response {
    let file = match instances::load_optional(path) {
        Ok(f) => f,
        Err(e) => return Response::err(e.error_code, e.message),
    };
    // 一次探测复用整份列表：`observed` 要真实进程事实，不能只看落盘 `state`。
    let probed = match ops::ls(ctx) {
        Ok(v) => v,
        Err(e) => return Response::err(e.error_code, e.message),
    };
    let mut items = Vec::with_capacity(file.instances.len() + probed.len());
    for (name, decl) in &file.instances {
        let pair = probed.iter().find(|(rec, _)| &rec.name == name);
        items.push(list_item(name, Some(decl), pair));
    }
    if include_unmanaged {
        for pair in &probed {
            if !file.instances.contains_key(&pair.0.name) {
                items.push(list_item(&pair.0.name, None, Some(pair)));
            }
        }
    }
    Response::ok(json!({ "instances": items }))
}

/// 单个列表项。`decl` 为 `None` 表示这是「只有记录、没有声明」的实例。
fn list_item(
    name: &str,
    decl: Option<&Declaration>,
    pair: Option<&(InstanceRecord, super::probe::Probe)>,
) -> Value {
    let rec = pair.map(|(r, _)| r);
    let observed = pair.map_or(Observed::Starting.as_str(), |(_, p)| p.observed.as_str());
    json!({
        "name": name,
        // 落盘态：缺记录时视为「已请求、尚未就绪」
        "state": rec.map_or(InstanceState::Starting.as_str(), |r| r.state.as_str()),
        // 派生观测态：`stale` / `held` 只可能出现在这里
        "observed": observed,
        "base_url": rec.and_then(|r| r.base_url.clone()),
        "port": rec.and_then(|r| r.port),
        "pid": rec.and_then(|r| r.pid),
        "last_error": rec.and_then(|r| r.last_error.clone()),
        "workspace": decl.map(|d| d.workspace.clone()),
        "profile": decl.and_then(|d| d.profile.clone()),
        "enabled": decl.map(|d| d.enabled),
        // 来源：`declared` 区分「声明里的」与「命令行起的」
        "declared": decl.is_some(),
        "managed_by": rec.and_then(|r| r.managed_by.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_ctl::port_pool::PortPool;
    use std::io::Write as _;
    use std::path::PathBuf;

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: paths::CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cmc{tag}{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    /// 在临时目录里跑一次「主循环 + 一次往返」。
    fn roundtrip(tag: &str, req: &str) -> (Value, PathBuf, PathBuf) {
        let dir = tempdir(tag);
        let sock = dir.join("s.sock");
        let decl = dir.join(instances::FILE_NAME);
        let server = Server::bind(&sock).expect("应能绑定 socket");
        let probe_sock = sock.clone();
        let line = req.to_string();
        let client = std::thread::spawn(move || call(&probe_sock, &line));

        // 请求可能在 `dispatch_line` 阶段就被拒（JSON 非法 / 未知字段），那它根本不会
        // 进通道；没有超时的话，这类用例会永久挂住测试进程。
        let ctx = ctx_at(&dir);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match server.try_next() {
                Some(i) => {
                    let (resp, _changed) = handle(&ctx, &decl, i.request);
                    let _ = i.reply.send(resp);
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let raw = client.join().expect("客户端线程");
        let value: Value = serde_json::from_str(&raw).expect("响应应为 JSON");
        (value, decl, dir)
    }

    fn call(sock: &Path, line: &str) -> String {
        let mut stream = UnixStream::connect(sock).expect("应能连接");
        stream
            .write_all(format!("{line}\n").as_bytes())
            .expect("应能写");
        let mut out = String::new();
        BufReader::new(&stream).read_line(&mut out).expect("应能读");
        out
    }

    #[test]
    fn permissions_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir("perm");
        let sock = dir.join("s.sock");
        let _server = Server::bind(&sock).expect("应能绑定");
        let mode = std::fs::metadata(&sock)
            .expect("应有 socket")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "socket 必须 0600，实际 {mode:o}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_bind_on_live_socket_is_refused() {
        let dir = tempdir("inuse");
        let sock = dir.join("s.sock");
        let _first = Server::bind(&sock).expect("首次应成功");
        let err = match Server::bind(&sock) {
            Ok(_) => panic!("已有服务时应拒绝第二次绑定"),
            Err(e) => e,
        };
        assert_eq!(err.error_code, "socket_in_use");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_is_ok_without_declarations() {
        let (v, decl, dir) = roundtrip("list0", r#"{"op":"list"}"#);
        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["instances"], json!([]));
        assert!(!decl.exists(), "list 不应创建声明文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_op_is_rejected() {
        let (v, _, dir) = roundtrip("badop", r#"{"op":"explode"}"#);
        assert_eq!(v["ok"], json!(false));
        assert_eq!(v["code"], json!("bad_request"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let (v, _, dir) = roundtrip("badfield", r#"{"op":"list","evil":1}"#);
        assert_eq!(v["ok"], json!(false));
        assert_eq!(v["code"], json!("bad_request"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_writes_declaration_and_persists_workspace() {
        let dir = tempdir("add");
        let ws = dir.join("proj-a");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let sock = dir.join("s.sock");
        let decl = dir.join(instances::FILE_NAME);
        let server = Server::bind(&sock).expect("应能绑定");
        let body = json!({"op": "add", "name": "proj-a", "workspace": ws.display().to_string()});
        let probe = sock.clone();
        let client = std::thread::spawn(move || call(&probe, &body.to_string()));

        let incoming = loop {
            if let Some(i) = server.try_next() {
                break i;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let ctx = ctx_at(&dir);
        let (resp, changed) = handle(&ctx, &decl, incoming.request);
        let _ = incoming.reply.send(resp.clone());
        let raw = client.join().expect("客户端线程");
        let v: Value = serde_json::from_str(&raw).expect("响应应为 JSON");

        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["created"], json!(true));
        assert!(changed, "新增声明应触发收敛");
        let loaded = instances::load(&decl).expect("声明应可加载（含 0600 校验）");
        let d = loaded.instances.get("proj-a").expect("应有 proj-a");
        assert_eq!(d.workspace, ws.display().to_string());
        assert!(d.enabled, "enabled 缺省为 true");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_is_idempotent() {
        let dir = tempdir("idem");
        let ws = dir.join("p");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let decl = dir.join(instances::FILE_NAME);
        let req = AddRequest {
            name: "p".to_string(),
            workspace: ws.display().to_string(),
            profile: None,
            port: None,
            enabled: true,
        };
        let (first, changed) = add(&decl, req.clone());
        assert!(changed && first.to_line().contains("\"created\":true"));
        let (second, changed_again) = add(&decl, req);
        assert!(!changed_again, "重复 add 不应再触发收敛");
        let line = second.to_line();
        assert!(line.contains("\"created\":false"), "应返回既有实例：{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_rejects_bad_name_without_writing() {
        let dir = tempdir("badname");
        let decl = dir.join(instances::FILE_NAME);
        for name in ["../etc", "a/b", ""] {
            let req = AddRequest {
                name: name.to_string(),
                workspace: dir.display().to_string(),
                profile: None,
                port: None,
                enabled: true,
            };
            let (resp, changed) = add(&decl, req);
            assert!(!changed, "非法名字不应写盘：{name}");
            assert!(resp.to_line().contains("\"ok\":false"), "应拒绝：{name}");
        }
        assert!(!decl.exists(), "非法请求不得创建声明文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_rejects_missing_workspace_without_writing() {
        let dir = tempdir("badws");
        let decl = dir.join(instances::FILE_NAME);
        let req = AddRequest {
            name: "p".to_string(),
            workspace: dir.join("absent").display().to_string(),
            profile: None,
            port: None,
            enabled: true,
        };
        let (resp, changed) = add(&decl, req);
        assert!(!changed);
        assert!(resp.to_line().contains("bad_workspace"));
        assert!(!decl.exists(), "起点不来的声明不应落盘");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 控制面**不接受** `config`：socket 只表达「用哪个预设」，不表达「加载哪个文件」。
    /// 放开它等于把「按任意路径加载配置」这个越权面重新引入，与 profile 的目的相反。
    #[test]
    fn add_rejects_config_field() {
        let (v, decl, dir) = roundtrip(
            "noconfig",
            r#"{"op":"add","name":"p","workspace":"/tmp","config":"/tmp/x.toml"}"#,
        );
        assert_eq!(v["ok"], json!(false));
        assert_eq!(v["code"], json!("bad_request"));
        assert!(!decl.exists(), "含禁用字段的请求不得创建声明文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_is_idempotent_and_clears_declaration() {
        let dir = tempdir("rm");
        let ws = dir.join("p");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let decl = dir.join(instances::FILE_NAME);
        let (_, changed) = add(
            &decl,
            AddRequest {
                name: "p".to_string(),
                workspace: ws.display().to_string(),
                profile: None,
                port: None,
                enabled: true,
            },
        );
        assert!(changed);
        let (resp, changed) = remove(
            &decl,
            RemoveRequest {
                name: "p".to_string(),
            },
        );
        assert!(changed);
        assert!(resp.to_line().contains("\"removed\":true"));
        let loaded = instances::load(&decl).expect("声明应仍可加载");
        assert!(loaded.instances.is_empty(), "声明项应被摘掉");

        let (resp2, changed2) = remove(
            &decl,
            RemoveRequest {
                name: "p".to_string(),
            },
        );
        assert!(!changed2, "删除不存在实例不应再收敛");
        assert!(resp2.to_line().contains("\"ok\":true"), "应幂等成功");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_reports_declared_instances_as_starting_without_record() {
        let dir = tempdir("list1");
        let ws = dir.join("p");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let decl = dir.join(instances::FILE_NAME);
        add(
            &decl,
            AddRequest {
                name: "p".to_string(),
                workspace: ws.display().to_string(),
                profile: None,
                port: None,
                enabled: true,
            },
        );
        let ctx = ctx_at(&dir);
        let body = list(&ctx, &decl, false).to_line();
        let v: Value = serde_json::from_str(&body).expect("应为 JSON");
        assert_eq!(v["instances"][0]["name"], json!("p"));
        assert_eq!(
            v["instances"][0]["state"],
            json!("starting"),
            "无状态记录时应给 starting，客户端据此画「已请求未就绪」"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_reports_stale_observed_when_ready_record_cannot_be_reached() {
        let dir = tempdir("list2");
        let ws = dir.join("p");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let decl = dir.join(instances::FILE_NAME);
        add(
            &decl,
            AddRequest {
                name: "p".to_string(),
                workspace: ws.display().to_string(),
                profile: None,
                port: None,
                enabled: true,
            },
        );
        let ctx = ctx_at(&dir);
        // 记录说 ready、pid 活着（用自身 pid），但端口没人监听 → 观测态必须暴露 stale。
        // 只回 `state` 的话客户端会拿着这个 base_url 去连一个空端口。
        let mut rec = crate::cm_ctl::record::InstanceRecord::new("p");
        rec.state = crate::cm_ctl::record::InstanceState::Ready;
        rec.pid = Some(std::process::id());
        rec.port = Some(1);
        rec.base_url = Some("http://127.0.0.1:1".to_string());
        let path = ctx.paths.record_path("p").expect("记录路径");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建状态目录");
        crate::cm_ctl::record::write_atomic(&path, &rec).expect("写记录");

        let v: Value = serde_json::from_str(&list(&ctx, &decl, false).to_line()).expect("应为 JSON");
        assert_eq!(v["instances"][0]["state"], json!("ready"), "落盘态保持 ready");
        assert_eq!(
            v["instances"][0]["observed"],
            json!("stale"),
            "连不上的 ready 记录必须报 stale，不能只回 state"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_include_unmanaged_lists_record_without_declaration() {
        let dir = tempdir("list3");
        let decl = dir.join(instances::FILE_NAME);
        let ctx = ctx_at(&dir);
        // 命令行 `up` 起的实例：有状态记录、没有声明
        let mut rec = crate::cm_ctl::record::InstanceRecord::new("cli-one");
        rec.state = crate::cm_ctl::record::InstanceState::Stopped;
        rec.managed_by = Some("cli".to_string());
        let path = ctx.paths.record_path("cli-one").expect("记录路径");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建状态目录");
        crate::cm_ctl::record::write_atomic(&path, &rec).expect("写记录");

        let plain: Value =
            serde_json::from_str(&list(&ctx, &decl, false).to_line()).expect("应为 JSON");
        assert!(
            plain["instances"].as_array().expect("数组").is_empty(),
            "缺省视野是「声明里有什么」，不该冒出非声明实例"
        );

        let all: Value = serde_json::from_str(&list(&ctx, &decl, true).to_line()).expect("应为 JSON");
        let items = all["instances"].as_array().expect("数组");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], json!("cli-one"));
        assert_eq!(items[0]["declared"], json!(false));
        assert_eq!(items[0]["managed_by"], json!("cli"));
        // 记录 stopped 且无 pid → 观测态就是 stopped
        assert_eq!(items[0]["observed"], json!("stopped"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
