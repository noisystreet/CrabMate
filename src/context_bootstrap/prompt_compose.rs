//! 首条 `system` 动态组装（L3 角色基底 + L4 运行时附录 + L5 按轮 Skills）。
//!
//! Web **`build_messages_for_turn`**、CLI REPL/`chat`、会话恢复与角色切换应共用本模块语义。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::tool_stats::ToolOutcomeRecorder;

/// 未知 `agent_role` 时如何解析 L3 基底。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSystemResolution {
    /// 须在 [`AgentConfig::roles_prompts.agent_roles`] 中存在，否则 `Err`。
    Strict,
    /// 未知 id 时退回全局 [`AgentConfig::roles_prompts.system_prompt`].
    Lenient,
}

/// L5：按当前用户输入从工作区 skills 目录选材时的上下文。
pub struct SkillsComposeContext<'a> {
    pub base_dir: &'a Path,
    pub user_text: &'a str,
}

/// Web / CLI 共用的 skills 扫描根：空工作区路径时回落进程 `cwd`。
#[must_use]
pub fn resolve_skills_base_dir(workspace_root: &Path) -> PathBuf {
    if workspace_root.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        workspace_root.to_path_buf()
    }
}

fn resolve_role_system_base(
    cfg: &AgentConfig,
    agent_role: Option<&str>,
    mode: RoleSystemResolution,
) -> Result<String, String> {
    match cfg.system_prompt_for_new_conversation(agent_role) {
        Ok(s) => Ok(s.to_string()),
        Err(e) => match mode {
            RoleSystemResolution::Strict => Err(e),
            RoleSystemResolution::Lenient => Ok(cfg.roles_prompts.system_prompt.clone()),
        },
    }
}

/// 在已解析的 L3 基底上叠加 L4（思维链附录、工具统计）与可选 L5（Skills top-k）。
pub fn compose_system_from_base(
    base_system: &str,
    cfg: &AgentConfig,
    tool_recorder: &ToolOutcomeRecorder,
    skills: Option<SkillsComposeContext<'_>>,
) -> String {
    let augmented = tool_recorder.augment_system_prompt(base_system, cfg);
    merge_skills_into_system(augmented, cfg, skills)
}

/// 从配置解析 L3，再叠加 L4 + 可选 L5。
pub fn compose_system_for_turn(
    cfg: &AgentConfig,
    agent_role: Option<&str>,
    tool_recorder: &ToolOutcomeRecorder,
    skills: Option<SkillsComposeContext<'_>>,
    role_resolution: RoleSystemResolution,
) -> Result<String, String> {
    let base = resolve_role_system_base(cfg, agent_role, role_resolution)?;
    Ok(compose_system_from_base(&base, cfg, tool_recorder, skills))
}

/// 首条 `system` 组装参数（L3 基底 + L4 + 可选 L5）。
pub struct FirstSystemComposeOpts<'a> {
    pub agent_role: Option<&'a str>,
    pub user_msg_for_skills: Option<&'a str>,
    pub skills_base_dir: Option<PathBuf>,
    pub role_resolution: RoleSystemResolution,
}

/// Web / CLI 续聊刷新首条 `system` 的统一入口（L3 + L4 + 可选 L5）。
pub fn compose_first_system_for_turn(
    cfg: &AgentConfig,
    tool_recorder: &Arc<ToolOutcomeRecorder>,
    opts: FirstSystemComposeOpts<'_>,
) -> Result<String, String> {
    let skills_ctx = opts
        .skills_base_dir
        .as_ref()
        .zip(opts.user_msg_for_skills)
        .map(|(base, user)| SkillsComposeContext {
            base_dir: base.as_path(),
            user_text: user,
        });
    compose_system_for_turn_arc(
        cfg,
        opts.agent_role,
        tool_recorder,
        skills_ctx,
        opts.role_resolution,
    )
}

/// 与 [`compose_system_for_turn`] 相同，但 `tool_recorder` 为 `Arc`（Web handler 常用）。
pub fn compose_system_for_turn_arc(
    cfg: &AgentConfig,
    agent_role: Option<&str>,
    tool_recorder: &Arc<ToolOutcomeRecorder>,
    skills: Option<SkillsComposeContext<'_>>,
    role_resolution: RoleSystemResolution,
) -> Result<String, String> {
    compose_system_for_turn(
        cfg,
        agent_role,
        tool_recorder.as_ref(),
        skills,
        role_resolution,
    )
}

/// 在已有 L3+L4 的 `system` 上仅叠加 L5（Skills top-k）。
pub fn merge_skills_for_turn(
    system_prompt: String,
    cfg: &AgentConfig,
    skills: SkillsComposeContext<'_>,
) -> String {
    merge_skills_into_system(system_prompt, cfg, Some(skills))
}

fn merge_skills_into_system(
    system_prompt: String,
    cfg: &AgentConfig,
    skills: Option<SkillsComposeContext<'_>>,
) -> String {
    let Some(sk) = skills else {
        return system_prompt;
    };
    crate::config::skills::merge_system_prompt_with_skills_selected(
        system_prompt.clone(),
        cfg.skills.skills_enabled,
        cfg.skills.skills_dir.as_str(),
        cfg.skills.skills_max_chars,
        sk.base_dir,
        sk.user_text,
        cfg.skills.skills_top_k,
    )
    .unwrap_or(system_prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_lenient_unknown_role_falls_back_to_global_system() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let rec = ToolOutcomeRecorder::new();
        let out = compose_system_for_turn(
            &cfg,
            Some("nonexistent_role_id_xyz"),
            &rec,
            None,
            RoleSystemResolution::Lenient,
        )
        .expect("lenient");
        assert!(!out.trim().is_empty());
        let global = cfg.roles_prompts.system_prompt.trim();
        assert!(
            out.contains(global),
            "lenient compose should include global system prompt"
        );
    }

    #[test]
    fn compose_strict_unknown_role_errors() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let rec = ToolOutcomeRecorder::new();
        let err = compose_system_for_turn(
            &cfg,
            Some("nonexistent_role_id_xyz"),
            &rec,
            None,
            RoleSystemResolution::Strict,
        )
        .expect_err("strict");
        assert!(err.contains("未知的 agent_role"));
    }

    #[test]
    fn compose_from_base_without_skills_matches_augment_only() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let rec = ToolOutcomeRecorder::new();
        let base = "BASE_PROMPT_MARKER";
        let expected = rec.augment_system_prompt(base, &cfg);
        let got = compose_system_from_base(base, &cfg, &rec, None);
        assert_eq!(got, expected);
    }
}
