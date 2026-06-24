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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirstSystemComposeDiagnostics {
    pub layers_applied: Vec<String>,
    pub chars_l3_base: usize,
    pub chars_l4_augmented: usize,
    pub chars_final: usize,
    pub skills_total_docs: usize,
    pub skills_selected_labels: Vec<String>,
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
    let (merged, _) = compose_first_system_for_turn_with_diagnostics(cfg, tool_recorder, opts)?;
    Ok(merged)
}

pub fn compose_first_system_for_turn_with_diagnostics(
    cfg: &AgentConfig,
    tool_recorder: &Arc<ToolOutcomeRecorder>,
    opts: FirstSystemComposeOpts<'_>,
) -> Result<(String, FirstSystemComposeDiagnostics), String> {
    let skills_ctx = opts
        .skills_base_dir
        .as_ref()
        .zip(opts.user_msg_for_skills)
        .map(|(base, user)| SkillsComposeContext {
            base_dir: base.as_path(),
            user_text: user,
        });
    let base = resolve_role_system_base(cfg, opts.agent_role, opts.role_resolution)?;
    let augmented = tool_recorder.augment_system_prompt(&base, cfg);
    let chars_l4_augmented = augmented.chars().count();
    let (merged, skills_meta) = merge_skills_into_system_with_meta(augmented, cfg, skills_ctx);
    let chars_final = merged.chars().count();
    let mut layers = vec!["L3".to_string(), "L4".to_string()];
    if !skills_meta.selected_labels.is_empty() {
        layers.push("L5".to_string());
    }
    Ok((
        merged,
        FirstSystemComposeDiagnostics {
            layers_applied: layers,
            chars_l3_base: base.chars().count(),
            chars_l4_augmented,
            chars_final,
            skills_total_docs: skills_meta.total_docs,
            skills_selected_labels: skills_meta.selected_labels,
        },
    ))
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

fn merge_skills_into_system(
    system_prompt: String,
    cfg: &AgentConfig,
    skills: Option<SkillsComposeContext<'_>>,
) -> String {
    merge_skills_into_system_with_meta(system_prompt, cfg, skills).0
}

fn merge_skills_into_system_with_meta(
    system_prompt: String,
    cfg: &AgentConfig,
    skills: Option<SkillsComposeContext<'_>>,
) -> (String, crate::config::skills::SkillsSelectionMeta) {
    let Some(sk) = skills else {
        return (
            system_prompt,
            crate::config::skills::SkillsSelectionMeta::default(),
        );
    };
    crate::config::skills::merge_system_prompt_with_skills_selected_with_meta(
        system_prompt.clone(),
        cfg.skills.skills_enabled,
        cfg.skills.skills_dir.as_str(),
        cfg.skills.skills_max_chars,
        sk.base_dir,
        sk.user_text,
        cfg.skills.skills_top_k,
    )
    .unwrap_or((
        system_prompt,
        crate::config::skills::SkillsSelectionMeta::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    #[test]
    fn compose_first_system_diagnostics_without_skills_reports_l3_l4_only() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (_system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: None,
                skills_base_dir: None,
                role_resolution: RoleSystemResolution::Lenient,
            },
        )
        .expect("compose");
        assert_eq!(
            diag.layers_applied,
            vec!["L3".to_string(), "L4".to_string()]
        );
        assert!(diag.chars_l3_base > 0);
        assert!(diag.chars_l4_augmented >= diag.chars_l3_base);
        assert_eq!(diag.skills_total_docs, 0);
        assert!(diag.skills_selected_labels.is_empty());
    }

    #[test]
    fn compose_first_system_diagnostics_with_skills_reports_l5_selection() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skills_dir = tmp.path().join(".crabmate/skills");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        let mut f = std::fs::File::create(skills_dir.join("rust.md")).expect("create skill file");
        writeln!(
            f,
            "---\nname: Rust Build Skill\n---\n使用 cargo test 与 cargo clippy 进行验证。"
        )
        .expect("write skill");

        let mut cfg = crate::config::load_config(None).expect("embed default");
        cfg.skills.skills_enabled = true;
        cfg.skills.skills_dir = ".crabmate/skills".to_string();
        cfg.skills.skills_top_k = 4;
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (_system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: Some("请帮我跑 cargo test"),
                skills_base_dir: Some(tmp.path().to_path_buf()),
                role_resolution: RoleSystemResolution::Lenient,
            },
        )
        .expect("compose");
        assert!(diag.layers_applied.contains(&"L5".to_string()));
        assert_eq!(diag.skills_total_docs, 1);
        assert_eq!(diag.skills_selected_labels.len(), 1);
        assert!(diag.skills_selected_labels[0].contains("Rust Build Skill"));
    }
}
