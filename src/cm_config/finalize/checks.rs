use super::*;

pub(super) fn validate_ltm_backend_when_enabled(
    enabled: bool,
    backend: LongTermMemoryVectorBackend,
) -> Result<(), String> {
    if !enabled {
        return Ok(());
    }
    match backend {
        LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector => Err(
            "配置错误：长期记忆向量后端 qdrant / pgvector 尚未接入；请使用 disabled 或 fastembed，或关闭 long_term_memory_enabled"
                .to_string(),
        ),
        LongTermMemoryVectorBackend::Disabled | LongTermMemoryVectorBackend::Fastembed => Ok(()),
    }
}

pub(super) fn validate_docker_sandbox_image(
    mode: types::SyncDefaultToolSandboxMode,
    image: &str,
) -> Result<(), String> {
    if mode == types::SyncDefaultToolSandboxMode::Docker && image.trim().is_empty() {
        return Err(
            "配置错误：sync_default_tool_sandbox_mode=docker 时必须设置非空的 sync_default_tool_sandbox_docker_image"
                .to_string(),
        );
    }
    Ok(())
}

pub(super) fn validate_required_llm_endpoints(b: &ConfigBuilder) -> Result<(), String> {
    if b.llm.api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 CM_API_BASE 中设置）".to_string());
    }
    if b.llm.model.is_empty() {
        return Err("配置错误：未设置 model（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 CM_MODEL 中设置）".to_string());
    }
    Ok(())
}

pub(super) fn canonical_run_command_working_dir(b: &ConfigBuilder) -> Result<PathBuf, String> {
    let raw = b
        .command_exec
        .run_command_working_dir
        .clone()
        .ok_or("配置错误：未设置 run_command_working_dir（请在 config/tools.toml、config.toml、.agent_demo.toml 或环境变量 CM_RUN_COMMAND_WORKING_DIR 中设置）")?;
    let p = Path::new(&raw);
    let p = match p.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            return Err(format!(
                "配置错误：run_command_working_dir \"{}\" 不存在或无法解析: {}",
                p.display(),
                e
            ));
        }
    };
    if !p.is_dir() {
        return Err(format!(
            "配置错误：run_command_working_dir \"{}\" 不是目录",
            p.display()
        ));
    }
    Ok(p)
}

pub(super) fn parse_builder_default_session_mode(
    b: &ConfigBuilder,
) -> Result<crate::cm_types::SessionMode, String> {
    match b
        .roles_prompts
        .default_session_mode
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => Ok(crate::cm_types::SessionMode::Act),
        Some(s) => crate::cm_types::parse_session_mode(s)
            .map_err(|e| format!("配置错误：default_session_mode：{e}")),
    }
}

pub(super) fn resolve_finalize_workspace_paths(
    b: &ConfigBuilder,
) -> Result<(PathBuf, Vec<PathBuf>, Option<PathBuf>), String> {
    let run_command_working_dir = canonical_run_command_working_dir(b)?;
    let workspace_allowed_roots = workspace_roots::resolve_workspace_allowed_roots(
        b.workspace_roots.workspace_allowed_roots.clone(),
        run_command_working_dir.as_path(),
    )?;
    let web_workspace_pool = workspace_roots::resolve_web_workspace_pool(
        b.workspace_roots.web_workspace_pool.clone(),
        workspace_allowed_roots.as_slice(),
    )?;
    Ok((
        run_command_working_dir,
        workspace_allowed_roots,
        web_workspace_pool,
    ))
}
