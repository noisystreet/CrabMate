//! 首条 `system` 动态组装（L3 角色基底 + AGENTS.md 项目规则 + L4 运行时附录 + L5 按轮 Skills）。
//!
//! Web **`build_messages_for_turn`**、CLI REPL/`chat`、会话恢复与角色切换应共用本模块语义。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;

use crate::cm_internal::tool_stats::ToolOutcomeRecorder;
use crate::cm_config::AgentConfig;
use crate::cm_types::SessionMode;

use super::agents_md;

/// 保留占位：组装路径恒为 Strict；角色容错见 [`resolve_agent_role_for_prompt_compose`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSystemResolution {
    Strict,
}

fn append_session_mode_appendix(system: String, mode: SessionMode) -> String {
    let body = crate::cm_config::embedded_session_mode_appendix(mode);
    if body.is_empty() {
        return system;
    }
    format!("{}\n\n{}", system.trim_end(), body)
}

fn resolve_role_system_base(cfg: &AgentConfig, agent_role: Option<&str>) -> Result<String, String> {
    let base = cfg
        .system_prompt_for_new_conversation(agent_role)
        .map(|s| s.to_string())
        .map_err(|e| e.to_string())?;
    let locale = crate::cm_internal::user_data::load_prefs().locale;
    Ok(append_locale_output_directive(base, locale.as_deref()))
}

/// 环境/界面 locale（BCP-47，如 `zh-Hans` / `en-US`）→ 模型可读的语言名；未知值原样回传。
fn locale_output_language_label(locale: &str) -> Option<String> {
    let t = locale.trim();
    if t.is_empty() {
        return None;
    }
    let primary = t
        .split(['-', '_'])
        .next()
        .unwrap_or(t)
        .to_ascii_lowercase();
    let label = match primary.as_str() {
        "zh" => {
            let lower = t.to_ascii_lowercase();
            if lower.contains("hant")
                || lower.contains("tw")
                || lower.contains("hk")
                || lower.contains("mo")
            {
                "繁體中文"
            } else {
                "简体中文"
            }
        }
        "en" => "English",
        "ja" => "日本語",
        "ko" => "한국어",
        "fr" => "Français",
        "de" => "Deutsch",
        "es" => "Español",
        "ru" => "Русский",
        "pt" => "Português",
        "it" => "Italiano",
        _ => return Some(t.to_string()),
    };
    Some(label.to_string())
}

/// 按环境/界面语言追加面向用户输出语言指令；`locale` 为空时不追加（退回 base 原行为）。
fn append_locale_output_directive(system: String, locale: Option<&str>) -> String {
    let Some(label) = locale.and_then(locale_output_language_label) else {
        return system;
    };
    format!(
        "{}\n\n## 输出语言\n\n当前界面/环境语言为「{label}」。面向用户的说明与回答默认使用该语言；若用户消息明确使用其它语言，以其为准。代码、命令、标识符与专有名词保留原文。",
        system.trim_end()
    )
}

/// 解析本回合用于组装的命名 `agent_role`。
///
/// - **显式请求**（`request_role` 非空）：未知 id → `Err`（Web/CLI/REPL `/agent set` 与三端对齐）。
/// - **仅持久化**（`request_role` 为空、`persisted_role` 非空）：配置已删除该 id 时 `warn` 并退回 `None`（默认人格）。
pub fn resolve_agent_role_for_prompt_compose(
    cfg: &AgentConfig,
    request_role: Option<&str>,
    persisted_role: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(id) = request_role.map(str::trim).filter(|s| !s.is_empty()) {
        cfg.system_prompt_for_new_conversation(Some(id))
            .map_err(|e| e.to_string())?;
        return Ok(Some(id.to_string()));
    }
    if let Some(id) = persisted_role.map(str::trim).filter(|s| !s.is_empty()) {
        if cfg.system_prompt_for_new_conversation(Some(id)).is_ok() {
            return Ok(Some(id.to_string()));
        }
        warn!(
            target: "crabmate",
            agent_role = %id,
            "persisted agent_role no longer in config; using default persona for prompt compose"
        );
        return Ok(None);
    }
    Ok(None)
}

/// L5：按当前用户输入从工作区 skills 目录选材时的上下文。
pub struct SkillsComposeContext<'a> {
    pub base_dir: &'a Path,
    pub user_text: &'a str,
    /// 用户通过 `/<id>` 显式选用的 skill；若设置则跳过 Top-K。
    pub forced_skill: Option<crate::cm_config::skills::SkillDoc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirstSystemComposeDiagnostics {
    pub layers_applied: Vec<String>,
    /// L3 基底字符数**含**已注入的 `AGENTS.md` 段（`layers_applied` 含 `"AGENTS"` 时），
    /// 即「L3 + 项目规则」合并后的长度；命名沿用以兼容既有日志解析。
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
    _role_resolution: RoleSystemResolution,
) -> Result<String, String> {
    let base = resolve_role_system_base(cfg, agent_role)?;
    Ok(compose_system_from_base(&base, cfg, tool_recorder, skills))
}

/// 首条 `system` 组装参数（L3 基底 + L4 + 可选 L5 + 会话模式附录）。`agent_role` 须经 [`resolve_agent_role_for_prompt_compose`] 解析后再传入。
pub struct FirstSystemComposeOpts<'a> {
    pub agent_role: Option<&'a str>,
    pub user_msg_for_skills: Option<&'a str>,
    pub skills_base_dir: Option<PathBuf>,
    /// 会话工作区根：用于读取 `<root>/AGENTS.md` 并追加到 L3 之后；`None` 时跳过。
    pub workspace_root: Option<PathBuf>,
    /// 用户 `/<id>` 强制选用的 skill（与 [`SkillsComposeContext::forced_skill`] 同源）。
    pub forced_skill: Option<crate::cm_config::skills::SkillDoc>,
    /// 保留字段；实现恒按 [`RoleSystemResolution::Strict`] 解析已传入的 `agent_role`。
    pub role_resolution: RoleSystemResolution,
    /// 会话工作模式附录；`None` 时用配置 `default_session_mode`。
    pub session_mode: Option<SessionMode>,
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
    debug_assert_eq!(
        opts.role_resolution,
        RoleSystemResolution::Strict,
        "pass pre-resolved agent_role via resolve_agent_role_for_prompt_compose"
    );
    let FirstSystemComposeOpts {
        agent_role,
        user_msg_for_skills,
        skills_base_dir,
        workspace_root,
        forced_skill,
        role_resolution: _,
        session_mode,
    } = opts;
    let skills_ctx = skills_base_dir
        .as_ref()
        .zip(user_msg_for_skills)
        .map(|(base, user)| SkillsComposeContext {
            base_dir: base.as_path(),
            user_text: user,
            forced_skill,
        });
    let base = resolve_role_system_base(cfg, agent_role)?;
    // 项目规则（AGENTS.md）按**会话工作区根**读取，紧接 L3 基底之后。
    // 注意：Web 续聊每轮都会重算首条 system，因此 AGENTS.md 的改动会在下一轮生效，
    // 同时使该会话的 system 前缀缓存失效（规则实时性优先于缓存命中率）。
    let (base, has_agents_md) = match workspace_root.as_deref().and_then(|root| {
        agents_md::load_agents_md_block(
            root,
            cfg.agents_md.agents_md_enabled,
            cfg.agents_md.agents_md_max_chars,
        )
    }) {
        Some(block) => (format!("{}\n\n{}", base.trim_end(), block), true),
        None => (base, false),
    };
    let augmented = tool_recorder.augment_system_prompt(&base, cfg);
    let chars_l4_augmented = augmented.chars().count();
    let (merged, skills_meta) = merge_skills_into_system_with_meta(augmented, cfg, skills_ctx);
    let mode = session_mode.unwrap_or(cfg.roles_prompts.default_session_mode);
    let merged = append_session_mode_appendix(merged, mode);
    let chars_final = merged.chars().count();
    let mut layers = vec!["L3".to_string()];
    if has_agents_md {
        layers.push("AGENTS".to_string());
    }
    layers.push("L4".to_string());
    if !skills_meta.selected_labels.is_empty() {
        layers.push("L5".to_string());
    }
    layers.push("mode".to_string());
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
) -> (String, crate::cm_config::skills::SkillsSelectionMeta) {
    let Some(sk) = skills else {
        return (
            system_prompt,
            crate::cm_config::skills::SkillsSelectionMeta::default(),
        );
    };
    crate::cm_config::skills::merge_system_prompt_with_skills_selected_with_meta(
        system_prompt.clone(),
        crate::cm_config::skills::SkillsSelectedMergeOpts {
            skills_enabled: cfg.skills.skills_enabled,
            skills_dir: cfg.skills.skills_dir.as_str(),
            skills_user_dir: cfg.skills.skills_user_dir.as_str(),
            skills_system_dir: cfg.skills.skills_system_dir.as_str(),
            skills_max_chars: cfg.skills.skills_max_chars,
            base_dir: sk.base_dir,
            user_text: sk.user_text,
            top_k: cfg.skills.skills_top_k,
            forced_skill: sk.forced_skill.as_ref(),
        },
    )
    .unwrap_or((
        system_prompt,
        crate::cm_config::skills::SkillsSelectionMeta::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn embed_default_config() -> crate::cm_config::AgentConfig {
        crate::cm_config::load_config_test_env::without_cm_env_overrides(|| {
            crate::cm_config::load_config(None).expect("embed default")
        })
    }

    #[test]
    fn resolve_persisted_unknown_role_falls_back_to_default() {
        let cfg = embed_default_config();
        let role =
            resolve_agent_role_for_prompt_compose(&cfg, None, Some("nonexistent_role_id_xyz"))
                .expect("persisted unknown should not error");
        assert_eq!(role, None);
    }

    #[test]
    fn resolve_explicit_unknown_role_errors() {
        let cfg = embed_default_config();
        let err =
            resolve_agent_role_for_prompt_compose(&cfg, Some("nonexistent_role_id_xyz"), None)
                .expect_err("explicit unknown");
        assert!(err.contains("未知的 agent_role"));
    }

    #[test]
    fn compose_lenient_unknown_role_falls_back_to_global_system() {
        let cfg = embed_default_config();
        let rec = ToolOutcomeRecorder::new();
        let role =
            resolve_agent_role_for_prompt_compose(&cfg, None, Some("nonexistent_role_id_xyz"))
                .expect("persisted");
        let out = compose_system_for_turn(
            &cfg,
            role.as_deref(),
            &rec,
            None,
            RoleSystemResolution::Strict,
        )
        .expect("compose");
        assert!(!out.trim().is_empty());
        let global = cfg.roles_prompts.system_prompt.trim();
        assert!(
            out.contains(global),
            "resolved default compose should include global system prompt"
        );
    }

    #[test]
    fn compose_strict_unknown_role_errors() {
        let cfg = embed_default_config();
        let err =
            resolve_agent_role_for_prompt_compose(&cfg, Some("nonexistent_role_id_xyz"), None)
                .expect_err("strict resolve");
        assert!(err.contains("未知的 agent_role"));
    }

    #[test]
    fn compose_from_base_without_skills_matches_augment_only() {
        let cfg = embed_default_config();
        let rec = ToolOutcomeRecorder::new();
        let base = "BASE_PROMPT_MARKER";
        let expected = rec.augment_system_prompt(base, &cfg);
        let got = compose_system_from_base(base, &cfg, &rec, None);
        assert_eq!(got, expected);
    }

    #[test]
    fn compose_first_system_diagnostics_without_skills_reports_l3_l4_only() {
        let cfg = embed_default_config();
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (_system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: None,
                skills_base_dir: None,
                workspace_root: None,
                forced_skill: None,
                role_resolution: RoleSystemResolution::Strict,
                session_mode: None,
            },
        )
        .expect("compose");
        assert_eq!(
            diag.layers_applied,
            vec!["L3".to_string(), "L4".to_string(), "mode".to_string()]
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

        let mut cfg = embed_default_config();
        cfg.skills.skills_enabled = true;
        cfg.skills.skills_dir = ".crabmate/skills".to_string();
        cfg.skills.skills_user_dir.clear();
        cfg.skills.skills_system_dir.clear();
        cfg.skills.skills_top_k = 4;
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (_system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: Some("请帮我跑 cargo test"),
                skills_base_dir: Some(tmp.path().to_path_buf()),
                workspace_root: Some(tmp.path().to_path_buf()),
                forced_skill: None,
                role_resolution: RoleSystemResolution::Strict,
                session_mode: None,
            },
        )
        .expect("compose");
        assert!(diag.layers_applied.contains(&"L5".to_string()));
        assert_eq!(diag.skills_total_docs, 1);
        assert_eq!(diag.skills_selected_labels.len(), 1);
        assert!(diag.skills_selected_labels[0].contains("Rust Build Skill"));
    }

    #[test]
    fn compose_first_system_forced_skill_skips_topk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skills_dir = tmp.path().join(".crabmate/skills");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        std::fs::write(
            skills_dir.join("a.md"),
            "---\nname: alpha\n---\nalpha body\n",
        )
        .expect("write a");
        std::fs::write(
            skills_dir.join("b.md"),
            "---\nname: beta\n---\nbeta body cargo\n",
        )
        .expect("write b");
        let forced = crate::cm_config::skills_slash::resolve_skill_by_id(
            crate::cm_config::skills::SkillsListOpts {
                workspace_base_dir: tmp.path(),
                skills_dir: ".crabmate/skills",
                skills_user_dir: "",
                skills_system_dir: "",
            },
            "alpha",
        )
        .expect("resolve");
        let mut cfg = embed_default_config();
        cfg.skills.skills_enabled = true;
        cfg.skills.skills_dir = ".crabmate/skills".to_string();
        cfg.skills.skills_user_dir.clear();
        cfg.skills.skills_system_dir.clear();
        cfg.skills.skills_top_k = 4;
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: Some("cargo"),
                skills_base_dir: Some(tmp.path().to_path_buf()),
                workspace_root: Some(tmp.path().to_path_buf()),
                forced_skill: Some(forced),
                role_resolution: RoleSystemResolution::Strict,
                session_mode: None,
            },
        )
        .expect("compose");
        assert!(system.contains("alpha body"));
        assert!(!system.contains("beta body"));
        assert!(diag.skills_selected_labels[0].contains("[forced]"));
    }

    #[test]
    fn compose_first_system_appends_agents_md_after_l3() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("AGENTS.md"), "规则：必须跑 cargo fmt\n").expect("write");
        let cfg = embed_default_config();
        assert!(cfg.agents_md.agents_md_enabled, "默认应启用 AGENTS.md");
        let rec = Arc::new(ToolOutcomeRecorder::new());
        let (system, diag) = compose_first_system_for_turn_with_diagnostics(
            &cfg,
            &rec,
            FirstSystemComposeOpts {
                agent_role: None,
                user_msg_for_skills: None,
                skills_base_dir: None,
                workspace_root: Some(tmp.path().to_path_buf()),
                forced_skill: None,
                role_resolution: RoleSystemResolution::Strict,
                session_mode: None,
            },
        )
        .expect("compose");
        assert!(system.contains("项目规则（AGENTS.md）"), "system: {system}");
        assert!(system.contains("必须跑 cargo fmt"), "system: {system}");
        assert_eq!(diag.layers_applied.first().map(String::as_str), Some("L3"));
        assert!(
            diag.layers_applied.contains(&"AGENTS".to_string()),
            "{:?}",
            diag.layers_applied
        );
    }

    #[test]
    fn locale_label_maps_common_tags() {
        assert_eq!(
            locale_output_language_label("zh-Hans").as_deref(),
            Some("简体中文")
        );
        assert_eq!(
            locale_output_language_label("zh-CN").as_deref(),
            Some("简体中文")
        );
        assert_eq!(
            locale_output_language_label("zh-Hant").as_deref(),
            Some("繁體中文")
        );
        assert_eq!(
            locale_output_language_label("zh-TW").as_deref(),
            Some("繁體中文")
        );
        assert_eq!(
            locale_output_language_label("en-US").as_deref(),
            Some("English")
        );
        assert_eq!(locale_output_language_label("ja").as_deref(), Some("日本語"));
        assert_eq!(
            locale_output_language_label("de-DE").as_deref(),
            Some("Deutsch")
        );
        assert_eq!(
            locale_output_language_label("xx-YY").as_deref(),
            Some("xx-YY")
        );
        assert_eq!(locale_output_language_label("  "), None);
    }

    #[test]
    fn locale_directive_appended_only_when_locale_present() {
        let base = "BASE".to_string();
        assert_eq!(append_locale_output_directive(base.clone(), None), "BASE");
        assert_eq!(append_locale_output_directive(base.clone(), Some("")), "BASE");
        let out = append_locale_output_directive(base, Some("en-US"));
        assert!(out.starts_with("BASE"));
        assert!(out.contains("## 输出语言"));
        assert!(out.contains("English"));
    }
}
