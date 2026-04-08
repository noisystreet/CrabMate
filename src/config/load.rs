//! 配置加载入口：嵌入分片 → 用户 TOML → 环境变量 → [`super::finalize`]。

use super::assembly;
use super::builder::ConfigBuilder;
use super::env_overrides::apply_env_overrides;
use super::finalize::finalize;
use super::types::AgentConfig;

/// 加载配置：嵌入的 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则依次尝试 config.toml、.agent_demo.toml。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
/// 默认 **`system_prompt_file`** 在 [`super::finalize::finalize`] 中按 cwd、各已加载配置文件目录（逆序）、`run_command_working_dir` 解析相对路径。
pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    let mut b = ConfigBuilder::default();

    // 嵌入默认分片与用户 TOML 的合并顺序见 `assembly` 模块文档。
    assembly::apply_embedded_config_shards(&mut b)?;
    let system_prompt_search_bases = assembly::merge_user_config_layers(config_path, &mut b)?;

    // 环境变量覆盖（优先级最高）
    apply_env_overrides(&mut b);

    finalize(b, system_prompt_search_bases)
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
