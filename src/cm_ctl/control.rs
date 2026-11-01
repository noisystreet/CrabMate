//! `supervise --socket`：本地 unix socket 控制面（`add` / `remove` / `list` /
//! `disable` / `enable`）。
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

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::CtlError;
use super::decl;
use super::instances::{self, Declaration};
use super::ops::Ctx;
use super::probe::Observed;
use super::query;
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
    /// 保留声明项、但不要它运行（客户端「摘 lane」的软删除，依赖 `enabled = false`）。
    Disable(TargetRequest),
    /// 撤销 [`Request::Disable`]：声明项本就还在，只把 `enabled` 放开。
    Enable(TargetRequest),
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

/// `{"op":"disable","name":"proj-a"}` / `{"op":"enable","name":"proj-a"}`。
///
/// 只有 `name`：`disable` / `enable` **不得**顺手改别的字段。要换 `workspace` /
/// `profile` / `port` 仍走 `remove` + `add` 的显式两步，避免「一次写操作同时改两件
/// 事」——那正是 `add` 幂等语义一直想避免的。`config` 同样不在此放开。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
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
            // `disable` / `enable` 只收 `name`：它们不是 `update`，不接受任何其它字段
            "remove" | "disable" | "enable" => &["op", "name"],
            "list" => &["op", "include_unmanaged"],
            other => {
                return Err(Response::err(
                    "bad_request",
                    format!("未知 `op`：{other}（支持 add / remove / list / disable / enable）"),
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
        Request::Disable(t) => set_decl_enabled(path, ctx, t.name, false),
        Request::Enable(t) => set_decl_enabled(path, ctx, t.name, true),
    }
}

/// 适配 [`decl::add`]：JSON 的 `name` / `created` / `note` 是控制面对外契约。
fn add(path: &Path, a: AddRequest) -> (Response, bool) {
    let spec = decl::AddSpec {
        name: a.name,
        workspace: a.workspace,
        profile: a.profile,
        port: a.port,
        enabled: a.enabled,
    };
    match decl::add(path, spec) {
        Ok(out) => {
            let mut body = json!({"name": out.name, "created": out.created});
            if let Some(note) = out.note() {
                body["note"] = json!(note);
            }
            (Response::ok(body), out.created)
        }
        Err(e) => (Response::err(e.error_code, e.message), false),
    }
}

/// 适配 [`decl::remove`]：`remove` 对不存在的实例也成功（客户端不必先查）。
fn remove(path: &Path, r: RemoveRequest) -> (Response, bool) {
    match decl::remove(path, &r.name) {
        Ok(out) => {
            let mut body = json!({"name": out.name, "removed": out.removed});
            if !out.removed {
                body["note"] = json!("声明中不存在该实例");
            }
            (Response::ok(body), out.removed)
        }
        Err(e) => (Response::err(e.error_code, e.message), false),
    }
}

/// 适配 [`decl::set_enabled`]：`disable` / `enable` 只翻声明里的 `enabled`。
///
/// 三条约定（与 `remove` 对齐，客户端可无脑重试）：**幂等**（值未变时 `changed: false`
/// 且不触发收敛）、**目标不存在也成功**（`note` 说明）、`enable` 顺手收回 `down`
/// 留下的 `desired_state = stopped`（否则收敛会一直 `Skipped`，声明明明 `enabled = true`）。
///
/// 返回值第二项即 [`handle`] 的「需要立刻收敛」，判据是
/// [`decl::ToggleOutcome::needs_reconcile`] 而**不是** `changed`：清 `down` 标记同样
/// 是一次写盘，只认 `changed` 会回一个「成功却不收敛」。
fn set_decl_enabled(path: &Path, ctx: &Ctx, name: String, enabled: bool) -> (Response, bool) {
    match decl::set_enabled(path, ctx, &name, enabled) {
        Ok(out) if out.missing => (
            Response::ok(json!({
                "name": out.name,
                "enabled": out.enabled,
                "changed": false,
                "note": "声明中不存在该实例",
            })),
            false,
        ),
        Ok(out) => {
            let mut body = json!({
                "name": out.name,
                "enabled": out.enabled,
                "changed": out.changed,
                "cleared_down": out.cleared_down,
            });
            if let Some(warn) = out.warning.as_deref() {
                body["warning"] = json!(warn);
            }
            let dirty = out.needs_reconcile();
            (Response::ok(body), dirty)
        }
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
    let probed = match query::ls(ctx) {
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
#[path = "control_tests.rs"]
mod tests;
