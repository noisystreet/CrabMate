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

fn allowed_commands_arc_from_builder(b: &ConfigBuilder) -> Arc<[String]> {
    b.command_exec
        .allowed_commands
        .clone()
        .unwrap_or_else(embedded_default_allowed_command_names)
        .into()
}

struct FinalizeMidLayerScalars {
    max_message_history: usize,
    tui_load_session_on_start: bool,
    tui_session_max_messages: usize,
    repl_initial_workspace_messages_enabled: bool,
    command_timeout_secs: u64,
    command_max_output_len: usize,
    max_tokens: u32,
    llm_context_tokens: u32,
    temperature: f32,
    api_timeout_secs: u64,
    api_max_retries: u32,
    api_retry_delay_secs: u64,
    weather_timeout_secs: u64,
    reflection_default_max_rounds: usize,
}

fn clamp_finalize_mid_layer_scalars(b: &ConfigBuilder) -> FinalizeMidLayerScalars {
    FinalizeMidLayerScalars {
        max_message_history: b
            .session_ui
            .max_message_history
            .unwrap_or(32)
            .clamp(1, 1024) as usize,
        tui_load_session_on_start: b.session_ui.tui_load_session_on_start.unwrap_or(false),
        tui_session_max_messages: b
            .session_ui
            .tui_session_max_messages
            .unwrap_or(400)
            .clamp(2, 50_000) as usize,
        repl_initial_workspace_messages_enabled: b
            .session_ui
            .repl_initial_workspace_messages_enabled
            .unwrap_or(false),
        command_timeout_secs: b.command_exec.command_timeout_secs.unwrap_or(30).max(1),
        command_max_output_len: b
            .command_exec
            .command_max_output_len
            .unwrap_or(8192)
            .clamp(1024, 8 * 1024 * 1024) as usize,
        max_tokens: b.llm_sampling.max_tokens.unwrap_or(4096).clamp(256, 32768) as u32,
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

/// 载入 system_prompt 并与 Cursor Rules / Skills 合并；供角色目录解析使用路径字段。
struct PromptMergeForRoles {
    /// 通用 L0（`base_system_prompt.md` 等），未叠加编程层 / cursor rules / skills。
    universal_l0_system_prompt: String,
    system_prompt: String,
    cursor_rules_enabled: bool,
    cursor_rules_dir: String,
    cursor_rules_include_agents_md: bool,
    cursor_rules_max_chars: u64,
    skills_enabled: bool,
    skills_dir: String,
    skills_max_chars: u64,
    skills_top_k: usize,
}

fn merge_system_prompt_layers_for_finalize(
    b: &mut ConfigBuilder,
    system_prompt_search_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<PromptMergeForRoles, String> {
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
    let universal_l0_system_prompt = system_prompt.clone();
    let system_prompt = super::agent_roles::prepend_l0_base_to_role_body(
        &universal_l0_system_prompt,
        embedded_coding_workbench_increment(),
    );
    let cursor_rules_enabled = b.cursor_rules.cursor_rules_enabled.unwrap_or(true);
    let cursor_rules_dir = b
        .cursor_rules
        .cursor_rules_dir
        .clone()
        .unwrap_or_else(|| ".cursor/rules".to_string());
    let cursor_rules_include_agents_md = b
        .cursor_rules
        .cursor_rules_include_agents_md
        .unwrap_or(true);
    let cursor_rules_max_chars = b
        .cursor_rules
        .cursor_rules_max_chars
        .unwrap_or(48_000)
        .clamp(1024, 1_000_000);
    let system_prompt = cursor_rules::merge_system_prompt_with_cursor_rules(
        system_prompt,
        cursor_rules_enabled,
        &cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars as usize,
    )?;
    let skills_enabled = b.skills.skills_enabled.unwrap_or(true);
    let skills_dir = b
        .skills
        .skills_dir
        .clone()
        .unwrap_or_else(|| ".crabmate/skills".to_string());
    let skills_max_chars = b
        .skills
        .skills_max_chars
        .unwrap_or(32_000)
        .clamp(1024, 1_000_000);
    let skills_top_k = b.skills.skills_top_k.unwrap_or(4).clamp(1, 64) as usize;
    let system_prompt = skills::merge_system_prompt_with_skills(
        system_prompt,
        skills_enabled,
        &skills_dir,
        skills_max_chars as usize,
    )?;
    Ok(PromptMergeForRoles {
        universal_l0_system_prompt,
        system_prompt,
        cursor_rules_enabled,
        cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars,
        skills_enabled,
        skills_dir,
        skills_max_chars,
        skills_top_k,
    })
}
