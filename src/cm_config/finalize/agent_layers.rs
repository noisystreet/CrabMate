use super::*;

// `finalize_agent_config` 拆分：`embedded allowed_commands`、中层 clamp、system_prompt + Cursor Rules + Skills。

fn embedded_default_allowed_command_names() -> Vec<String> {
    vec![
        "aclocal".into(),
        "ar".into(),
        "autoconf".into(),
        "automake".into(),
        "autoreconf".into(),
        "basename".into(),
        "bzcat".into(),
        "c++filt".into(),
        "cargo".into(),
        "cat".into(),
        "clang".into(),
        "clang++".into(),
        "cmake".into(),
        "cmp".into(),
        "column".into(),
        "cut".into(),
        "date".into(),
        "df".into(),
        "diff".into(),
        "dirname".into(),
        "du".into(),
        "echo".into(),
        "egrep".into(),
        "env".into(),
        "expand".into(),
        "fgrep".into(),
        "file".into(),
        "find".into(),
        "fmt".into(),
        "fold".into(),
        "free".into(),
        "g++".into(),
        "gcc".into(),
        "git".into(),
        "grep".into(),
        "head".into(),
        "hexdump".into(),
        "hostname".into(),
        "id".into(),
        "join".into(),
        "jq".into(),
        "ld".into(),
        "ldd".into(),
        "ls".into(),
        "lsblk".into(),
        "lscpu".into(),
        "make".into(),
        "ninja".into(),
        "nl".into(),
        "nm".into(),
        "nproc".into(),
        "objdump".into(),
        "od".into(),
        "paste".into(),
        "pkg-config".into(),
        "pre-commit".into(),
        "printenv".into(),
        "ps".into(),
        "pwd".into(),
        "readelf".into(),
        "readlink".into(),
        "realpath".into(),
        "rev".into(),
        "rustc".into(),
        "seq".into(),
        "size".into(),
        "sort".into(),
        "stat".into(),
        "strings".into(),
        "tac".into(),
        "tail".into(),
        "tr".into(),
        "tree".into(),
        "uname".into(),
        "unexpand".into(),
        "uniq".into(),
        "uptime".into(),
        "wc".into(),
        "whereis".into(),
        "which".into(),
        "whoami".into(),
        "xxd".into(),
        "xzcat".into(),
        "zcat".into(),
    ]
}

pub(super) fn allowed_commands_arc_from_builder(b: &ConfigBuilder) -> Arc<[String]> {
    b.command_exec
        .allowed_commands
        .clone()
        .unwrap_or_else(embedded_default_allowed_command_names)
        .into()
}

pub(super) struct FinalizeMidLayerScalars {
    pub(super) max_message_history: usize,
    pub(super) command_timeout_secs: u64,
    pub(super) command_max_output_len: usize,
    pub(super) allow_external_path_with_approval: bool,
    pub(super) max_tokens: u32,
    pub(super) llm_context_tokens: u32,
    pub(super) temperature: f32,
    pub(super) api_timeout_secs: u64,
    pub(super) api_max_retries: u32,
    pub(super) api_retry_delay_secs: u64,
    pub(super) weather_timeout_secs: u64,
    pub(super) reflection_default_max_rounds: usize,
}

pub(super) fn clamp_finalize_mid_layer_scalars(b: &ConfigBuilder) -> FinalizeMidLayerScalars {
    FinalizeMidLayerScalars {
        max_message_history: b
            .session_ui
            .max_message_history
            .unwrap_or(32)
            .clamp(1, 1024) as usize,
        command_timeout_secs: b.command_exec.command_timeout_secs.unwrap_or(30).max(1),
        command_max_output_len: b
            .command_exec
            .command_max_output_len
            .unwrap_or(8192)
            .clamp(1024, 8 * 1024 * 1024) as usize,
        allow_external_path_with_approval: b
            .command_exec
            .allow_external_path_with_approval
            .unwrap_or(true),
        max_tokens: b.llm_sampling.max_tokens.unwrap_or(16384).clamp(256, 32768) as u32,
        llm_context_tokens: b
            .llm_sampling
            .llm_context_tokens
            .unwrap_or(0)
            .min(10_000_000) as u32,
        temperature: b.llm_sampling.temperature.unwrap_or(0.3).clamp(0.0, 2.0) as f32,
        api_timeout_secs: b.llm_http_retry.api_timeout_secs.unwrap_or(60).max(1),
        api_max_retries: b.llm_http_retry.api_max_retries.unwrap_or(2).min(10) as u32,
        api_retry_delay_secs: b.llm_http_retry.api_retry_delay_secs.unwrap_or(2).max(1),
        weather_timeout_secs: b.weather_tool.weather_timeout_secs.unwrap_or(15).max(1),
        reflection_default_max_rounds: b
            .per_plan_policy
            .reflection_default_max_rounds
            .unwrap_or(5)
            .max(1) as usize,
    }
}

/// 载入 system_prompt 并与 Skills 索引合并；供角色目录解析使用路径字段。
///
/// 项目规则 `AGENTS.md` 不再于 finalize 读盘（原 `.cursor/rules` 兼容已移除）：
/// 它按**会话工作区根**在首条 `system` 组装时追加，见 `context_bootstrap::prompt_compose`。
pub(super) struct PromptMergeForRoles {
    /// 通用 L0（`base_system_prompt.md` 等），未叠加编程层 / skills。
    pub(super) universal_l0_system_prompt: String,
    pub(super) coding_workbench_enabled: bool,
    pub(super) coding_workbench_increment_file: String,
    pub(super) coding_workbench_increment: String,
    pub(super) system_prompt: String,
}

fn load_required_system_prompt_for_finalize(
    b: &ConfigBuilder,
    system_prompt_search_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    let system_prompt = if let Some(ref path) = b.roles_prompts.system_prompt_file {
        read_system_prompt_file_resolved(
            path,
            system_prompt_search_bases,
            run_command_working_dir,
        )?
    } else if !b.roles_prompts.system_prompt.trim().is_empty() {
        b.roles_prompts.system_prompt.clone()
    } else {
        return Err(
            "配置错误：未设置 system_prompt_file 或内联 system_prompt（请在 config/default_config.toml、config.toml、环境变量 CM_SYSTEM_PROMPT / CM_SYSTEM_PROMPT_FILE 中配置）".to_string(),
        );
    };
    if system_prompt.trim().is_empty() {
        return Err("配置错误：system_prompt 从文件或内联加载后为空".to_string());
    }
    Ok(system_prompt)
}

fn stack_l0_with_coding_workbench(l0: &str, increment: &str) -> String {
    if increment.trim().is_empty() {
        l0.to_string()
    } else {
        super::agent_roles::prepend_l0_base_to_role_body(l0, increment)
    }
}

pub(super) fn merge_system_prompt_layers_for_finalize(
    b: &mut ConfigBuilder,
    system_prompt_search_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<PromptMergeForRoles, String> {
    let universal_l0_system_prompt = load_required_system_prompt_for_finalize(
        b,
        system_prompt_search_bases,
        run_command_working_dir,
    )?;
    let coding_workbench_enabled = b.roles_prompts.coding_workbench_enabled.unwrap_or(true);
    let coding_workbench_increment_file = b
        .roles_prompts
        .coding_workbench_increment_file
        .clone()
        .unwrap_or_else(|| "config/prompts/coding_workbench_increment.md".to_string());
    let coding_workbench_increment = resolve_coding_workbench_increment(
        coding_workbench_enabled,
        &coding_workbench_increment_file,
        system_prompt_search_bases,
        run_command_working_dir,
    )?;
    let system_prompt =
        stack_l0_with_coding_workbench(&universal_l0_system_prompt, &coding_workbench_increment);
    // 技能 L2 索引不在此拼装：它按**会话工作区根**扫描，与 `skill_read` 同一基准，
    // 见 `context_bootstrap::prompt_compose`（#1326）。
    Ok(PromptMergeForRoles {
        universal_l0_system_prompt,
        coding_workbench_enabled,
        coding_workbench_increment_file,
        coding_workbench_increment,
        system_prompt,
    })
}
