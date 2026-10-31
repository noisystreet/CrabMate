//! `cm_ctl::control`（`supervise --socket` 控制面）的单元测试。
//!
//! 单独成文件：`control.rs` 逼近 920 行上限（`scripts/fn_nloc_rust_metrics.py`），
//! 拆测试是保持该上限的手段；测试路径仍是 `cm_ctl::control::tests`。

use super::*;
use crate::cm_ctl::paths;
use crate::cm_ctl::port_pool::PortPool;
use std::collections::BTreeMap;
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
    assert_eq!(d.workspace, Some(ws.display().to_string()));
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

/// 一份带 `env` / `config` 的声明 —— 「手工写在文件里、客户端并不知道」的那部分。
fn decl_with_manual_fields(dir: &Path) -> (PathBuf, PathBuf) {
    let ws = dir.join("p");
    std::fs::create_dir_all(&ws).expect("建 workspace");
    let decl = dir.join(instances::FILE_NAME);
    let mut env = BTreeMap::new();
    env.insert("CM_PERMISSION_LEVEL".to_string(), "read".to_string());
    let mut file = instances::InstancesFile::default();
    file.instances.insert(
        "p".to_string(),
        Declaration {
            workspace: Some(ws.display().to_string()),
            config: Some("/etc/crabmate/readonly.toml".to_string()),
            profile: None,
            port: None,
            enabled: true,
            env,
        },
    );
    instances::write_atomic(&decl, &file).expect("写声明");
    (decl, ws)
}

/// #1187 的核心回归：`disable` 是**软删除**，不得像 `remove` + `add` 那样把
/// 人工写在文件里的 `env` / `config` 冲掉。
#[test]
fn disable_preserves_env_and_config() {
    let dir = tempdir("dis_keep");
    let (decl, ws) = decl_with_manual_fields(&dir);
    let ctx = ctx_at(&dir);

    let (resp, changed) = handle(
        &ctx,
        &decl,
        Request::Disable(TargetRequest {
            name: "p".to_string(),
        }),
    );
    assert!(changed, "改了声明就必须触发收敛");

    let v: Value = serde_json::from_str(&resp.to_line()).expect("响应应为 JSON");
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["changed"], json!(true));
    assert_eq!(v["enabled"], json!(false));

    let loaded = instances::load(&decl).expect("声明应仍可加载");
    let d = loaded.instances.get("p").expect("声明项必须还在（软删除）");
    assert!(!d.enabled, "enabled 应被改为 false");
    assert_eq!(
        d.config.as_deref(),
        Some("/etc/crabmate/readonly.toml"),
        "config 不得被清空"
    );
    assert_eq!(
        d.env.get("CM_PERMISSION_LEVEL").map(String::as_str),
        Some("read"),
        "env 不得被清空"
    );
    assert_eq!(d.workspace, Some(ws.display().to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 已 disabled 再 disable：成功、`changed:false`、且不触发收敛。
#[test]
fn disable_is_idempotent_and_does_not_reconverge() {
    let dir = tempdir("dis_idem");
    let (decl, _) = decl_with_manual_fields(&dir);
    let ctx = ctx_at(&dir);
    let req = || {
        Request::Disable(TargetRequest {
            name: "p".to_string(),
        })
    };

    let (_, first) = handle(&ctx, &decl, req());
    assert!(first);
    let (second, changed_again) = handle(&ctx, &decl, req());
    assert!(!changed_again, "值未变不应再收敛");
    let v: Value = serde_json::from_str(&second.to_line()).expect("响应应为 JSON");
    assert_eq!(v["ok"], json!(true), "幂等成功");
    assert_eq!(v["changed"], json!(false));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 目标不存在：与 `remove` 同口径 —— 幂等成功，客户端不必先 `list`。
#[test]
fn disable_unknown_name_is_ok() {
    let dir = tempdir("dis_missing");
    let (decl, _) = decl_with_manual_fields(&dir);
    let ctx = ctx_at(&dir);
    let (resp, changed) = handle(
        &ctx,
        &decl,
        Request::Disable(TargetRequest {
            name: "nope".to_string(),
        }),
    );
    assert!(!changed);
    let v: Value = serde_json::from_str(&resp.to_line()).expect("响应应为 JSON");
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["changed"], json!(false));
    let loaded = instances::load(&decl).expect("声明应仍可加载");
    assert!(loaded.instances.contains_key("p"), "不得动其它声明项");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `enable` 必须收回 `down` 留下的 `desired_state = stopped`：
/// 否则收敛会一直 `Skipped`，声明写着 `enabled = true` 实例却永远起不来。
#[test]
fn enable_clears_down_marker() {
    let dir = tempdir("en_clear");
    let (decl, _) = decl_with_manual_fields(&dir);
    let ctx = ctx_at(&dir);
    ctx.paths.ensure_dirs().expect("建状态目录");

    // 先 disable，模拟客户端摘过 lane
    handle(
        &ctx,
        &decl,
        Request::Disable(TargetRequest {
            name: "p".to_string(),
        }),
    );
    // 再模拟 `down` 留下的「用户显式停止」标记
    let rec_path = ctx.paths.record_path("p").expect("记录路径");
    let mut rec = crate::cm_ctl::record::InstanceRecord::new("p");
    rec.desired_state = crate::cm_ctl::record::DesiredState::Stopped;
    crate::cm_ctl::record::write_atomic(&rec_path, &rec).expect("写记录");

    let (resp, changed) = handle(
        &ctx,
        &decl,
        Request::Enable(TargetRequest {
            name: "p".to_string(),
        }),
    );
    assert!(changed, "放开 enabled 应触发收敛");

    let v: Value = serde_json::from_str(&resp.to_line()).expect("响应应为 JSON");
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["enabled"], json!(true));
    assert_eq!(v["cleared_down"], json!(true), "应回报清掉了 down 标记");

    let after = crate::cm_ctl::record::read_optional(&rec_path)
        .expect("读记录")
        .expect("记录应存在");
    assert_eq!(
        after.desired_state,
        crate::cm_ctl::record::DesiredState::Running,
        "desired_state 必须回到 running，否则收敛恒定 Skipped"
    );
    let loaded = instances::load(&decl).expect("声明应仍可加载");
    assert!(loaded.instances["p"].enabled);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 声明本就 `enabled = true`、记录却被 `down` 标成 `stopped` 时，`enable` 必须
/// **立刻收敛**：`changed` 只反映 `enabled` 字段变没变，清标记是另一次写盘；只认
/// `changed` 会回一个 `ok` 却什么都不做（默认 `--restart never`，`--watch` 又只跟
/// 声明指纹，实例于是永远停着 —— 客户端拿到的却是成功）。
#[test]
fn enable_on_already_enabled_but_downed_instance_reconverges() {
    let dir = tempdir("en_idem");
    let (decl, _) = decl_with_manual_fields(&dir);
    let ctx = ctx_at(&dir);
    ctx.paths.ensure_dirs().expect("建状态目录");

    // 只留下 `down` 的标记，`enabled` 仍是 true（客户端重试 enable 的场景）
    let rec_path = ctx.paths.record_path("p").expect("记录路径");
    let mut rec = crate::cm_ctl::record::InstanceRecord::new("p");
    rec.desired_state = crate::cm_ctl::record::DesiredState::Stopped;
    crate::cm_ctl::record::write_atomic(&rec_path, &rec).expect("写记录");

    let (resp, dirty) = handle(
        &ctx,
        &decl,
        Request::Enable(TargetRequest {
            name: "p".to_string(),
        }),
    );
    let v: Value = serde_json::from_str(&resp.to_line()).expect("响应应为 JSON");
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["changed"], json!(false), "enabled 本就是 true");
    assert_eq!(v["cleared_down"], json!(true), "应回报清掉了 down 标记");
    assert!(dirty, "清了标记就必须收敛，否则实例把 ok 当成已启动");

    let after = crate::cm_ctl::record::read_optional(&rec_path)
        .expect("读记录")
        .expect("记录应存在");
    assert_eq!(
        after.desired_state,
        crate::cm_ctl::record::DesiredState::Running,
        "desired_state 必须回到 running"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `disable` / `enable` 只收 `name`：多给的字段一律拒绝（手写白名单是唯一防线）。
#[test]
fn disable_rejects_unknown_field() {
    let (v, decl, dir) = roundtrip(
        "dis_evil",
        r#"{"op":"disable","name":"p","enabled":true}"#,
    );
    assert_eq!(v["ok"], json!(false));
    assert_eq!(v["code"], json!("bad_request"));
    assert!(!decl.exists(), "非法请求不得创建声明文件");
    let _ = std::fs::remove_dir_all(&dir);
}
