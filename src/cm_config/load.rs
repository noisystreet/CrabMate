//! 配置加载入口：嵌入分片 → 用户 TOML → 环境变量 → [`super::finalize`]。

use super::assembly;
use super::builder::ConfigBuilder;
use super::env_overrides::apply_env_overrides;
use super::finalize::finalize;
use super::layer_precedence::{ConfigLayer, LayerTrace, WatchedValues};
use super::types::AgentConfig;

/// 源码树内嵌默认 `system_prompt_file` 等相对路径的解析锚点（`CARGO_MANIFEST_DIR` = 仓库根）。
fn embedded_repo_config_search_bases() -> Vec<std::path::PathBuf> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![manifest.join("config"), manifest.clone()]
}

/// 加载配置：嵌入的 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则优先 cwd 的 `config.toml` / `.agent_demo.toml`，再尝试 **`$XDG_CONFIG_HOME/crabmate/config.toml`**（可从 **`/etc/crabmate`** 首次种子；源码树内默认跳过，除非设 **`CM_CRABMATE_CONFIG_DIR`**）。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
/// 默认 **`system_prompt_file`** 在 `super::finalize::finalize` 中按 cwd、各已加载配置文件目录（逆序）、`run_command_working_dir` 解析相对路径。
pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    load_config_traced(config_path).map(|(cfg, _)| cfg)
}

/// 与 [`load_config`] 相同，另返回关键键的层溯源（#1342 P1/P2）。
///
/// 只覆盖在 builder 阶段合并的三层（[`super::layer_precedence::is_pre_load_layer`]）；
/// user-data 与 CLI 两层的顺序由 [`super::layer_precedence::apply_post_load_layers`]
/// 在最终 `AgentConfig` 上按**同一张** [`super::layer_precedence::LAYERS`] 表执行。
pub(crate) fn load_config_traced(
    config_path: Option<&str>,
) -> Result<(AgentConfig, LayerTrace), String> {
    let mut b = ConfigBuilder::default();
    let mut trace = LayerTrace::default();
    let empty = WatchedValues::default();

    // 嵌入默认分片与用户 TOML 的合并顺序见 `assembly` 模块文档。
    assembly::apply_embedded_config_shards(&mut b)?;
    let embedded = WatchedValues::from_builder(&b);
    embedded.record_changes(&empty, ConfigLayer::Embedded, &mut trace);

    let mut system_prompt_search_bases = assembly::merge_user_config_layers(config_path, &mut b)?;
    let user_toml = WatchedValues::from_builder(&b);
    user_toml.record_changes(&embedded, ConfigLayer::UserToml, &mut trace);

    for base in embedded_repo_config_search_bases() {
        if base.is_dir() {
            system_prompt_search_bases.push(base);
        }
    }

    // 环境变量覆盖（优先级最高）
    apply_env_overrides(&mut b);
    let env = WatchedValues::from_builder(&b);
    env.record_changes(&user_toml, ConfigLayer::Env, &mut trace);

    // 两道闸（条数 / token 预算）的自检刻意**不**放这里：本层还没有 user-data 覆盖，
    // 看到的窗口可能小于最终生效值，会在窗口被放大后才真正成立的不变量上漏报（#1342）。
    // 自检在 `cm_cli::config_load::load_cli_agent_config`（最终 cfg）里做。
    let cfg = finalize(b, system_prompt_search_bases)?;
    Ok((cfg, trace))
}

/// CLI 子命令入口：加载失败时打印错误并映射为 `InvalidData`，与历史 `lib::run` 行为一致。
pub fn load_config_for_cli(config_path: Option<&str>) -> Result<AgentConfig, std::io::Error> {
    match load_config(config_path) {
        Ok(c) => Ok(c),
        Err(e) => {
            eprintln!("{e}");
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }
    }
}
