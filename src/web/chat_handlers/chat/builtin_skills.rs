//! Web 内置 `/skills` 命令：扫描 skills 目录并生成回复文本。

use std::sync::Arc;

use super::super::super::app_state::AppState;

pub(super) fn merge_system_prompt_with_workspace_skills_for_web(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
    skills_top_k: usize,
    workspace_root: &std::path::Path,
    user_text: &str,
) -> String {
    let base_dir = resolve_skills_base_dir(workspace_root);
    crate::config::skills::merge_system_prompt_with_skills_selected(
        system_prompt.clone(),
        skills_enabled,
        skills_dir,
        skills_max_chars,
        base_dir.as_path(),
        user_text,
        skills_top_k,
    )
    .unwrap_or(system_prompt)
}

fn resolve_skills_base_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    if workspace_root.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        workspace_root.to_path_buf()
    }
}

fn resolve_skills_dir_path(
    base_dir: &std::path::Path,
    skills_dir: &str,
) -> Result<std::path::PathBuf, String> {
    let raw = skills_dir.trim();
    if raw.is_empty() {
        return Err("skills_dir 为空".to_string());
    }
    let p = std::path::Path::new(raw);
    Ok(if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    })
}

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

#[derive(Debug, Clone)]
struct SkillFileInfo {
    display_path: String,
    content: String,
    skill_name: Option<String>,
}

fn parse_skill_name_from_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("name:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn is_markdown_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn list_skill_files_for_web_builtin(
    skills_dir: &str,
    base_dir: &std::path::Path,
) -> Result<Vec<SkillFileInfo>, String> {
    let base = resolve_skills_dir_path(base_dir, skills_dir)?;
    if !base.exists() {
        return Ok(Vec::new());
    }
    if !base.is_dir() {
        return Err(format!("skills_dir 不是目录: {}", base.display()));
    }
    let mut out: Vec<SkillFileInfo> = Vec::new();
    for entry in std::fs::read_dir(&base).map_err(|e| format!("无法读取 skills_dir: {e}"))? {
        let Ok(entry) = entry else {
            continue;
        };
        let child = entry.path();
        let skill_path = if child.is_file() && is_markdown_file(&child) {
            child
        } else {
            continue;
        };
        if !skill_path.is_file() {
            continue;
        }
        let display = skill_path
            .strip_prefix(base_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| skill_path.display().to_string());
        let content = std::fs::read_to_string(&skill_path)
            .map_err(|e| format!("读取技能文件失败 {}: {e}", skill_path.display()))?;
        out.push(SkillFileInfo {
            display_path: display,
            skill_name: parse_skill_name_from_frontmatter(&content),
            content,
        });
    }
    out.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(out)
}

fn split_loaded_skills_by_budget(
    files: &[SkillFileInfo],
    max_chars: usize,
) -> (Vec<SkillFileInfo>, Vec<SkillFileInfo>) {
    // 与 `config::skills::render_skills_appendix` 的模板保持一致。
    let mut used = "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n"
        .chars()
        .count();
    let mut loaded: Vec<SkillFileInfo> = Vec::new();
    let mut skipped: Vec<SkillFileInfo> = Vec::new();
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
    state: &Arc<AppState>,
    command: &str,
) -> Option<String> {
    match classify_web_builtin_command(command)? {
        "skills" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills_max_chars;
            let dir = cfg.skills_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());

            let text = match list_skill_files_for_web_builtin(&dir, base_dir.as_path()) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：`{dir}`\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    format!(
                        "skills 概览：共 {} 个文件，按上限预计完整加载 {} 个，未完整加载 {} 个。\n目录：`{}`\n上限：skills_max_chars={}\n\n输入 `/skills list` 查看具体文件。",
                        files.len(),
                        loaded.len(),
                        skipped.len(),
                        dir,
                        max_chars
                    )
                }
                Err(e) => format!("读取 skills 失败：{e}"),
            };
            Some(text)
        }
        "skills_list" => {
            let cfg = state.cfg.read().await;
            if !cfg.skills_enabled {
                return Some(
                    "skills 已关闭（skills_enabled=false），当前不会加载任何 skills。".to_string(),
                );
            }
            let max_chars = cfg.skills_max_chars;
            let dir = cfg.skills_dir.clone();
            drop(cfg);
            let ws = std::path::PathBuf::from(state.effective_workspace_path().await);
            let base_dir = resolve_skills_base_dir(ws.as_path());
            let text = match list_skill_files_for_web_builtin(&dir, base_dir.as_path()) {
                Ok(files) if files.is_empty() => {
                    format!(
                        "当前未发现 skills。\n目录：`{dir}`\n上限：skills_max_chars={max_chars}"
                    )
                }
                Ok(files) => {
                    let (loaded, skipped) = split_loaded_skills_by_budget(&files, max_chars);
                    let loaded_lines = if loaded.is_empty() {
                        "- （无）".to_string()
                    } else {
                        loaded
                            .iter()
                            .map(|f| {
                                let name = f.skill_name.as_deref().unwrap_or("未声明 name");
                                format!("- `{}` (name: `{}`)", f.display_path, name)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let skipped_lines = if skipped.is_empty() {
                        "- （无）".to_string()
                    } else {
                        skipped
                            .iter()
                            .map(|f| {
                                let name = f.skill_name.as_deref().unwrap_or("未声明 name");
                                format!("- `{}` (name: `{}`)", f.display_path, name)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    format!(
                        "当前已加载（完整进入 system）skills：\n{}\n\n未完整加载（受上限影响）skills：\n{}\n\n目录：`{}`\n上限：skills_max_chars={}（扫描总数：{}）",
                        loaded_lines,
                        skipped_lines,
                        dir,
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
