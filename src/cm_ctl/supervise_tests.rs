//! `cm_ctl::supervise`（声明式收敛）的单元测试。
//!
//! 单独成文件：`supervise.rs` 逼近 920 行上限（`scripts/fn_nloc_rust_metrics.py`），
//! 拆测试是保持该上限的手段；测试路径仍是 `cm_ctl::supervise::tests`。

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

use crate::test_fixtures::tempdir;

fn decl(workspace: &str, enabled: bool, port: Option<u16>) -> Declaration {
    Declaration {
        workspace: Some(workspace.to_string()),
        config: None,
        profile: None,
        port,
        enabled,
        env: BTreeMap::new(),
        env_file: None,
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

/// #1187：`disable` 的语义是「这条声明别再跑了」，**不是**「杀掉同名进程」。
/// 手工 `crabmatectl start` 起的实例不归声明管，不得代为停止。
#[test]
fn disabled_decl_does_not_stop_cli_origin_instance() {
    let tmp = tempdir("disabled_cli");
    let ctx = ctx_at(&tmp);
    write_live_record(&ctx, "a", Origin::Cli, DesiredState::Running);
    let file = file_with(&[("a", decl("/tmp/a", false, None))]);
    let steps = plan(&ctx, &file).expect("计划");
    match &steps[0] {
        Step::Skipped { reason, .. } => assert!(reason.contains("cli"), "{reason}"),
        other => panic!("期望 Skipped（cli 起的实例不该被软删除杀掉），实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 另一半：`supervise` 自己拉起的实例，`enabled = false` 仍然要停。
#[test]
fn disabled_decl_stops_supervise_origin_instance() {
    let tmp = tempdir("disabled_sup");
    let ctx = ctx_at(&tmp);
    write_live_record(&ctx, "a", Origin::Supervise, DesiredState::Running);
    let file = file_with(&[("a", decl("/tmp/a", false, None))]);
    let steps = plan(&ctx, &file).expect("计划");
    assert!(
        matches!(steps[0], Step::Stop { .. }),
        "期望 Stop，实际 {:?}",
        steps[0]
    );
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
        restart: restart::Restart::Never,
        backoff: restart::Backoff::default(),
        health: Duration::from_secs(30),
        scope: None,
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

fn write_decl(path: &std::path::Path, body: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).expect("写声明");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("设权限");
}

const DECL_BODY: &str = "[instances.a]\nworkspace = \"/tmp/a\"\n";

#[test]
fn watch_first_round_reconciles_then_goes_quiet() {
    let dir = tempdir("watch_first");
    let path = dir.join("instances.toml");
    write_decl(&path, DECL_BODY, 0o600);

    let (first, fp) = poll_once(&path, None);
    assert!(matches!(first, Poll::Reconcile(_)), "{first:?}");

    // 指纹未变 ⇒ 不再收敛（否则每秒重新 probe 一遍 TCP）
    let (second, _) = poll_once(&path, Some(&fp));
    assert!(matches!(second, Poll::Unchanged), "{second:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_picks_up_content_change() {
    let dir = tempdir("watch_change");
    let path = dir.join("instances.toml");
    write_decl(&path, DECL_BODY, 0o600);
    let (_, fp) = poll_once(&path, None);

    write_decl(
        &path,
        "[instances.a]\nworkspace = \"/tmp/a\"\n\n[instances.b]\nworkspace = \"/tmp/b\"\n",
        0o600,
    );
    let (poll, _) = poll_once(&path, Some(&fp));
    match poll {
        Poll::Reconcile(file) => assert_eq!(file.instances.len(), 2),
        other => panic!("期望 Reconcile，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_pauses_when_permissions_widen_and_recovers_after_chmod() {
    let dir = tempdir("watch_perm");
    let path = dir.join("instances.toml");
    write_decl(&path, DECL_BODY, 0o600);
    let (_, fp) = poll_once(&path, None);

    // 仅放宽权限、内容一字未改：必须被察觉（指纹含 mode）并暂停收敛
    std::fs::set_permissions(&path, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o644)
    })
    .expect("放宽权限");
    let (poll, fp2) = poll_once(&path, Some(&fp));
    match poll {
        Poll::Refused(reason) => {
            assert!(reason.contains("instances_file_insecure"), "{reason}")
        }
        other => panic!("期望 Refused，实际 {other:?}"),
    }

    // chmod 回 0600、内容仍不变：也要重新加载，否则会永远卡在「已暂停」
    std::fs::set_permissions(&path, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o600)
    })
    .expect("恢复权限");
    let (poll, _) = poll_once(&path, Some(&fp2));
    assert!(matches!(poll, Poll::Reconcile(_)), "{poll:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_refuses_missing_declaration_without_crashing() {
    let dir = tempdir("watch_missing");
    let path = dir.join("instances.toml");
    let (poll, fp) = poll_once(&path, None);
    match poll {
        Poll::Refused(reason) => assert!(reason.contains("instances_file_missing"), "{reason}"),
        other => panic!("期望 Refused，实际 {other:?}"),
    }
    // 同一缺失状态不重复告警（指纹已记住）
    let (again, _) = poll_once(&path, Some(&fp));
    assert!(matches!(again, Poll::Unchanged), "{again:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 写一份 `0600` 的 profiles.toml 到本 ctx 的配置目录。
fn write_profiles(ctx: &Ctx, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = ctx.paths.profiles_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("建配置目录");
    }
    std::fs::write(&path, body).expect("写 profiles");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("设权限");
}

/// 写一条「指向自身 pid、profile = 旧值」的 supervise 记录（进程活着）。
fn write_profile_record(ctx: &Ctx, name: &str, profile: &str) {
    ctx.paths.ensure_dirs().expect("建目录");
    let mut rec = InstanceRecord::new(name);
    rec.state = InstanceState::Ready;
    rec.pid = Some(std::process::id());
    rec.pgid = Some(std::process::id() as i32);
    rec.port = Some(1);
    rec.boot_id = crate::cm_ctl::identity::boot_id();
    rec.pid_start_ticks = crate::cm_ctl::identity::pid_start_ticks(std::process::id());
    rec.managed_by = Some(Origin::Supervise.as_str().to_string());
    rec.profile = Some(profile.to_string());
    rec.profile_in_use = Some(profile.to_string());
    let path = ctx.paths.record_path(name).expect("记录路径");
    record::write_atomic(&path, &rec).expect("写记录");
}

#[test]
fn profile_change_on_running_instance_plans_restart() {
    let tmp = tempdir("profile_restart");
    let ctx = ctx_at(&tmp);
    write_profiles(&ctx, "[profiles.new]\npermission_level = \"read\"\n");
    write_profile_record(&ctx, "a", "old");

    let mut d = decl("/tmp/a", true, None);
    d.profile = Some("new".to_string());
    let file = file_with(&[("a", d)]);

    let steps = plan(&ctx, &file).expect("计划");
    match &steps[0] {
        Step::Start {
            restart,
            reason,
            spec,
            ..
        } => {
            assert!(*restart, "profile 变更必须重启才生效");
            assert!(reason.contains("profile"), "{reason}");
            assert_eq!(spec.profile.as_deref(), Some("new"));
        }
        other => panic!("期望 Start（重启），实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn profile_no_tools_and_allowed_commands_reach_start_spec() {
    let tmp = tempdir("profile_launch");
    let ctx = ctx_at(&tmp);
    write_profiles(
        &ctx,
        "[profiles.audit]\nno_tools = true\nallowed_commands = [\"grep\"]\n",
    );

    let mut d = decl("/tmp/a", true, None);
    d.profile = Some("audit".to_string());
    let file = file_with(&[("a", d)]);

    let steps = plan(&ctx, &file).expect("计划");
    match &steps[0] {
        Step::Start { spec, .. } => {
            assert!(spec.no_tools, "no_tools 应传到 --no-tools");
            assert_eq!(
                spec.env.get("CM_ALLOWED_COMMANDS").map(String::as_str),
                Some("grep")
            );
        }
        other => panic!("期望 Start，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn profile_change_reason_compares_name_then_fingerprint() {
    let mut rec = InstanceRecord::new("a");
    let mut d = decl("/tmp/a", true, None);

    // 进程已停 ⇒ 不构成「需重启」
    d.profile = Some("new".to_string());
    rec.profile = Some("old".to_string());
    assert!(profile_change_reason(&d, &rec, false, Some("f1")).is_none());

    // 名字变了 ⇒ 报名字
    let reason = profile_change_reason(&d, &rec, true, Some("f1")).expect("应重启");
    assert!(reason.contains("变为"), "{reason}");

    // 名字相同、指纹不同 ⇒ 内容变了，仍要重启
    rec.profile = Some("new".to_string());
    rec.profile_fingerprint = Some("old-fp".to_string());
    let reason = profile_change_reason(&d, &rec, true, Some("new-fp")).expect("内容变更应重启");
    assert!(reason.contains("内容已变更"), "{reason}");

    // 名字与指纹都一致 ⇒ 无动作（否则每次收敛都会白白重启）
    rec.profile_fingerprint = Some("new-fp".to_string());
    assert!(profile_change_reason(&d, &rec, true, Some("new-fp")).is_none());

    // 双方都没有 profile ⇒ 无动作
    let plain = decl("/tmp/a", true, None);
    let mut bare = InstanceRecord::new("a");
    bare.profile = None;
    assert!(profile_change_reason(&plain, &bare, true, None).is_none());
}

#[test]
fn missing_profile_is_reported_at_plan_time() {
    let tmp = tempdir("profile_missing");
    let ctx = ctx_at(&tmp);
    let mut d = decl("/tmp/a", true, None);
    d.profile = Some("nope".to_string());
    let file = file_with(&[("a", d)]);

    // 计划阶段就报错：不能等到 spawn 才发现引用错了
    let err = plan(&ctx, &file).expect_err("引用不存在的 profile 应报错");
    assert_eq!(err.error_code, "profile_not_found");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `env_file` 权限过宽必须在**计划阶段**报错：这是安全边界，也是「坏文件只影响
/// 这一条 lane」的那条取舍的落点（别的事儿都没发生，实例也没被停）。
#[test]
fn bad_env_file_is_reported_at_plan_time() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir("env_file_bad");
    let ctx = ctx_at(&tmp);
    let creds = tmp.join("creds.env");
    std::fs::write(&creds, "API_KEY=sk-fake\n").expect("写 env_file");
    std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o644)).expect("权限");

    let mut d = decl("/tmp/a", true, None);
    d.env_file = Some(creds.display().to_string());
    let file = file_with(&[("a", d)]);

    let err = plan(&ctx, &file).expect_err("权限过宽应被拒绝");
    assert_eq!(err.error_code, "env_file_insecure");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 正常 `env_file` 的值进**独立通道** `env_file_env`，不混进声明 `env`
/// （后者对 `API_KEY` 是硬拒绝的，混进去只会自相矛盾）。
#[test]
fn good_env_file_reaches_start_spec_credentials() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir("env_file_ok");
    let ctx = ctx_at(&tmp);
    let creds = tmp.join("creds.env");
    std::fs::write(&creds, "API_KEY=sk-fake-from-file\n").expect("写 env_file");
    std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o600)).expect("权限");

    let mut d = decl("/tmp/a", true, None);
    d.env_file = Some(creds.display().to_string());
    let file = file_with(&[("a", d)]);

    let steps = plan(&ctx, &file).expect("计划");
    match &steps[0] {
        Step::Start { spec, .. } => {
            assert_eq!(
                spec.env_file_env.get("API_KEY").map(String::as_str),
                Some("sk-fake-from-file"),
                "凭据应进独立通道"
            );
            assert!(
                !spec.env.contains_key("API_KEY"),
                "凭据不得混进声明 env：{spec:?}"
            );
        }
        other => panic!("期望 Start，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn declared_env_addition_plans_restart() {
    // 这条正是 #1355 之外实测撞到的坑：只改了 `[instances.x.env]`，实例 Ready，
    // 收敛一直「已就绪（无动作）」，声明与运行时静默不一致。
    let mut d = decl("/tmp/a", true, Some(8080));
    d.env
        .insert("CM_ALLOWED_COMMANDS".to_string(), "*".to_string());
    let mut rec = InstanceRecord::new("a");
    rec.env_keys = vec![
        "HOME".to_string(),
        "PATH".to_string(),
        "XDG_RUNTIME_DIR".to_string(),
    ];

    let reason = env_change_reason(&d, &rec, None).expect("声明新增 env 键应要求重启");
    assert!(
        reason.contains("CM_ALLOWED_COMMANDS"),
        "原因里要点名缺的键，否则用户不知道为什么被重启：{reason}"
    );
    assert!(
        !reason.contains("*"),
        "原因里不该回显 env 的**值**（这里是通配符也不回显，免得养成习惯）：{reason}"
    );
}

#[test]
fn declared_env_already_present_needs_no_restart() {
    let mut d = decl("/tmp/a", true, Some(8080));
    d.env.insert("RUST_LOG".to_string(), "debug".to_string());
    let mut rec = InstanceRecord::new("a");
    rec.env_keys = vec!["HOME".to_string(), "RUST_LOG".to_string()];

    assert!(
        env_change_reason(&d, &rec, None).is_none(),
        "键已在记录里就不该反复重启，否则每轮收敛都会抖一次"
    );
}

#[test]
fn changed_env_value_is_not_detected_by_design() {
    // 边界钉子：状态记录只存 `env_keys`（名），值永不落盘（#1355 A 项）。
    // 没有值就没有比较对象，所以「改值」检不出来——显式 `crabmatectl restart` 才生效。
    // 哪天有人想在这里「顺手比对值」，先改那条安全边界并同步文档。
    let mut d = decl("/tmp/a", true, Some(8080));
    d.env.insert("RUST_LOG".to_string(), "trace".to_string());
    let mut rec = InstanceRecord::new("a");
    rec.env_keys = vec!["RUST_LOG".to_string()];

    assert!(
        env_change_reason(&d, &rec, None).is_none(),
        "只改值不该被判成需要重启（判据里没有值）"
    );
}
