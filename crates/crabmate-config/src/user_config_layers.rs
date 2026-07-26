//! 用户 TOML 与角色侧车文件的合并（原 `assembly.rs` 中步骤 8–9）。

use std::path::{Path, PathBuf};

use super::agent_roles;
use super::builder::{ConfigBuilder, override_opt_string_non_empty};
use super::source::parse_config_file_roles;
use super::user_config_xdg::{
    cwd_has_local_user_config, resolve_default_user_config_toml_after_seed,
};

/// 合并用户 TOML（步骤 8–9），返回 `system_prompt_file` 相对路径解析用的配置目录栈（先发现者在前，后加载在后）。
pub(super) fn merge_user_config_layers(
    config_path: Option<&str>,
    b: &mut ConfigBuilder,
) -> Result<Vec<PathBuf>, String> {
    // 无显式 `--config` 时：
    // 1) cwd 已有 `config.toml` / `.agent_demo.toml` → 合并二者（项目本地优先）
    // 2) 否则尝试 `$XDG_CONFIG_HOME/crabmate/config.toml`（可从 `/etc/crabmate` 首次种子；
    //    源码树内默认跳过，除非设了 `CM_CRABMATE_CONFIG_DIR`）
    let xdg_owned = match config_path.map(str::trim).filter(|s| !s.is_empty()) {
        Some(_) => None,
        None if cwd_has_local_user_config() => None,
        None => {
            resolve_default_user_config_toml_after_seed().map(|p| p.to_string_lossy().into_owned())
        }
    };
    let effective_path = config_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(xdg_owned.as_deref());

    let config_paths: Vec<&str> = match effective_path {
        Some(p) => vec![p],
        None => vec!["config.toml", ".agent_demo.toml"],
    };

    let mut system_prompt_search_bases: Vec<PathBuf> = Vec::new();

    merge_from_primary_user_files(
        &config_paths,
        effective_path,
        b,
        &mut system_prompt_search_bases,
    )?;
    merge_agent_roles_sidecar(effective_path, b)?;

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

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use crate::builder::ConfigBuilder;
    use crate::xdg::{ENV_CONFIG_DIR, test_env_lock};

    fn recover_cwd_if_needed() {
        if std::env::current_dir().is_err() {
            let fallback = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let _ = std::env::set_current_dir(&fallback);
        }
    }

    #[test]
    fn cwd_local_config_beats_xdg() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let tmp = tempfile::tempdir().expect("tempdir");
        let xdg = tmp.path().join("xdg");
        let cwd = tmp.path().join("cwd");
        std::fs::create_dir_all(&xdg).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(xdg.join("config.toml"), b"[agent]\nmodel = \"from-xdg\"\n").unwrap();
        std::fs::write(cwd.join("config.toml"), b"[agent]\nmodel = \"from-cwd\"\n").unwrap();

        let prev = std::env::current_dir().unwrap_or_else(|_| cwd.clone());
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::set_var(ENV_CONFIG_DIR, &xdg);
        }
        std::env::set_current_dir(&cwd).unwrap();

        let mut b = ConfigBuilder::default();
        let bases = merge_user_config_layers(None, &mut b).unwrap();

        let _ = std::env::set_current_dir(&prev);
        unsafe {
            std::env::remove_var(ENV_CONFIG_DIR);
        }

        assert_eq!(b.llm.model, "from-cwd");
        assert_eq!(bases, vec![cwd]);
    }

    #[test]
    fn xdg_used_when_no_cwd_local_and_config_dir_set() {
        let _guard = test_env_lock();
        recover_cwd_if_needed();
        let tmp = tempfile::tempdir().expect("tempdir");
        let xdg = tmp.path().join("xdg");
        let cwd = tmp.path().join("cwd");
        std::fs::create_dir_all(&xdg).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(xdg.join("config.toml"), b"[agent]\nmodel = \"from-xdg\"\n").unwrap();

        let prev = std::env::current_dir().unwrap_or_else(|_| cwd.clone());
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::set_var(ENV_CONFIG_DIR, &xdg);
        }
        std::env::set_current_dir(&cwd).unwrap();

        let mut b = ConfigBuilder::default();
        let bases = merge_user_config_layers(None, &mut b).unwrap();

        let _ = std::env::set_current_dir(&prev);
        unsafe {
            std::env::remove_var(ENV_CONFIG_DIR);
        }

        assert_eq!(b.llm.model, "from-xdg");
        assert_eq!(bases, vec![xdg]);
    }
}
