//! 声明文件（`instances.toml`）的**写原语**：`add` / `remove` / `disable` / `enable`。
//!
//! 两个入口共用这一份实现：
//!
//! - socket 控制面（[`super::control`]）：原语 → 适配成既有的 JSON 响应；
//! - CLI 子命令（[`cli_add`] 等）：原语 → [`super::render`] 的人读输出。
//!
//! 为什么必须共用：这些语义**都不是显然的**，各写一遍必然漂移——`add` 幂等且
//! 不覆盖手工写的 `env`、`remove` 对不存在的实例也成功、`enable` 顺手清掉 `down`
//! 留下的标记。任何一条在两个入口上不一致，都会变成「同一条命令两条通道两种行为」。
//!
//! 本模块**只写声明，不碰进程**：`add` 之后实例要等 `up` 或常驻 `supervise` 收敛才
//! 会起来；`disable` 之后进程要等 `supervise --watch` 那一轮才停。一次性命令里顺手
//! 杀进程是这套设计一直刻意避开的事（回收语义只属于收敛）。
//!
//! CLI 面收窄到与 socket **同一组字段**：`name` / `workspace` / `profile` / `port` /
//! `enabled`。`config` 不开放（否则又成了「按任意路径加载配置」的入口，与 profile
//! 的目的正好相反），`env` 也不开放（`add` 幂等、不覆盖已有声明，收进来只会让人
//! 误以为生效）。两者仍只能手写 TOML。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::instances::{self, Declaration};
use super::ops::{self, Ctx};
use super::paths;
use super::CtlError;

/// `add` 的入参：字段面与 socket 的 `AddRequest` 逐字一致。
pub(crate) struct AddSpec {
    pub name: String,
    /// 写进声明的工作区。与 socket 一样**必填**：交互式入口不放行「待绑定」，
    /// 那条路径属于无人值守的声明文件（见 `crabmatectl up <name>` 的 `missing_workspace`）。
    pub workspace: String,
    pub profile: Option<String>,
    pub port: Option<u16>,
    pub enabled: bool,
}

/// `add` 的结果。
#[derive(Debug, Clone)]
pub(crate) struct AddOutcome {
    pub name: String,
    /// `false` = 已在声明中，**未改动**（幂等；覆盖会悄悄丢掉手工写的 `env`）。
    pub created: bool,
}

impl AddOutcome {
    /// 未新增时的说明（socket 要把同一句话放进 `note` 字段）。
    pub(crate) fn note(&self) -> Option<&'static str> {
        (!self.created).then_some("已存在，未改动（要换 workspace 请先 remove 再 add）")
    }
}

/// `remove` 的结果。
#[derive(Debug, Clone)]
pub(crate) struct RemoveOutcome {
    pub name: String,
    /// `false` = 声明里本就没有（幂等，调用方不必先查）。
    pub removed: bool,
}

/// `disable` / `enable` 的结果。
#[derive(Debug, Clone)]
pub(crate) struct ToggleOutcome {
    pub name: String,
    pub enabled: bool,
    /// 声明里没有这个实例：不报错（客户端可无脑重试），但也不能声称改了。
    pub missing: bool,
    /// `enabled` 字段是否真的变了（值本就一致时为 `false`，且**没有写盘**）。
    pub changed: bool,
    /// `enable` 是否顺手收回了 `down` 留下的 `desired_state = stopped`。
    pub cleared_down: bool,
    /// 清标记失败时的告警：声明已经写成 `enabled = true`，但不该让人以为整件事都成了。
    pub warning: Option<String>,
}

impl ToggleOutcome {
    /// 是否需要立刻收敛。
    ///
    /// **不能只看 `changed`**：它只说明 `enabled` 变没变，而清 `down` 标记同样是一次
    /// 写盘。声明本就 `enabled = true`、记录却被 `down` 标成 `stopped` 时（客户端重试
    /// `enable`），只认 `changed` 会回一个「成功却不收敛」，实例于是永远不启动。
    pub(crate) fn needs_reconcile(&self) -> bool {
        self.changed || self.cleared_down
    }
}

/// 新增一条声明；已存在时**原样不动**。
pub(crate) fn add(path: &Path, spec: AddSpec) -> Result<AddOutcome, CtlError> {
    paths::validate_name(&spec.name)?;
    let decl = Declaration {
        workspace: Some(spec.workspace),
        // 控制面与 CLI 都不接受 `config`：否则就成了「按任意路径加载配置」的入口。
        config: None,
        profile: spec.profile,
        port: spec.port,
        enabled: spec.enabled,
        env: BTreeMap::new(),
    };
    reject_bad_workspace(&decl)?;
    if instances::load_optional(path)?.instances.contains_key(&spec.name) {
        return Ok(AddOutcome {
            name: spec.name,
            created: false,
        });
    }
    instances::upsert(path, &spec.name, decl)?;
    Ok(AddOutcome {
        name: spec.name,
        created: true,
    })
}

/// 删掉一条声明（不删实例目录与日志 —— 那是 `gc` 的活）。
pub(crate) fn remove(path: &Path, name: &str) -> Result<RemoveOutcome, CtlError> {
    paths::validate_name(name)?;
    let removed = instances::remove_entry(path, name)?;
    Ok(RemoveOutcome {
        name: name.to_string(),
        removed,
    })
}

/// 只翻声明里的 `enabled`，其余字段（`env` / `config` / `profile` / `port` /
/// `workspace`）原样保留 —— 这是「软删」与 `remove` 的全部差别。
pub(crate) fn set_enabled(
    path: &Path,
    ctx: &Ctx,
    name: &str,
    enabled: bool,
) -> Result<ToggleOutcome, CtlError> {
    paths::validate_name(name)?;
    let Some(changed) = instances::set_enabled(path, name, enabled)? else {
        return Ok(ToggleOutcome {
            name: name.to_string(),
            enabled,
            missing: true,
            changed: false,
            cleared_down: false,
            warning: None,
        });
    };
    let mut cleared_down = false;
    let mut warning = None;
    if enabled {
        match ops::mark_wanted_running(ctx, name) {
            Ok(cleared) => cleared_down = cleared,
            // 声明已写成 `enabled = true`，此时回错误会让人以为没改成；
            // 但「这半边没做成」必须被看见，否则会干等一个不会启动的实例。
            Err(e) => {
                warning = Some(format!(
                    "已写入 enabled = true，但清理 down 标记失败：{}（{}）",
                    e.message, e.error_code
                ))
            }
        }
    }
    Ok(ToggleOutcome {
        name: name.to_string(),
        enabled,
        missing: false,
        changed,
        cleared_down,
        warning,
    })
}

/// `workspace` 必须已存在：写进一条必然起不来的声明，比直接拒绝更难排查。
///
/// **越界**（不在子实例 `workspace_allowed_roots` 内）不在这里判定 —— 那需要加载
/// 子实例配置，属收敛阶段 `preflight` 的职责，届时实例置 `failed` 并给出原因。
fn reject_bad_workspace(decl: &Declaration) -> Result<(), CtlError> {
    let Some(p) = decl.workspace_path()? else {
        return Ok(());
    };
    if p.is_dir() {
        return Ok(());
    }
    Err(CtlError::usage(
        "bad_workspace",
        format!("workspace 不存在或不是目录：{}", p.display()),
    ))
}

/// 声明文件路径：给了 `--instances-file` 就按 **ctl 的 cwd** 绝对化，否则用默认 XDG 路径。
pub(crate) fn target_path(ctx: &Ctx, raw: Option<&str>) -> Result<PathBuf, CtlError> {
    match raw {
        Some(p) => paths::absolutize(p),
        None => Ok(ctx.paths.instances_file()),
    }
}

// ---------------------------------------------------------------------------
// CLI 侧分派
//
// 放在本模块而不是 `run.rs`：`run.rs` 已逼近 920 行的棘轮上限，而这几个函数只做
// 「clap 类型 → 原语 → 输出」，与上面的语义是同一条线的两端。输出仍走
// [`super::render`]，与 `run.rs` 其它分支一致（`ops` 自己不打印）。
// ---------------------------------------------------------------------------

use crate::config::cli::ctl::{AddCmd, RemoveCmd, ToggleCmd};

/// `crabmatectl add`
pub(crate) fn cli_add(ctx: &Ctx, c: &AddCmd) -> Result<(), CtlError> {
    let path = target_path(ctx, c.instances_file.as_deref())?;
    let out = add(
        &path,
        AddSpec {
            name: c.name.clone(),
            workspace: c.workspace.clone(),
            profile: c.profile.clone(),
            port: c.port,
            enabled: c.enabled.unwrap_or(true),
        },
    )?;
    super::render::decl_add(&out, &path);
    Ok(())
}

/// `crabmatectl remove`
pub(crate) fn cli_remove(ctx: &Ctx, c: &RemoveCmd) -> Result<(), CtlError> {
    let path = target_path(ctx, c.instances_file.as_deref())?;
    let out = remove(&path, &c.name)?;
    super::render::decl_remove(&out, &path);
    Ok(())
}

/// `crabmatectl disable` / `crabmatectl enable`（`enabled` 由调用方给，不猜）。
pub(crate) fn cli_toggle(
    ctx: &Ctx,
    c: &ToggleCmd,
    enabled: bool,
) -> Result<(), CtlError> {
    let path = target_path(ctx, c.instances_file.as_deref())?;
    let out = set_enabled(&path, ctx, &c.name, enabled)?;
    super::render::decl_toggle(&out, &path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::cm_ctl::paths::CtlPaths;
    use crate::cm_ctl::port_pool::PortPool;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cm_ctl_decl_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn ctx_at(dir: &Path) -> Ctx {
        Ctx {
            paths: CtlPaths::for_test(dir),
            pool: PortPool::default_pool(),
        }
    }

    fn decl_path(dir: &Path) -> PathBuf {
        dir.join("config").join(instances::FILE_NAME)
    }

    fn spec(name: &str, ws: &Path) -> AddSpec {
        AddSpec {
            name: name.to_string(),
            workspace: ws.display().to_string(),
            profile: None,
            port: None,
            enabled: true,
        }
    }

    #[test]
    fn add_creates_then_is_idempotent() {
        let dir = tempdir("add");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let path = decl_path(&dir);

        let first = add(&path, spec("a", &ws)).expect("首次 add");
        assert!(first.created, "首次应新增");
        let file = instances::load(&path).expect("加载声明");
        assert_eq!(
            file.instances.get("a").expect("应有 a").workspace_path().expect("路径").as_deref(),
            Some(ws.as_path())
        );

        let again = add(&path, spec("a", &dir)).expect("重复 add");
        assert!(!again.created, "重复 add 必须幂等");
        assert!(again.note().is_some(), "幂等时应给出说明");
        // 幂等的实质：workspace 不被第二次调用覆盖
        let file = instances::load(&path).expect("加载声明");
        assert_eq!(
            file.instances.get("a").expect("应有 a").workspace_path().expect("路径").as_deref(),
            Some(ws.as_path()),
            "已存在的声明不得被覆盖"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `add` 幂等的真正理由：文件里手工写的 `env` 一旦被覆盖就永久丢失。
    #[test]
    fn add_does_not_clobber_handwritten_env() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir("env");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let path = decl_path(&dir);
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建配置目录");
        std::fs::write(
            &path,
            format!(
                "[instances.a]\nworkspace = \"{}\"\n\n[instances.a.env]\nGIT_SSH_COMMAND = \"ssh -i /tmp/k\"\n",
                ws.display()
            ),
        )
        .expect("写声明");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("0600");

        add(&path, spec("a", &ws)).expect("add 应成功（幂等）");
        let file = instances::load(&path).expect("加载声明");
        assert_eq!(
            file.instances.get("a").expect("应有 a").env.get("GIT_SSH_COMMAND").map(String::as_str),
            Some("ssh -i /tmp/k"),
            "手工写的 env 必须原地保留"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_rejects_missing_workspace_and_bad_name() {
        let dir = tempdir("add_bad");
        let path = decl_path(&dir);
        let err = add(&path, spec("a", &dir.join("nope"))).expect_err("workspace 不存在应拒绝");
        assert_eq!(err.error_code, "bad_workspace");
        assert!(!path.exists(), "拒绝时不得写出声明文件");

        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let err = add(&path, spec("../evil", &ws)).expect_err("非法实例名应拒绝");
        assert_eq!(err.error_code, "bad_name");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = tempdir("remove");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let path = decl_path(&dir);
        add(&path, spec("a", &ws)).expect("add");

        assert!(remove(&path, "a").expect("删除").removed);
        assert!(!remove(&path, "a").expect("再删").removed, "不存在也算成功");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disable_preserves_other_fields_and_enable_clears_down_marker() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir("toggle");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let ctx = ctx_at(&dir);
        let path = decl_path(&dir);
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建配置目录");
        std::fs::write(
            &path,
            format!(
                "[instances.a]\nworkspace = \"{}\"\nprofile = \"readonly-audit\"\n\n[instances.a.env]\nX = \"1\"\n",
                ws.display()
            ),
        )
        .expect("写声明");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("0600");

        let off = set_enabled(&path, &ctx, "a", false).expect("disable");
        assert!(off.changed && !off.missing);
        assert!(!off.needs_reconcile() || off.changed, "changed 本身就是收敛依据");
        let file = instances::load(&path).expect("加载声明");
        let d = file.instances.get("a").expect("应有 a");
        assert!(!d.enabled);
        assert_eq!(d.profile.as_deref(), Some("readonly-audit"), "profile 必须保住");
        assert_eq!(d.env.get("X").map(String::as_str), Some("1"), "env 必须保住");

        // 值未变时不写盘、不收敛
        let again = set_enabled(&path, &ctx, "a", false).expect("重复 disable");
        assert!(!again.changed && !again.needs_reconcile());

        let on = set_enabled(&path, &ctx, "a", true).expect("enable");
        assert!(on.changed);
        assert!(
            !on.cleared_down,
            "没有状态记录时无可清，`cleared_down` 必须是 false 而不是谎报"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目标不在声明里：不报错，但也不能声称改了（客户端会无脑重试）。
    #[test]
    fn toggle_missing_instance_is_ok_but_reports_missing() {
        let dir = tempdir("toggle_missing");
        let ctx = ctx_at(&dir);
        let out = set_enabled(&decl_path(&dir), &ctx, "ghost", true).expect("应成功");
        assert!(out.missing && !out.changed && !out.needs_reconcile());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CLI 侧：省略 `--enabled` 必须是 `true`，`--instances-file` 要真起作用，
    /// 且 `env` / `config` **不得**被 CLI 写入（那两条只能手写 TOML）。
    #[test]
    fn cli_add_honours_flags_and_never_writes_env_or_config() {
        use crate::config::cli::ctl::AddCmd;

        let dir = tempdir("cli_add");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let ctx = ctx_at(&dir);
        let file = dir.join("elsewhere").join(instances::FILE_NAME);
        std::fs::create_dir_all(file.parent().expect("父目录")).expect("建目录");

        cli_add(
            &ctx,
            &AddCmd {
                name: "a".to_string(),
                workspace: ws.display().to_string(),
                profile: Some("readonly-audit".to_string()),
                port: Some(9123),
                enabled: None,
                instances_file: Some(file.display().to_string()),
            },
        )
        .expect("add 应成功");

        let loaded = instances::load(&file).expect("加载指到的那份声明");
        let d = loaded.instances.get("a").expect("应有 a");
        assert!(d.enabled, "省略 --enabled 必须是 true");
        assert_eq!(d.profile.as_deref(), Some("readonly-audit"));
        assert_eq!(d.port, Some(9123));
        assert!(d.config.is_none() && d.env.is_empty(), "CLI 不得写入 config / env");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `enable` 必须收回 `down` 留下的「用户显式停止」标记 —— 否则声明写着
    /// `enabled = true`，收敛却永远 `Skipped`，实例再也起不来。这条把 CLI 与
    /// socket 的同一条语义钉在一起。
    #[test]
    fn cli_enable_clears_down_marker() {
        use crate::cm_ctl::record::{self, DesiredState, InstanceRecord};
        use crate::config::cli::ctl::ToggleCmd;

        let dir = tempdir("cli_enable");
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).expect("建 workspace");
        let ctx = ctx_at(&dir);
        ctx.paths.ensure_dirs().expect("建目录");
        let path = decl_path(&dir);
        add(&path, spec("a", &ws)).expect("add");
        set_enabled(&path, &ctx, "a", false).expect("先 disable");

        // 模拟 `down` 写下的标记
        let rec_path = ctx.paths.record_path("a").expect("记录路径");
        let mut rec = InstanceRecord::new("a");
        rec.desired_state = DesiredState::Stopped;
        record::write_atomic(&rec_path, &rec).expect("写记录");

        cli_toggle(
            &ctx,
            &ToggleCmd {
                name: "a".to_string(),
                instances_file: Some(path.display().to_string()),
            },
            true,
        )
        .expect("enable 应成功");

        let after = record::read_required(&rec_path).expect("读回记录");
        assert_eq!(
            after.desired_state,
            DesiredState::Running,
            "enable 必须收回 down 标记，否则实例永远起不来"
        );
        let file = instances::load(&path).expect("加载声明");
        assert!(file.instances.get("a").expect("应有 a").enabled);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
