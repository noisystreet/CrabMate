//! Web 内置 `/skills` 命令：扫描 skills 目录并生成回复文本。

use crate::config::skills::{SkillDoc, list_skills, skill_ui_description};
use crate::config::skills_slash::skill_callable_id;
use crate::context_bootstrap::prompt_compose::resolve_skills_base_dir;
use crate::web::app_state_facets::WebChatTurnAppFacet;

fn classify_web_builtin_command(input: &str) -> Option<&'static str> {
    let s = input.trim();
    if s.eq_ignore_ascii_case("/skills") {
        return Some("skills");
    }
    if s.eq_ignore_ascii_case("/skills list") {
        return Some("skills_list");
    }
    None
}

fn format_skills_layer_dirs(workspace: &str, user: &str, system: &str) -> String {
    let mut parts = vec![format!("workspace=`{workspace}`")];
    if !user.trim().is_empty() {
        parts.push(format!("user=`{user}`"));
    }
    if !system.trim().is_empty() {
        parts.push(format!("system=`{system}`"));
    }
    parts.join(" · ")
}

fn format_skill_list_line(doc: &SkillDoc) -> String {
    let id = skill_callable_id(doc);
    let name = doc.name.as_deref().unwrap_or("未声明 name");
    let desc = skill_ui_description(doc);
    let desc = if desc.is_empty() {
        String::new()
    } else {
        format!(" — {desc}")
    };
    format!("- `/{id}` → `{}` (name: `{name}`){desc}", doc.display_path)
}

fn split_loaded_skills_by_budget(
    files: &[SkillDoc],
    max_chars: usize,
) -> (Vec<SkillDoc>, Vec<SkillDoc>) {
    // 与 `config::skills::render_skills_appendix` 的模板保持一致。
    let mut used = "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n"
        .chars()
        .count();
    let mut loaded: Vec<SkillDoc> = Vec::new();
    let mut skipped: Vec<SkillDoc> = Vec::new();
    for f in files {
        let per_file = format!(
            "\n\n---\n技能文件: {}\n\n{}",
            f.display_path,
            f.content.trim()
        );
        let need = per_file.chars().count();
        if used + need <= max_chars {
            used += need;
            loaded.push(f.clone());
        } else {
            skipped.push(f.clone());
        }
    }
    (loaded, skipped)
}

pub(super) async fn run_web_builtin_command(
    state: &WebChatTurnAppFacet,
    command: &str,
) -> Option<String> {
    match classify_web_builtin_command(command)? {
        "skills" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills.skills_max_chars;
            let dir = cfg.skills.skills_dir.clone();
            let user_dir = cfg.skills.skills_user_dir.clone();
            let system_dir = cfg.skills.skills_system_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());
            let opts = crate::config::skills::SkillsListOpts {
                workspace_base_dir: base_dir.as_path(),
                skills_dir: dir.as_str(),
                skills_user_dir: user_dir.as_str(),
                skills_system_dir: system_dir.as_str(),
            };
            let layers = format_skills_layer_dirs(&dir, &user_dir, &system_dir);

            let text = match list_skills(opts) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：{layers}\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    format!(
                        "skills 概览：共 {} 个文件，按上限预计完整加载 {} 个，未完整加载 {} 个。\n目录：{}\n上限：skills_max_chars={}\n\n输入 `/skills list` 查看可 `/<id>` 调用的技能；对话中发送 `/<id> [任务]` 可强制选用。",
                        files.len(),
                        loaded.len(),
                        skipped.len(),
                        layers,
                        max_chars
                    )
                }
                Err(e) => format!("读取 skills 失败：{e}"),
            };
            Some(text)
        }
        "skills_list" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills.skills_max_chars;
            let dir = cfg.skills.skills_dir.clone();
            let user_dir = cfg.skills.skills_user_dir.clone();
            let system_dir = cfg.skills.skills_system_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());
            let opts = crate::config::skills::SkillsListOpts {
                workspace_base_dir: base_dir.as_path(),
                skills_dir: dir.as_str(),
                skills_user_dir: user_dir.as_str(),
                skills_system_dir: system_dir.as_str(),
            };
            let layers = format_skills_layer_dirs(&dir, &user_dir, &system_dir);
            let text = match list_skills(opts) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：{layers}\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    let loaded_lines = if loaded.is_empty() {
                        "- （无）".to_string()
                    } else {
                        loaded
                            .iter()
                            .map(format_skill_list_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let skipped_lines = if skipped.is_empty() {
                        "- （无）".to_string()
                    } else {
                        skipped
                            .iter()
                            .map(format_skill_list_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    format!(
                        "当前已加载（完整进入 system）skills：\n{}\n\n未完整加载（受上限影响）skills：\n{}\n\n对话中可用 `/<id> [任务]` 强制选用某一技能（跳过 Top-K）。\n目录：{}\n上限：skills_max_chars={}（扫描总数：{}）",
                        loaded_lines,
                        skipped_lines,
                        layers,
                        max_chars,
                        files.len()
                    )
                }
                Err(e) => format!("读取 skills 失败：{e}"),
            };
            Some(text)
        }
        _ => None,
    }
}
