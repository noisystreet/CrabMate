//! 配置装配顺序：嵌入分片与用户 TOML 的合并入口，避免 `mod.rs` 中顺序散落难查。
//!
//! ## 合并顺序（自底向上，后者覆盖前者）
//!
//! 1. 嵌入 **`config/default_config.toml`**
//! 2. 嵌入 **`config/session.toml`**
//! 3. 嵌入 **`config/context_inject.toml`**
//! 4. 嵌入 **`config/tools.toml`**（`[agent]` + 可选 **`[tool_registry]`**）
//! 5. 嵌入 **`config/sandbox.toml`**
//! 6. 嵌入 **`config/planning.toml`**
//! 7. 嵌入 **`config/memory.toml`**
//! 8. 用户 **`config.toml`** 或 **`.agent_demo.toml`**（或 **`--config`** 指定单文件；存在则不再探测另一默认名）
//! 9. 可选 **`agent_roles.toml`**（与主配置同目录，或仓库 **`config/agent_roles.toml`**）
//! 10. **`CM_*` 环境变量**（在 `config/env_overrides.rs` 的 `apply_env_overrides` 中应用，本模块不负责）

use super::builder::ConfigBuilder;
use super::source::{parse_agent_section, parse_tools_config_bundle};

/// 编译时嵌入（与 `mod.rs` 中常量一致，仅在此集中说明顺序）
const DEFAULT_CONFIG: &str = include_str!("../../config/default_config.toml");
const SESSION_DEFAULT_CONFIG: &str = include_str!("../../config/session.toml");
const CONTEXT_INJECT_DEFAULT_CONFIG: &str = include_str!("../../config/context_inject.toml");
const TOOLS_DEFAULT_CONFIG: &str = include_str!("../../config/tools.toml");
const SANDBOX_DEFAULT_CONFIG: &str = include_str!("../../config/sandbox.toml");
const PLANNING_DEFAULT_CONFIG: &str = include_str!("../../config/planning.toml");
const MEMORY_DEFAULT_CONFIG: &str = include_str!("../../config/memory.toml");

fn apply_embedded_agent_shard(
    b: &mut ConfigBuilder,
    shard_label: &'static str,
    toml_src: &'static str,
) -> Result<(), String> {
    let agent = parse_agent_section(toml_src).map_err(|e| {
        format!("嵌入默认配置 {shard_label} TOML 无效（须与仓库 config 一致）: {e}")
    })?;
    if let Some(agent) = agent {
        b.apply_section(agent);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn apply_embedded_agent_shard_for_test(
    b: &mut ConfigBuilder,
    shard_label: &'static str,
    toml_src: &'static str,
) -> Result<(), String> {
    apply_embedded_agent_shard(b, shard_label, toml_src)
}

/// 应用全部嵌入默认分片（步骤 1–7）。
pub(super) fn apply_embedded_config_shards(b: &mut ConfigBuilder) -> Result<(), String> {
    apply_embedded_agent_shard(b, "default_config.toml", DEFAULT_CONFIG)?;
    apply_embedded_agent_shard(b, "session.toml", SESSION_DEFAULT_CONFIG)?;
    apply_embedded_agent_shard(b, "context_inject.toml", CONTEXT_INJECT_DEFAULT_CONFIG)?;

    let (tools_agent, tools_tr) = parse_tools_config_bundle(TOOLS_DEFAULT_CONFIG)
        .map_err(|e| format!("嵌入默认配置 tools.toml TOML 无效（须与仓库 config 一致）: {e}"))?;
    if let Some(agent) = tools_agent {
        b.apply_section(agent);
    }
    if let Some(tr) = tools_tr {
        b.apply_tool_registry(tr);
    }

    apply_embedded_agent_shard(b, "sandbox.toml", SANDBOX_DEFAULT_CONFIG)?;
    apply_embedded_agent_shard(b, "planning.toml", PLANNING_DEFAULT_CONFIG)?;
    apply_embedded_agent_shard(b, "memory.toml", MEMORY_DEFAULT_CONFIG)?;
    Ok(())
}

pub(super) use super::user_config_layers::merge_user_config_layers;
