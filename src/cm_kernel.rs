//! 工具层与领域层共用的**中立内核**（#1510 P2a）。
//!
//! 这里只收「工具名约定 + 来源/只读判定」这类**两个方向都要用**的纯逻辑：
//!
//! - `cm_tools`（工具实现与注册表）用它决定并行只读批、动态工具后缀、同批去重等；
//! - `cm_agent`（领域层）用它做权限 L1 的只读正列表推导、子任务工具裁剪。
//!
//! 放在任何一侧都会造成反向依赖（`cm_agent → cm_tools` 或反过来），故下沉到本模块。
//! 原有路径 `cm_tools::tool_naming::*` 与 `cm_tools::registry_policy::{is_readonly_tool, …}`
//! 由 `cm_tools` 侧 `pub use` 保持，调用点零涟漪。
//!
//! **依赖方向**：只依赖 `cm_config`（写副作用清单与 `AgentConfig`）与 `std`，
//! **不依赖** `cm_tools` / `cm_agent` —— 由 `scripts/check-crate-deps.sh` 强制。

use std::collections::HashSet;

use crate::cm_config::AgentConfig;
use crate::cm_config::tool_write_effect::builtin_write_effect_tools;

/// OpenAI 兼容 MCP 代理工具名前缀（`mcp__{slug}__{remote_name}`）。
pub const MCP_PROXY_PREFIX: &str = "mcp__";

/// 工作区 `plugins/*.json` 动态工具名前缀。
pub const DYNAMIC_TOOL_PREFIX: &str = "dyn__";

/// MCP 代理工具（`mcp__*`）；语义未知，默认禁止与内建只读工具并行同批。
///
/// **私有**：crate 内只由本模块 [`tool_origin`] 使用（`mcp/*` 的三处 `pub use` 转发
/// 已在 permission_levels.md §17.2 清理时删除）。需要「是不是 MCP 代理」请判
/// [`tool_origin`]，不要重新公开这个谓词。
#[inline]
fn is_mcp_proxy_tool(name: &str) -> bool {
    name.starts_with(MCP_PROXY_PREFIX)
}

/// 运行时动态工具（`dyn__*`）；语义不可静态证明，默认按写副作用处理。
#[inline]
pub fn is_dynamic_tool_name(name: &str) -> bool {
    name.starts_with(DYNAMIC_TOOL_PREFIX)
}

/// 工具来源：内建 / MCP 代理 / 运行时动态工具。
///
/// 后续“外部工具禁用”只需一个谓词（`tool_origin(n) != Builtin`），
/// 不必逐点判断两个前缀——逐点判断必漏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    Builtin,
    Mcp,
    Dynamic,
}

/// 判定工具来源。两个前缀互斥，故判定顺序不影响结果。
#[inline]
pub fn tool_origin(name: &str) -> ToolOrigin {
    if is_mcp_proxy_tool(name) {
        ToolOrigin::Mcp
    } else if is_dynamic_tool_name(name) {
        ToolOrigin::Dynamic
    } else {
        ToolOrigin::Builtin
    }
}

/// 多角色 / 回合工具白名单判定（列表裁剪与执行层须共用本函数，避免分叉）。
///
/// - `allow == None`：不限制。
/// - 内置工具：须出现在集合中。
/// - `mcp__*`：集合含字面量 **`mcp`**（放行全部代理）**或**完整工具名时允许。
#[inline]
pub fn tool_name_allowed_by_turn_allowlist(name: &str, allow: Option<&HashSet<String>>) -> bool {
    let Some(set) = allow else {
        return true;
    };
    if tool_origin(name) == ToolOrigin::Mcp {
        return set.contains("mcp") || set.contains(name);
    }
    set.contains(name)
}

/// 内建工具（非 `mcp__*` / `dyn__*`）。
#[inline]
pub fn is_builtin_tool(name: &str) -> bool {
    tool_origin(name) == ToolOrigin::Builtin
}

/// 纯判定：给定写副作用清单，工具是否只读（**非内建一律 false**）。
///
/// 这是「只读」的唯一口径 —— [`is_readonly_tool`]（按配置清单）与
/// [`is_readonly_tool_for_permission`]（按默认清单）只负责**选清单**，
/// 不各自再实现一遍判定（否则两个口径迟早漂移，见 permission_levels.md §4.2 / §7.2）。
pub fn is_readonly_given(write_effect: &HashSet<String>, name: &str) -> bool {
    is_builtin_tool(name) && !write_effect.contains(name)
}

/// 权限语义的「只读」判定（权限等级 L1 的只读正列表推导用）。
///
/// **不读** `tool_registry_write_effect_tools`：那是并行只读批的**性能开关**（整体替换式），
/// 运维为调并行顺手漏配 `run_command` 就会让 L1 放行子进程——不能拿它当安全边界
/// （设计 `docs/design/permission_levels.md` §4.2 / §7.2）。
///
/// 非内置来源（`mcp__*` / `plugins/*.json` 动态工具）一律**不算**只读：L1 下它们必须被
/// early-deny 拒掉，而不是靠「没进 tools 列表」兜着。
pub fn is_readonly_tool_for_permission(name: &str) -> bool {
    is_readonly_given(builtin_write_effect_tools(), name)
}

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
///
/// 与权限口径的差异只在**清单来源**：这里允许 `[tool_registry]` 的覆盖生效。
pub fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    let writes = match &cfg.tool_registry_policy.tool_registry_write_effect_tools {
        None => builtin_write_effect_tools(),
        Some(arc) => arc.as_ref(),
    };
    is_readonly_given(writes, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_allowlist_mcp_token_and_exact_name() {
        let mut by_token = HashSet::new();
        by_token.insert("mcp".to_string());
        assert!(tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_watchlist_list",
            Some(&by_token)
        ));

        let mut by_exact = HashSet::new();
        by_exact.insert("mcp__fanalyzer__fanalyzer_watchlist_list".to_string());
        assert!(tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_watchlist_list",
            Some(&by_exact)
        ));
        assert!(!tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_analyze",
            Some(&by_exact)
        ));
        assert!(!tool_name_allowed_by_turn_allowlist(
            "read_file",
            Some(&by_exact)
        ));
    }

    #[test]
    fn tool_origin_classifies_three_way() {
        assert_eq!(tool_origin("read_file"), ToolOrigin::Builtin);
        assert_eq!(tool_origin("mcp__fanalyzer__list"), ToolOrigin::Mcp);
        assert_eq!(tool_origin("dyn__deploy"), ToolOrigin::Dynamic);
        // 只在**开头**匹配前缀，不按包含匹配。
        assert_eq!(tool_origin("read_mcp__x"), ToolOrigin::Builtin);
        assert_eq!(tool_origin(""), ToolOrigin::Builtin);
    }

    /// R2 等价验收：同一批工具名上，新枚举映射与旧布尔判定逐点一致。
    #[test]
    fn tool_origin_matches_legacy_predicates() {
        let cases = [
            "read_file",
            "write_file",
            "git",
            "mcp",
            "mcp__a__b",
            "mcp_",
            "dyn",
            "dyn__x",
            "dyn_",
            "read_mcp__x",
            "",
            "MCP__A__B",
            "Dyn__x",
        ];
        for name in cases {
            let origin = tool_origin(name);
            assert_eq!(
                origin == ToolOrigin::Mcp,
                is_mcp_proxy_tool(name),
                "mcp 判定不一致：{name:?}"
            );
            assert_eq!(
                origin == ToolOrigin::Dynamic,
                is_dynamic_tool_name(name),
                "dynamic 判定不一致：{name:?}"
            );
            assert_eq!(
                origin != ToolOrigin::Builtin,
                is_mcp_proxy_tool(name) || is_dynamic_tool_name(name),
                "external 判定不一致：{name:?}"
            );
        }
    }

    /// 只读判定的**固定预期**（不抄实现）：只读工具在、写工具不在、外部来源一律不算。
    #[test]
    fn permission_readonly_matches_documented_expectations() {
        for readonly in ["read_file", "list_tree"] {
            assert!(
                is_readonly_tool_for_permission(readonly),
                "{readonly} 应为只读"
            );
        }
        for writing in [
            "modify_file",
            "search_replace",
            "run_command",
            "delegate_task",
        ] {
            assert!(
                !is_readonly_tool_for_permission(writing),
                "{writing} 不应为只读"
            );
        }
        // 非内建来源即便不在写清单里也不算只读（L1 必须 early-deny）。
        for external in ["mcp__x__y", "dyn__deploy"] {
            assert!(
                !is_readonly_tool_for_permission(external),
                "{external} 不应为只读"
            );
        }
    }

    /// 覆盖只影响「并行执行口径」，不污染「权限口径」—— 两条口径分离是 §4.2 的要求。
    #[test]
    fn readonly_cfg_override_does_not_leak_into_permission_verdict() {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default");
        assert!(is_readonly_tool(&cfg, "read_file"));
        cfg.tool_registry_policy.tool_registry_write_effect_tools = Some(std::sync::Arc::new(
            ["read_file".to_string()].into_iter().collect(),
        ));
        assert!(
            !is_readonly_tool(&cfg, "read_file"),
            "覆盖后 read_file 应被视为写（并行口径）"
        );
        assert!(
            is_readonly_tool_for_permission("read_file"),
            "权限口径不读配置覆盖"
        );
    }
}
