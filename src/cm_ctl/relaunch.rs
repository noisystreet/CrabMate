//! 「状态记录 → 启动规格」的纯映射（`restart` 用）。
//!
//! 单独成模块，是为了让这段逻辑能被单测直接钉住：`restart` 本身是 down + up
//! （真的动进程、等 readiness），测试够不到 [`UpArgs`]；而「记录 → 启动规格」正是
//! 「重启后悄悄变成另一个实例」这类 bug 的唯一落点——`--yes` / profile / 持有者
//! 都只在这里搬运，漏一个不会有任何编译期或运行期信号，只会让实例在重启后换了身份。
//!
//! `env` 是同一类缺陷的第二次发作（#1212）：状态记录只存 `env_keys`（名字，无值），
//! 从记录重建必然丢掉声明里的 `[instances.x.env]`。所以**声明优先**——实例在
//! `instances.toml` 里就按声明重建（复用 [`supervise::start_spec`]），记录只作
//! 「未声明实例」的回落；回落时丢掉的变量名必须报出来，否则就是静默改配置。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use super::instances::{self, Declaration};
use super::ops::{Origin, UpArgs};
use super::profiles;
use super::record::InstanceRecord;
use super::spawn;
use super::supervise;
use super::CtlError;

/// 由状态记录重建启动规格（`restart` 的后半段）。
pub(crate) fn args_from_record(
    rec: &InstanceRecord,
    resolved: Option<&profiles::Resolved>,
    timeout: Duration,
) -> UpArgs {
    UpArgs {
        name: rec.name.clone(),
        // 记录里没有工作区 = 该实例本来就是待绑定态，重启后仍是待绑定（不回填，见 #1183 §2 ②）。
        workspace: rec.workspace.clone(),
        serve_config: resolved
            .and_then(|r| r.serve_config.clone())
            .or_else(|| rec.config.clone()),
        port: rec.port,
        // 记录里有才沿用：`restart` 不应替用户新增或撤销 `--yes`（改动它会改变工具面）
        yes: rec.yes,
        env: resolved.map(|r| r.env.clone()).unwrap_or_default(),
        // 就绪等待来自 `--timeout`，与终止宽限 `--grace` 是两个独立旋钮
        timeout,
        // 沿用原持有者：`supervise` 拉起的实例被人工 restart 后仍归 `supervise` 回收，
        // 否则它会变成「无主的 supervise 实例」——既不被声明回收，也不被声明接管。
        origin: Origin::from_record(rec),
        profile: rec.profile.clone(),
        profile_fingerprint: resolved.map(|r| r.fingerprint()),
        no_tools: resolved.map(|r| r.no_tools).unwrap_or(false),
    }
}

/// `restart` 的重建结果：启动规格 + 必须让用户看见的提示。
#[derive(Debug)]
pub(crate) struct RestartPlan {
    pub args: UpArgs,
    /// 进 `UpOutcome::warnings`（由 `render` 打到 stderr）。
    ///
    /// 「哪些 env 不会再注入」只能靠它说清：状态记录只存**变量名**，丢掉的值无法回读，
    /// 不提示就等于静默改配置——这正是 #1212「照文档 `restart` 反而丢 env」的成因。
    pub notes: Vec<String>,
}

/// 由「记录 + 声明 + 已解析 profile」产出重启规格（`restart` 的前半段，纯函数）。
///
/// 两条路径的差别就是 #1212 的实质：
///
/// - **有声明**：按声明重建（profile / config / env / port 以当前 `instances.toml`
///   为准），与 `supervise` 共用 [`supervise::start_spec`]，不抄第二份；
/// - **无声明**：只能回落到记录；记录里只有 `env_keys`（名，无值），这一路**必然**
///   丢 env，所以把丢掉的变量名报出来。
pub(crate) fn plan_restart(
    rec: &InstanceRecord,
    decl: Option<&Declaration>,
    resolved: Option<&profiles::Resolved>,
    timeout: Duration,
) -> Result<RestartPlan, CtlError> {
    let Some(decl) = decl else {
        let args = args_from_record(rec, resolved, timeout);
        let notes = env_loss_notes("该实例不在 instances.toml 里", rec, &args.env)?;
        return Ok(RestartPlan { args, notes });
    };

    // 声明省略 `workspace` = 待绑定（#1183）；但实例运行期可能已被 client
    // `POST /workspace` 绑定过，缺省时沿用记录里的值——「不回填」不等于「把已绑定的
    // 丢掉」，否则重启一次就退回待绑定态。
    let declared_workspace = decl.workspace_path()?.map(|p| p.display().to_string());
    let spec = supervise::start_spec(decl, declared_workspace.as_deref(), rec.port, resolved);
    let args = UpArgs {
        name: rec.name.clone(),
        workspace: declared_workspace.or_else(|| rec.workspace.clone()),
        // `start_spec` 只认 `resolved.serve_config` 与 `decl.config`：声明没写 config
        // 而记录里有（`up --serve-config` 起过）时，这一层回落不能丢。
        serve_config: spec.serve_config.or_else(|| rec.config.clone()),
        port: spec.port,
        // `--yes` 与持有者只有记录知道，且必须是记录那一份（`StartSpec` 没有这两个字段）
        yes: rec.yes,
        env: spec.env,
        timeout,
        origin: Origin::from_record(rec),
        profile: spec.profile,
        profile_fingerprint: spec.profile_fingerprint,
        no_tools: spec.no_tools,
    };
    let mut notes = env_loss_notes("声明里没有这些变量", rec, &args.env)?;
    if !decl.enabled {
        notes.push(format!(
            "声明里 `{}` 是 enabled = false：本次按你的要求拉起，下一次 `supervise` 收敛仍会停掉它",
            rec.name
        ));
    }
    if let Some(note) = profile_drift_note(rec, &args) {
        notes.push(note);
    }
    Ok(RestartPlan { args, notes })
}

/// `restart` 的入口：读声明与 profiles，产出 [`RestartPlan`]。
///
/// 放这里而不是 `ops`：读盘只为构造这一个纯对象，`ops::restart` 剩下的就该是
/// 「算规格 → down → up」，别再多一段「顺便读两个文件」。
pub(crate) fn load_restart_plan(
    rec: &InstanceRecord,
    instances_path: &Path,
    profiles_path: &Path,
    timeout: Duration,
) -> Result<RestartPlan, CtlError> {
    let (decl, decl_note) = declared_entry(instances_path, &rec.name);
    let instance_env = decl.as_ref().map(|d| d.env.clone()).unwrap_or_default();
    // 声明是「这个实例应该是什么」的事实来源：声明里没写 profile 就是没写，只有
    // **未声明**实例才沿用记录里的名字（那时它是唯一线索）。
    let profile = match decl.as_ref() {
        Some(d) => d.profile.clone(),
        None => rec.profile.clone(),
    };
    let resolved = resolve_profile(profiles_path, profile.as_deref(), &instance_env)?;
    let mut plan = plan_restart(rec, decl.as_ref(), resolved.as_ref(), timeout)?;
    if let Some(note) = decl_note {
        plan.notes.insert(0, note);
    }
    Ok(plan)
}

/// 从声明文件取同名条目；文件本身不可用时**不阻断**重启。
///
/// 权限过宽（`instances_file_insecure`）或 TOML 写错都会让 `load` 失败，而 `restart`
/// 恰恰是此时手上唯一的恢复手段——所以降级到状态记录，并带一条说明：声明里的
/// env / profile 这次不会用上，修好文件后 `restart` 才会按声明重建。
fn declared_entry(path: &Path, name: &str) -> (Option<Declaration>, Option<String>) {
    match instances::load_optional(path) {
        Ok(file) => (file.instances.get(name).cloned(), None),
        Err(e) => (
            None,
            Some(format!(
                "声明文件 {} 不可用（{}）：本次按状态记录重建，声明里的 env / profile 不会生效",
                path.display(),
                e.error_code
            )),
        ),
    }
}

/// 按 profile 名重新解析预设。
///
/// 这段语义值得一个函数名而不只是内联：**重启必须按当前 `profiles.toml` 重建权限
/// 集合**，否则 `--no-tools` / `allowed_commands` 会在重启后消失——而那是向**放松**
/// 方向漂移，比报错更危险。
///
/// `instance_env` 必须是**声明里的** env：它与 `profile.env` 同名时按合并顺序
/// （派生键 → `profile.env` → `instance.env`）覆盖。传空表等于把
/// `[instances.x.env]` 从 `Resolved` 里抹掉——这正是 #1212 的一半。
fn resolve_profile(
    path: &Path,
    profile: Option<&str>,
    instance_env: &BTreeMap<String, String>,
) -> Result<Option<profiles::Resolved>, CtlError> {
    let Some(name) = profile else {
        return Ok(None);
    };
    let file = profiles::load_optional(path)?;
    Ok(Some(profiles::resolve(&file, name, instance_env, path)?))
}

/// 记录里出现过、这次却不会再注入的变量名。
///
/// `build_env` 在**这里**调用，而不是等 spawn 时报错：它同时是「敏感键一律拒绝」的
/// 闸门，放在 `down` 之前才能避免「实例已经停了，才发现声明里写了 `API_KEY`」。
fn env_loss_notes(
    reason: &str,
    rec: &InstanceRecord,
    declared: &BTreeMap<String, String>,
) -> Result<Vec<String>, CtlError> {
    let (_, keys) = spawn::build_env(declared)?;
    let kept: Vec<&str> = keys.iter().map(String::as_str).collect();
    let lost: Vec<&str> = rec
        .env_keys
        .iter()
        .map(String::as_str)
        .filter(|name| !kept.contains(name))
        .collect();
    if lost.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![format!(
        "{reason}：以下环境变量重启后不再注入（状态记录只存变量名，值无法回读）——{}",
        lost.join(", ")
    )])
}

/// profile 变了先说一句：`restart` 已按声明生效，别让人以为还是旧的那份。
fn profile_drift_note(rec: &InstanceRecord, args: &UpArgs) -> Option<String> {
    if rec.profile == args.profile {
        return None;
    }
    Some(format!(
        "本次按声明生效：profile `{}` → `{}`",
        rec.profile.as_deref().unwrap_or("（无）"),
        args.profile.as_deref().unwrap_or("（无）")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// `restart` 用**记录**重建启动规格，所以没被搬过去的字段会静默消失。
    /// `yes` 就是这样：丢了不报错，只让重启后的实例把命令白名单又打开
    /// （方向是收紧，但已经不是用户要求重启的那个实例）。
    #[test]
    fn carries_recorded_yes_and_owner() {
        let mut rec = InstanceRecord::new("a");
        rec.yes = true;
        rec.managed_by = Some("supervise".to_string());
        rec.port = Some(8081);
        rec.profile = Some("dev".to_string());
        rec.config = Some("/tmp/serve.toml".to_string());

        let args = args_from_record(&rec, None, Duration::from_secs(7));

        assert!(args.yes, "`--yes` 必须随记录搬过去；丢了等于重启后把白名单又打开");
        assert_eq!(
            args.origin,
            Origin::Supervise,
            "持有者要沿用，否则 supervise 不再回收这个实例"
        );
        assert_eq!(args.port, Some(8081), "端口沿用记录，而不是重新分配");
        assert_eq!(args.profile.as_deref(), Some("dev"));
        assert_eq!(
            args.serve_config.as_deref(),
            Some("/tmp/serve.toml"),
            "没有 profile 时回落到记录里的 config"
        );
        assert_eq!(
            args.timeout,
            Duration::from_secs(7),
            "就绪等待来自 `--timeout`，不是终止宽限 `--grace`"
        );
    }

    /// profile 解析成功时 `serve_config` / `env` / `no_tools` 以**当前** presets 为准
    /// （`restart` 是「按声明重建」）：否则 `--no-tools` 会在重启后消失，属放松方向漂移。
    #[test]
    fn prefers_resolved_profile_over_record() {
        let mut rec = InstanceRecord::new("a");
        rec.config = Some("/tmp/stale.toml".to_string());
        let mut env = BTreeMap::new();
        env.insert("CM_LOG".to_string(), "debug".to_string());
        let resolved = profiles::Resolved {
            serve_config: Some("/tmp/profile.toml".to_string()),
            env: env.clone(),
            no_tools: true,
        };

        let args = args_from_record(&rec, Some(&resolved), Duration::from_secs(10));

        assert_eq!(args.serve_config.as_deref(), Some("/tmp/profile.toml"));
        assert_eq!(args.env, env);
        assert!(args.no_tools, "profile 的 --no-tools 必须带过去");
        assert!(
            args.profile_fingerprint.is_some(),
            "指纹用于发现「改了内容但没改名」"
        );
        assert!(!args.yes, "记录里没有 --yes 时不能凭 profile 新增");
    }

    /// 待绑定实例 `restart` 后**仍然**是待绑定（#1183 §2 ②：接受无人值守退化，不回填）。
    ///
    /// 与 `carries_recorded_yes_and_owner` 同一种失败模式：`None` 一旦被「顺手补上」
    /// 配置里的默认目录，运行时状态就悄悄变成了声明状态——用户重启一个「等 client
    /// 绑定」的实例，回来却指向自己从未选过的目录，全程没有任何信号。
    #[test]
    fn pending_workspace_stays_pending_after_restart() {
        let mut rec = InstanceRecord::new("a");
        rec.workspace = None;

        let args = args_from_record(&rec, None, Duration::from_secs(5));
        assert_eq!(
            args.workspace, None,
            "待绑定实例重启后必须保持待绑定；补一个默认目录就是把运行时状态偷换成声明状态"
        );

        // 对照组：记录里真有路径时必须原样搬过去，
        // 否则「不回填」会退化成「永远不带工作区」，两次重启后漂移成另一个实例。
        rec.workspace = Some("/tmp/proj-a".to_string());
        let args = args_from_record(&rec, None, Duration::from_secs(5));
        assert_eq!(args.workspace.as_deref(), Some("/tmp/proj-a"));
    }

    // ---- #1212：`restart` 丢弃声明 env ----

    fn declaration(profile: Option<&str>, workspace: Option<&str>) -> Declaration {
        Declaration {
            workspace: workspace.map(str::to_string),
            config: None,
            profile: profile.map(str::to_string),
            port: None,
            enabled: true,
            env: BTreeMap::new(),
        }
    }

    fn with_env(mut decl: Declaration, pairs: &[(&str, &str)]) -> Declaration {
        for (key, val) in pairs {
            decl.env.insert((*key).to_string(), (*val).to_string());
        }
        decl
    }

    /// #1212 的回归钉：`[instances.a.env]` 必须在 `restart` 后仍然生效。
    ///
    /// 修复前这里无论如何都得到空 `env`：有 profile 时 `resolve(..., &BTreeMap::new(), ..)`
    /// 把实例级 env 当成空表，没有 profile 时 `resolved = None` 直接 `unwrap_or_default()`。
    /// 而记录只存变量名，丢掉的值不可回读——所以这条用例失败时不只是少一个变量。
    #[test]
    fn declared_env_survives_restart() {
        let rec = InstanceRecord::new("a");
        let decl = with_env(declaration(None, None), &[("FOO", "bar")]);

        let plan = plan_restart(&rec, Some(&decl), None, Duration::from_secs(5)).expect("应成功");

        assert_eq!(
            plan.args.env.get("FOO").map(String::as_str),
            Some("bar"),
            "声明里的 env 必须进启动规格；丢了就等于把实例换成另一份配置"
        );
    }

    /// 声明真的在磁盘上时走的是 `load_restart_plan`，把「写好的 `[instances.a.env]`
    /// → 启动规格」串起来钉一次：`plan_restart` 的单测证明不了读盘这一段没把 env 弄丢。
    #[test]
    fn load_restart_plan_reads_declaration_from_disk() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "cm_relaunch_plan_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let instances = dir.join("instances.toml");
        std::fs::write(&instances, "[instances.a]\n\n[instances.a.env]\nFOO = \"bar\"\n")
            .expect("写声明");
        std::fs::set_permissions(&instances, std::fs::Permissions::from_mode(0o600))
            .expect("声明必须 0600");
        // profiles.toml 不存在：`load_optional` 视为空表
        let profiles_path = dir.join("profiles.toml");

        let rec = InstanceRecord::new("a");
        let plan = load_restart_plan(&rec, &instances, &profiles_path, Duration::from_secs(5))
            .expect("应成功");

        assert_eq!(
            plan.args.env.get("FOO").map(String::as_str),
            Some("bar"),
            "声明里的 env 必须穿过读盘这一段"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// profile 生效时 `Resolved.env` 已经是「派生键 → `profile.env` → `instance.env`」
    /// 的合并结果，`restart` 照搬即可，不得再被记录或空表覆盖。
    #[test]
    fn resolved_profile_env_is_passed_through() {
        let rec = InstanceRecord::new("a");
        let decl = with_env(declaration(Some("dev"), None), &[("FOO", "instance")]);
        let mut env = BTreeMap::new();
        env.insert("FOO".to_string(), "instance".to_string());
        let resolved = profiles::Resolved {
            serve_config: None,
            env: env.clone(),
            no_tools: false,
        };

        let plan =
            plan_restart(&rec, Some(&decl), Some(&resolved), Duration::from_secs(5)).expect("应成功");

        assert_eq!(plan.args.env, env, "profile 解析结果就是最终 env");
    }

    /// 声明省略 `workspace` 是「待绑定」，但运行期可能已被 client 绑定过：
    /// 重启不能把已绑定的目录丢掉（那是把运行时状态偷换回声明状态）。
    #[test]
    fn pending_declaration_keeps_bound_workspace() {
        let mut rec = InstanceRecord::new("a");
        rec.workspace = Some("/tmp/bound".to_string());

        let plan = plan_restart(&rec, Some(&declaration(None, None)), None, Duration::from_secs(5))
            .expect("应成功");
        assert_eq!(
            plan.args.workspace.as_deref(),
            Some("/tmp/bound"),
            "声明缺省 ≠ 取消绑定"
        );

        let decl = declaration(None, Some("/tmp/declared"));
        let plan = plan_restart(&rec, Some(&decl), None, Duration::from_secs(5)).expect("应成功");
        assert_eq!(
            plan.args.workspace.as_deref(),
            Some("/tmp/declared"),
            "声明给了 workspace 就以声明为准"
        );
    }

    /// 未声明实例的 env 恢复不了（记录只存名字）——那就必须**报出来**。
    #[test]
    fn undeclared_instance_reports_lost_env() {
        let mut rec = InstanceRecord::new("a");
        rec.env_keys = vec!["GIT_SSH_COMMAND".to_string()];

        let plan = plan_restart(&rec, None, None, Duration::from_secs(5)).expect("应成功");

        assert!(plan.args.env.is_empty(), "记录拿不回值，只能是空表");
        assert_eq!(plan.notes.len(), 1, "应有一条丢失提示：{:?}", plan.notes);
        assert!(
            plan.notes[0].contains("GIT_SSH_COMMAND"),
            "提示要点出变量名：{}",
            plan.notes[0]
        );
    }

    /// 声明里删掉了某个变量，同样要报——否则「改了声明」与「变量消失」之间没有线索。
    #[test]
    fn declared_instance_reports_env_dropped_from_declaration() {
        let mut rec = InstanceRecord::new("a");
        rec.env_keys = vec!["GIT_SSH_COMMAND".to_string()];

        let plan = plan_restart(&rec, Some(&declaration(None, None)), None, Duration::from_secs(5))
            .expect("应成功");

        assert!(
            plan.notes.iter().any(|n| n.contains("GIT_SSH_COMMAND")),
            "{:?}",
            plan.notes
        );
    }

    /// `enabled = false` 的实例被人工重启：允许（这是用户明确的意图），但要说清
    /// 下一轮 `supervise` 会把它停掉；profile 变更也要点名。
    #[test]
    fn disabled_and_profile_drift_are_flagged() {
        let mut rec = InstanceRecord::new("a");
        rec.profile = Some("dev".to_string());
        let mut decl = declaration(Some("prod"), None);
        decl.enabled = false;

        let plan = plan_restart(&rec, Some(&decl), None, Duration::from_secs(5)).expect("应成功");

        assert_eq!(plan.args.profile.as_deref(), Some("prod"), "声明是事实来源");
        assert!(
            plan.notes.iter().any(|n| n.contains("enabled = false")),
            "{:?}",
            plan.notes
        );
        assert!(
            plan.notes
                .iter()
                .any(|n| n.contains("dev") && n.contains("prod")),
            "{:?}",
            plan.notes
        );
    }

    /// 声明里出现敏感键：在 `down` **之前**就报错。擦边用例——修复前要等到 spawn
    /// 才失败，那时实例已经被停掉了。
    #[test]
    fn forbidden_declared_env_fails_before_any_action() {
        let rec = InstanceRecord::new("a");
        let decl = with_env(declaration(None, None), &[("API_KEY", "placeholder")]);

        let err = plan_restart(&rec, Some(&decl), None, Duration::from_secs(5))
            .expect_err("敏感键应被拒绝");
        assert_eq!(err.error_code, "forbidden_env");
    }
}
