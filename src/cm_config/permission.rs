//! 权限等级的**服务端上限**（ceiling）：按部署形态推导、启动期冻结、热重载只降不升。
//!
//! 设计见 `docs/design/permission_levels.md`：§5.1（推导）、§9 MB-2 / MB-3（客户端与 HTTP 都
//! 不得抬升）、§9.1（热重载可降不可升）。
//!
//! 等级本身的回合解析链与六轴投影在 [`crate::cm_agent::permission`]——依赖方向不允许本模块
//! 反向依赖 `cm_agent`。

use std::net::IpAddr;

use crate::cm_types::{CeilingSource, PermissionLevel};

use super::types::PermissionConfig;

/// 按部署形态推导上限（设计 §5.1）。
///
/// | 输入 | 结果 | `ceiling_source` |
/// |---|---|---|
/// | 显式 `permission_level_max` | 按配置 | `config` |
/// | 未配置 + loopback | `unrestricted` | `derived_loopback` |
/// | 未配置 + 非 loopback | `approved` | `derived_non_loopback` |
/// | 未配置 + **不知道**绑定地址（非 `serve` 入口） | `approved` | `derived_non_loopback` |
///
/// `loopback = None` 表示「判断不了」（例如 `bench` / 运维 CLI 没有监听地址），按**保守**方向
/// 处理，不因为「可能本机」而放宽。
///
/// 非 loopback 下显式要 `unrestricted` 而没开 `allow_unrestricted_permission_for_non_loopback`
/// 时返回 `Err`：这是**启动失败**而不是钳制（MB-3：共享 Bearer 的远端部署不得默认拿到 L4）。
pub fn resolve_permission_ceiling(
    explicit: Option<PermissionLevel>,
    loopback: Option<bool>,
    allow_unrestricted_for_non_loopback: bool,
) -> Result<(PermissionLevel, CeilingSource), String> {
    if let Some(level) = explicit {
        let remote_unrestricted = level == PermissionLevel::Unrestricted
            && loopback == Some(false)
            && !allow_unrestricted_for_non_loopback;
        if remote_unrestricted {
            return Err(
                "配置错误：permission_level_max = \"unrestricted\" 但监听地址为非 loopback。\
                 请显式设置 allow_unrestricted_permission_for_non_loopback = true\
                 （CM_ALLOW_UNRESTRICTED_PERMISSION_FOR_NON_LOOPBACK=1）后重启，\
                 或把上限降回 approved（设计 §9 MB-3）"
                    .to_string(),
            );
        }
        return Ok((level, CeilingSource::Config));
    }
    Ok(match loopback {
        Some(true) => (
            PermissionLevel::Unrestricted,
            CeilingSource::DerivedLoopback,
        ),
        Some(false) | None => (
            PermissionLevel::Approved,
            CeilingSource::DerivedNonLoopback,
        ),
    })
}

/// 热重载后的上限钳制：**只降不升**（设计 §9.1）。
///
/// `startup` 是进程启动时冻结的 `(生效上限, 来源)`；`incoming_explicit` 是热重载读到的显式
/// `permission_level_max`。
///
/// - 显式值**更低** → 立即生效（fail-safe），来源变 `config`；
/// - 显式值**更高** → 不认，保持启动值（抬上限须重启进程：那是另一把钥匙）；
/// - 重载后**未**显式配置 → 保持启动期结论（热重载拿不到绑定地址，无条件重新推导会把
///   `derived_non_loopback` 的 `approved` 悄悄"推导"成别的值）。
pub fn clamp_permission_ceiling_on_reload(
    startup: (PermissionLevel, CeilingSource),
    incoming_explicit: Option<PermissionLevel>,
) -> (PermissionLevel, CeilingSource) {
    let Some(level) = incoming_explicit else {
        return startup;
    };
    let (startup_level, startup_source) = startup;
    // 降与持平都按「配置说了算」，升则不认：`<=` 一次表达，避免两个分支同体。
    if level <= startup_level {
        (level, CeilingSource::Config)
    } else {
        (startup_level, startup_source)
    }
}

/// `serve` 启动期冻结上限：由 `bind_ip` 推导（或无配置时按推导值），写回运行配置。
///
/// 只在启动路径调用一次；`POST /config/reload` 走 [`clamp_permission_ceiling_on_reload`]。
pub async fn freeze_permission_ceiling_for_bind(
    cfg: &super::SharedAgentConfig,
    bind_ip: IpAddr,
) -> Result<(PermissionLevel, CeilingSource), String> {
    let mut guard = cfg.write().await;
    apply_ceiling_for_bind(&mut guard.permission, bind_ip)
}

/// [`freeze_permission_ceiling_for_bind`] 的纯逻辑：推导上限 → 写回生效值 → 用**刚冻结的**
/// 上限再钳制默认等级。
///
/// 第二步顺序不能颠倒：推导值只有在这里才拿得到（`derive_permission_config` 阶段还不知道
/// 绑定地址），所以默认等级的钳制必须落在冻结**之后**，否则 loopback 上
/// `permission_level = "unrestricted"` + 不写上限会被悄悄降成 `approved`。
fn apply_ceiling_for_bind(
    perm: &mut PermissionConfig,
    bind_ip: IpAddr,
) -> Result<(PermissionLevel, CeilingSource), String> {
    let resolved = resolve_permission_ceiling(
        perm.permission_level_max,
        Some(bind_ip.is_loopback()),
        perm.allow_unrestricted_permission_for_non_loopback,
    )?;
    perm.permission_level_max_effective = resolved.0;
    perm.permission_level_max_source = resolved.1;
    perm.permission_level = reconcile_default_level_with_ceiling(perm.permission_level, resolved.0);
    Ok(resolved)
}

/// `permission_level > permission_level_max` 时按上限生效并告警（设计 §10 校验规则：可解释、
/// 不阻断启动）。
pub fn reconcile_default_level_with_ceiling(
    level: PermissionLevel,
    ceiling: PermissionLevel,
) -> PermissionLevel {
    if level > ceiling {
        log::warn!(
            target: "crabmate",
            "配置：permission_level = {level} 高于上限 permission_level_max = {ceiling}；按上限 {ceiling} 生效"
        );
        return ceiling;
    }
    level
}

/// 校验权限配置（启动期）。当前只做**无需工具注册表**的检查：`cm_config` 不得依赖 `cm_tools`，
/// 「`permission_chat_control_tools` / `permission_confirm_tools` 里的名字须存在于
/// `tool_specs_registry`」放在消费点（S2 的 early-deny / 确认门）校验。
pub fn validate_permission_config(perm: &PermissionConfig) -> Result<(), String> {
    if let Some(list) = &perm.permission_read_tools
        && list.is_empty()
    {
        return Err(
            "配置错误：permission_read_tools 不能是空数组（L1 会因此没有任何只读工具）。\
             要「不给任何只读工具」请用 permission_level = \"chat\"，并省略本键以走推导"
                .to_string(),
        );
    }
    Ok(())
}

/// 从累加器解析出 [`PermissionConfig`]（`finalize` 调用）。
///
/// 生效上限的**最终**值仍由启动期冻结（[`freeze_permission_ceiling_for_bind`]）写入；这里的初值
/// 取「显式配置或 `approved`」，因此**未经冻结**的入口（`bench` / 运维 CLI）也落在现状档位。
///
/// 正因初值可能只是占位，默认等级只在**显式配置了上限**时才在此处预钳制；否则留到冻结之后
/// （[`apply_ceiling_for_bind`]），否则推导出的 `unrestricted` 永远不会落到默认等级上。
pub(crate) fn derive_permission_config(
    b: &super::builder::ConfigBuilder,
) -> Result<PermissionConfig, String> {
    let raw = &b.permission;
    let level = crate::cm_types::parse_optional_permission_level(raw.permission_level_str.as_deref())
        .map_err(|e| format!("配置错误：permission_level：{e}"))?
        .unwrap_or_default();
    let explicit_max =
        crate::cm_types::parse_optional_permission_level(raw.permission_level_max_str.as_deref())
            .map_err(|e| format!("配置错误：permission_level_max：{e}"))?;
    let read_tools = raw
        .permission_read_tools
        .as_ref()
        .map(|list| trim_drop_empty(list.clone()));
    let mut cfg = PermissionConfig {
        permission_level: level,
        permission_level_max: explicit_max,
        permission_level_max_effective: explicit_max.unwrap_or_default(),
        permission_level_max_source: if explicit_max.is_some() {
            CeilingSource::Config
        } else {
            // 尚未冻结：报「非 loopback 推导」这个保守来源，冻结成功后会被覆盖。
            CeilingSource::DerivedNonLoopback
        },
        permission_chat_control_tools: trim_drop_empty(
            raw.permission_chat_control_tools.clone().unwrap_or_default(),
        ),
        permission_read_tools: read_tools,
        permission_deny_system_paths: trim_drop_empty(
            raw.permission_deny_system_paths.clone().unwrap_or_default(),
        ),
        permission_confirm_tools: trim_drop_empty(
            raw.permission_confirm_tools.clone().unwrap_or_default(),
        ),
        allow_unrestricted_permission_for_non_loopback: raw
            .allow_unrestricted_permission_for_non_loopback
            .unwrap_or(false),
    };
    validate_permission_config(&cfg)?;
    // 只在**上限已显式配置**时预钳制。未配置上限时 `permission_level_max_effective` 只是
    // `approved` 占位，真值要等启动期按绑定地址冻结（loopback → `unrestricted`）；此刻钳制会把
    // 「loopback + 默认 unrestricted」永久降成 approved，而冻结后不会有人再抬回来。
    // 冻结后的钳制见 [`apply_ceiling_for_bind`]，热重载见 [`clamp_permission_ceiling_on_reload`]。
    if cfg.permission_level_max.is_some() {
        cfg.permission_level = reconcile_default_level_with_ceiling(
            cfg.permission_level,
            cfg.permission_level_max_effective,
        );
    }
    Ok(cfg)
}

/// `trim` 后丢弃空项（配置里的尾随逗号 / 空串不进入生效集合）。
fn trim_drop_empty(list: Vec<String>) -> Vec<String> {
    list.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_ceiling_by_deployment_shape() {
        assert_eq!(
            resolve_permission_ceiling(None, Some(true), false).unwrap(),
            (
                PermissionLevel::Unrestricted,
                CeilingSource::DerivedLoopback
            )
        );
        assert_eq!(
            resolve_permission_ceiling(None, Some(false), false).unwrap(),
            (
                PermissionLevel::Approved,
                CeilingSource::DerivedNonLoopback
            )
        );
        // 没有监听地址（bench / 运维 CLI）：保守按非 loopback 处理。
        assert_eq!(
            resolve_permission_ceiling(None, None, false).unwrap(),
            (
                PermissionLevel::Approved,
                CeilingSource::DerivedNonLoopback
            )
        );
    }

    #[test]
    fn explicit_config_overrides_derivation() {
        assert_eq!(
            resolve_permission_ceiling(Some(PermissionLevel::Workspace), Some(true), false).unwrap(),
            (PermissionLevel::Workspace, CeilingSource::Config)
        );
        assert_eq!(
            resolve_permission_ceiling(
                Some(PermissionLevel::Unrestricted),
                Some(true),
                false
            )
            .unwrap(),
            (PermissionLevel::Unrestricted, CeilingSource::Config)
        );
    }

    #[test]
    fn non_loopback_unrestricted_requires_explicit_switch() {
        let err =
            resolve_permission_ceiling(Some(PermissionLevel::Unrestricted), Some(false), false)
                .expect_err("MB-3：非 loopback 默认不得拿到 L4");
        assert!(err.contains("allow_unrestricted_permission_for_non_loopback"), "{err}");
        assert_eq!(
            resolve_permission_ceiling(Some(PermissionLevel::Unrestricted), Some(false), true)
                .unwrap(),
            (PermissionLevel::Unrestricted, CeilingSource::Config)
        );
        // 非 loopback 但显式要更严的档位：允许。
        assert_eq!(
            resolve_permission_ceiling(Some(PermissionLevel::Workspace), Some(false), false)
                .unwrap(),
            (PermissionLevel::Workspace, CeilingSource::Config)
        );
    }

    #[test]
    fn reload_only_lowers_the_ceiling() {
        let startup = (PermissionLevel::Approved, CeilingSource::DerivedNonLoopback);
        // 降：立即生效。
        assert_eq!(
            clamp_permission_ceiling_on_reload(startup, Some(PermissionLevel::Workspace)),
            (PermissionLevel::Workspace, CeilingSource::Config)
        );
        // 升：不认。
        assert_eq!(
            clamp_permission_ceiling_on_reload(startup, Some(PermissionLevel::Unrestricted)),
            startup
        );
        // 重载后未配置：保持启动期结论（无从重新推导）。
        assert_eq!(clamp_permission_ceiling_on_reload(startup, None), startup);

        // 本机推导出 L4 时，显式写更低的档位仍是「降」。
        let loopback = (PermissionLevel::Unrestricted, CeilingSource::DerivedLoopback);
        assert_eq!(
            clamp_permission_ceiling_on_reload(loopback, Some(PermissionLevel::Approved)),
            (PermissionLevel::Approved, CeilingSource::Config)
        );
    }

    #[test]
    fn default_level_above_ceiling_follows_ceiling() {
        assert_eq!(
            reconcile_default_level_with_ceiling(
                PermissionLevel::Unrestricted,
                PermissionLevel::Workspace
            ),
            PermissionLevel::Workspace
        );
        assert_eq!(
            reconcile_default_level_with_ceiling(
                PermissionLevel::Read,
                PermissionLevel::Workspace
            ),
            PermissionLevel::Read
        );
    }

    #[test]
    fn derive_does_not_clamp_level_before_ceiling_is_frozen() {
        let mut b = crate::cm_config::builder::ConfigBuilder::default();
        b.permission.permission_level_str = Some("unrestricted".to_string());
        let perm = derive_permission_config(&b).expect("derive 应成功");
        // 上限未显式配置：`approved` 只是占位，不得据此钳制用户配置的默认等级。
        assert_eq!(perm.permission_level_max, None);
        assert_eq!(perm.permission_level, PermissionLevel::Unrestricted);
    }

    #[test]
    fn freeze_on_loopback_keeps_configured_unrestricted_level() {
        let mut b = crate::cm_config::builder::ConfigBuilder::default();
        b.permission.permission_level_str = Some("unrestricted".to_string());
        let mut perm = derive_permission_config(&b).expect("derive 应成功");
        let (ceiling, source) =
            apply_ceiling_for_bind(&mut perm, IpAddr::from([127, 0, 0, 1])).expect("loopback 冻结");
        assert_eq!(ceiling, PermissionLevel::Unrestricted);
        assert_eq!(source, CeilingSource::DerivedLoopback);
        assert_eq!(perm.permission_level, PermissionLevel::Unrestricted);
        assert_eq!(perm.effective_ceiling(), PermissionLevel::Unrestricted);
    }

    #[test]
    fn freeze_on_non_loopback_clamps_level_to_derived_approved() {
        let mut b = crate::cm_config::builder::ConfigBuilder::default();
        b.permission.permission_level_str = Some("unrestricted".to_string());
        let mut perm = derive_permission_config(&b).expect("derive 应成功");
        let (ceiling, source) =
            apply_ceiling_for_bind(&mut perm, IpAddr::from([0, 0, 0, 0])).expect("非 loopback 冻结");
        assert_eq!(ceiling, PermissionLevel::Approved);
        assert_eq!(source, CeilingSource::DerivedNonLoopback);
        // 非 loopback 推导出 approved：默认等级按上限生效（可解释，不阻断启动）。
        assert_eq!(perm.permission_level, PermissionLevel::Approved);
    }

    #[test]
    fn explicit_max_still_clamps_derived_level() {
        let mut b = crate::cm_config::builder::ConfigBuilder::default();
        b.permission.permission_level_str = Some("unrestricted".to_string());
        b.permission.permission_level_max_str = Some("workspace".to_string());
        let perm = derive_permission_config(&b).expect("derive 应成功");
        assert_eq!(perm.permission_level, PermissionLevel::Workspace);
        assert_eq!(perm.effective_ceiling(), PermissionLevel::Workspace);
    }

    #[test]
    fn rejects_empty_read_tools_override() {
        let mut perm = crate::cm_config::load_config(None)
            .expect("embed default")
            .permission;
        assert!(validate_permission_config(&perm).is_ok());
        perm.permission_read_tools = Some(Vec::new());
        assert!(validate_permission_config(&perm).is_err());
    }
}
