//! 用户 TOML 与角色侧车文件的合并（原 `assembly.rs` 中步骤 8–9）。

use std::path::{Path, PathBuf};

use super::agent_roles;
use super::builder::{ConfigBuilder, override_opt_string_non_empty};
use super::source::parse_config_file_roles;

/// 合并用户 TOML（步骤 8–9），返回 `system_prompt_file` 相对路径解析用的配置目录栈（先发现者在前，后加载在后）。
pub(super) fn merge_user_config_layers(
    config_path: Option<&str>,
    b: &mut ConfigBuilder,
) -> Result<Vec<PathBuf>, String> {
    let config_paths: Vec<&str> = match config_path {
        Some(p) => {
            let p = p.trim();
            if p.is_empty() { vec![] } else { vec![p] }
        }
        None => vec!["config.toml", ".agent_demo.toml"],
    };

    let mut system_prompt_search_bases: Vec<PathBuf> = Vec::new();

    merge_from_primary_user_files(
        &config_paths,
        config_path,
        b,
        &mut system_prompt_search_bases,
    )?;
    merge_agent_roles_sidecar(config_path, b)?;

    Ok(system_prompt_search_bases)
}

fn merge_from_primary_user_files(
    config_paths: &[&str],
    config_path: Option<&str>,
    b: &mut ConfigBuilder,
    system_prompt_search_bases: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for path in config_paths {
        if Path::new(path).exists() {
            apply_one_existing_user_config(path, b, system_prompt_search_bases)?;
            if config_path.is_some() {
                break;
            }
        } else if config_path.is_some() {
            return Err(format!("配置文件 \"{path}\" 不存在"));
        }
    }
    Ok(())
}

fn apply_one_existing_user_config(
    path: &str,
    b: &mut ConfigBuilder,
    system_prompt_search_bases: &mut Vec<PathBuf>,
) -> Result<(), String> {
    system_prompt_search_bases.push(directory_containing_config_file(path));
    let s =
        std::fs::read_to_string(path).map_err(|e| format!("无法读取配置文件 \"{path}\": {e}"))?;
    let (agent_opt, role_rows, tr_opt, sched_rows) = parse_config_file_roles(&s)
        .map_err(|e| format!("配置文件 \"{path}\" TOML 解析失败: {e}"))?;
    if let Some(agent) = agent_opt {
        b.apply_section(agent);
    }
    b.merge_agent_role_rows(&role_rows);
    b.merge_scheduled_agent_task_rows(&sched_rows);
    if let Some(tr) = tr_opt {
        b.apply_tool_registry(tr);
    }
    Ok(())
}

fn merge_agent_roles_sidecar(
    config_path: Option<&str>,
    b: &mut ConfigBuilder,
) -> Result<(), String> {
    let sidecar_path = resolve_agent_roles_sidecar_path(config_path);
    let Some(sc) = sidecar_path.filter(|p| p.exists()) else {
        return Ok(());
    };

    let s = std::fs::read_to_string(&sc)
        .map_err(|e| format!("无法读取角色配置文件 \"{}\": {}", sc.display(), e))?;
    let mut default_slot: Option<String> = None;
    agent_roles::merge_agent_roles_file_into_builder(
        &s,
        &mut default_slot,
        &mut b.agent_role_entries,
    )?;
    override_opt_string_non_empty(&mut b.roles_prompts.default_agent_role_id, default_slot);
    Ok(())
}

fn resolve_agent_roles_sidecar_path(config_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = config_path
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Path::new(p)
            .parent()
            .map(|dir| dir.join("agent_roles.toml"))
    } else {
        Some(Path::new("config/agent_roles.toml").to_path_buf())
    }
}

/// `system_prompt_file` 相对路径解析：与 `foo.toml` 同目录下的 `config/prompts/...` 等可被找到。
fn directory_containing_config_file(config_path: &str) -> PathBuf {
    let p = Path::new(config_path);
    match p.parent() {
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        Some(parent) if parent.as_os_str().is_empty() => {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
        Some(parent) => parent.to_path_buf(),
    }
}
