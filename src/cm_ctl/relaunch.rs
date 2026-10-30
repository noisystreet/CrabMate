//! 「状态记录 → 启动规格」的纯映射（`restart` 用）。
//!
//! 单独成模块，是为了让这段逻辑能被单测直接钉住：`restart` 本身是 down + up
//! （真的动进程、等 readiness），测试够不到 [`UpArgs`]；而「记录 → 启动规格」正是
//! 「重启后悄悄变成另一个实例」这类 bug 的唯一落点——`--yes` / profile / 持有者
//! 都只在这里搬运，漏一个不会有任何编译期或运行期信号，只会让实例在重启后换了身份。

use std::time::Duration;

use super::ops::{Origin, UpArgs};
use super::profiles;
use super::record::InstanceRecord;

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
}
