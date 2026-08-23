/// `sync_default_tool_sandbox_mode = docker` 时，在宿主完成审批/白名单后把本类工具交给容器内 `tool-runner-internal`。
async fn dispatch_non_sync_tool_to_docker(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    kind: &str,
    args: &str,
    runner_cfg_path: Result<PathBuf, String>,
) -> Option<(String, Option<serde_json::Value>)> {
    if env.cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode
        != SyncDefaultToolSandboxMode::Docker
    {
        return None;
    }
    if !workspace_is_set {
        return Some((
            "错误：未设置工作区，无法在 Docker 沙盒中执行该工具（请先设置工作区目录）。"
                .to_string(),
            None,
        ));
    }
    let path = match runner_cfg_path {
        Ok(p) => p,
        Err(e) => return Some((e, None)),
    };
    let inv = crate::cm_internal::tool_sandbox::ToolInvocationLine {
        kind: kind.to_string(),
        tool: None,
        args_json: args.to_string(),
    };
    let out = crate::cm_internal::tool_sandbox::run_tool_in_docker(
        env.sandbox_backend,
        env.cfg.as_ref(),
        effective_working_dir,
        path,
        inv,
    )
    .await;
    Some(match out {
        Ok(s) => (s, None),
        Err(e) => (e, None),
    })
}
