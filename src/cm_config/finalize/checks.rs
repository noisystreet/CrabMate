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

/// `unrestricted`（L4）× Docker 沙盒是**拒绝组合**（设计 §6.4）。
///
/// L4 的承诺是「不受工作区路径与命令白名单限制、在宿主上跑」，而 Docker 沙盒恰恰把
/// 路径与网络两轴钳掉——两者同时要就是自相矛盾。设计选了**二选一**（启动期报错），
/// 而不是偷偷把 L4 降成 L2：那样「我明明开了 L4」会变成一句谎话。
///
/// 判据只认**显式/生效的 L4**：
/// - 默认档 `permission_level = unrestricted`（进程默认就在 L4），或
/// - 显式 `permission_level_max = unrestricted`（运维明确允许切到 L4）。
///
/// **不**把「loopback 自动推导出 L4」算进来：那是绑定地址的副作用，用户并没说要 L4；
/// 算进来会让「loopback + 开 Docker 沙盒」这个常见组合直接起不来。
pub(super) fn validate_unrestricted_with_docker_sandbox(
    mode: types::SyncDefaultToolSandboxMode,
    b: &ConfigBuilder,
) -> Result<(), String> {
    if mode != types::SyncDefaultToolSandboxMode::Docker {
        return Ok(());
    }
    let default_l4 = is_unrestricted(b.permission.permission_level_str.as_deref());
    let ceiling_l4 = is_unrestricted(b.permission.permission_level_max_str.as_deref());
    if !default_l4 && !ceiling_l4 {
        return Ok(());
    }
    Err(
        "配置错误：sync_default_tool_sandbox_mode = \"docker\" 与 unrestricted 权限等级是互斥组合\
         （设计 §6.4：要宿主级就别进容器，要隔离就别要 L4）。\
         请二选一：把 sync_default_tool_sandbox_mode 改回 \"none\"，\
         或把 permission_level / permission_level_max 降到 approved 及以下"
            .to_string(),
    )
}

/// 配置串是否为 `unrestricted`（解析失败按「不是」处理，错误由
/// [`derive_permission_config`] 在解析阶段报出，不在这里重复）。
fn is_unrestricted(raw: Option<&str>) -> bool {
    matches!(
        crate::cm_types::parse_optional_permission_level(raw),
        Ok(Some(crate::cm_types::PermissionLevel::Unrestricted))
    )
}

/// `subagent_max_depth` 目前只支持 `1`（禁止嵌套）。
///
/// **为什么不是静默忽略**：这个键名（「深度上限」）暗示可以配 > 1，但首版没有嵌套实现
/// ——派生子任务时会**显式排除 `delegate_task` 自己**（`readonly_subset_for_subturn`），
/// 子任务根本无法再派。留着「配了不生效」比报错更糟：用户会以为已开启嵌套，
/// 实际直到发现子任务看不到该工具才察觉。
///
/// 将来实现真嵌套时，这里的上限与派生逻辑（`max_depth > 1` 时保留本工具）需一起放宽。
pub(super) fn validate_subagent_depth(b: &ConfigBuilder) -> Result<(), String> {
    if let Some(d) = b.subagent.max_depth
        && d != 1
    {
        return Err(format!(
            "配置错误：subagent_max_depth = {d} 暂不支持（首版只允许 1 = 禁止嵌套）。             子任务会被显式排除 `delegate_task`，配置 > 1 不会生效。"
        ));
    }
    Ok(())
}

/// finalize 前的多项校验合并入口。
///
/// 收成一个函数是为了 `finalize_agent_config` 的 CCN：逐条 `?` 会让它超过 ≤10 的门禁。
pub(super) fn validate_pre_finalize(b: &ConfigBuilder) -> Result<(), String> {
    validate_required_llm_endpoints(b)?;
    validate_subagent_depth(b)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_config::builder::ConfigBuilder;

    fn builder_with_levels(level: Option<&str>, max: Option<&str>) -> ConfigBuilder {
        let mut b = ConfigBuilder::default();
        b.permission.permission_level_str = level.map(str::to_string);
        b.permission.permission_level_max_str = max.map(str::to_string);
        b
    }

    /// §6.4：`unrestricted` × Docker 是拒绝组合；但**不**误伤 loopback 隐式推导的 L4。
    #[test]
    fn rejects_unrestricted_with_docker_sandbox() {
        let docker = types::SyncDefaultToolSandboxMode::Docker;

        // 默认档就是 L4 ⇒ 拒。
        let b = builder_with_levels(Some("unrestricted"), None);
        assert!(validate_unrestricted_with_docker_sandbox(docker, &b).is_err());

        // 显式把上限设成 L4 ⇒ 拒。
        let b = builder_with_levels(None, Some("unrestricted"));
        assert!(validate_unrestricted_with_docker_sandbox(docker, &b).is_err());

        // 默认 approved + 未显式配上限 ⇒ 放行。loopback 会推导出 L4，但那是绑定地址的
        // 副作用、用户没说要不要 L4；算进来会让「lo + docker 沙盒」直接起不来。
        let b = builder_with_levels(Some("approved"), None);
        assert!(validate_unrestricted_with_docker_sandbox(docker, &b).is_ok());

        // 显式更严的等级 ⇒ 放行。
        let b = builder_with_levels(Some("workspace"), Some("workspace"));
        assert!(validate_unrestricted_with_docker_sandbox(docker, &b).is_ok());

        // 非 Docker ⇒ 恒放行。
        let b = builder_with_levels(Some("unrestricted"), Some("unrestricted"));
        assert!(
            validate_unrestricted_with_docker_sandbox(
                types::SyncDefaultToolSandboxMode::None,
                &b
            )
            .is_ok()
        );
    }
}
