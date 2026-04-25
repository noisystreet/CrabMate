use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct SkillDoc {
    pub display_path: String,
    pub content: String,
    pub name: Option<String>,
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
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

fn resolve_skills_dir(base_dir: &Path, skills_dir: &str) -> Result<PathBuf, String> {
    let skills_dir = skills_dir.trim();
    if skills_dir.is_empty() {
        return Err("配置错误：skills_dir 不能为空".to_string());
    }
    let p = Path::new(skills_dir);
    let dir_path = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    };
    Ok(dir_path)
}

pub(crate) fn list_skills_from_base(
    base_dir: &Path,
    skills_dir: &str,
) -> Result<Vec<SkillDoc>, String> {
    let dir_path = resolve_skills_dir(base_dir, skills_dir)?;
    if dir_path.exists() && !dir_path.is_dir() {
        return Err(format!(
            "配置错误：skills_dir \"{}\" 不是目录",
            dir_path.display()
        ));
    }
    if !dir_path.is_dir() {
        return Ok(Vec::new());
    }

    let mut skill_files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir_path)
        .map_err(|e| format!("无法读取 skills_dir \"{}\": {}", dir_path.display(), e))?
    {
        let Ok(entry) = entry else {
            continue;
        };
        let child = entry.path();
        if child.is_file() && is_markdown_file(&child) {
            skill_files.push(child);
        }
    }
    skill_files.sort();

    let mut out: Vec<SkillDoc> = Vec::new();
    for path in skill_files {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("无法读取技能文件 \"{}\": {}", path.display(), e))?;
        if content.trim().is_empty() {
            continue;
        }
        let display_path = path
            .strip_prefix(base_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        let name = parse_skill_name_from_frontmatter(&content);
        out.push(SkillDoc {
            display_path,
            content,
            name,
        });
    }
    Ok(out)
}

fn render_skills_appendix(docs: &[SkillDoc], max_chars: usize) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut body = String::from(
        "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n",
    );
    for d in docs {
        body.push_str("\n\n---\n");
        body.push_str(&format!("技能文件: {}\n\n", d.display_path));
        body.push_str(d.content.trim());
    }
    if body.chars().count() <= max_chars {
        return body;
    }
    let mut truncated = super::super::text_util::truncate_str_to_max_chars(&body, max_chars);
    truncated.push_str(
        "\n\n[提示] 技能内容已按 skills_max_chars 截断。后续不得假定未出现在本 system 中的技能条文。",
    );
    truncated
}

fn extract_query_terms(user_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in user_text.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.chars().count() >= 2 {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && cur.chars().count() >= 2 {
        out.push(cur);
    }
    out.sort();
    out.dedup();
    out
}

fn score_skill_doc(doc: &SkillDoc, terms: &[String]) -> usize {
    if terms.is_empty() {
        return 0;
    }
    let mut score = 0usize;
    let path_l = doc.display_path.to_ascii_lowercase();
    let name_l = doc.name.clone().unwrap_or_default().to_ascii_lowercase();
    let content_head_l = doc
        .content
        .chars()
        .take(800)
        .collect::<String>()
        .to_ascii_lowercase();
    for t in terms {
        if path_l.contains(t) {
            score += 4;
        }
        if !name_l.is_empty() && name_l.contains(t) {
            score += 5;
        }
        if content_head_l.contains(t) {
            score += 1;
        }
    }
    score
}

pub(crate) fn select_skills_top_k(
    docs: &[SkillDoc],
    user_text: &str,
    top_k: usize,
) -> Vec<SkillDoc> {
    if docs.is_empty() || top_k == 0 {
        return Vec::new();
    }
    let terms = extract_query_terms(user_text);
    let mut scored: Vec<(usize, &SkillDoc)> = docs
        .iter()
        .map(|d| (score_skill_doc(d, &terms), d))
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.display_path.cmp(&b.1.display_path))
    });
    let any_positive = scored.iter().any(|(s, _)| *s > 0);
    if any_positive {
        scored
            .into_iter()
            .filter(|(s, _)| *s > 0)
            .take(top_k)
            .map(|(_, d)| d.clone())
            .collect::<Vec<_>>()
    } else {
        docs.iter().take(top_k).cloned().collect::<Vec<_>>()
    }
}

pub(crate) fn merge_system_prompt_with_skills_selected(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
    base_dir: &Path,
    user_text: &str,
    top_k: usize,
) -> Result<String, String> {
    if !skills_enabled {
        return Ok(system_prompt);
    }
    let docs = list_skills_from_base(base_dir, skills_dir)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let selected = select_skills_top_k(&docs, user_text, top_k);
    if selected.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_skills_appendix(&selected, skills_max_chars);
    if appendix.is_empty() {
        return Ok(system_prompt);
    }
    Ok(format!("{}\n\n{}", system_prompt.trim_end(), appendix))
}

pub(super) fn merge_system_prompt_with_skills(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    merge_system_prompt_with_skills_selected(
        system_prompt,
        skills_enabled,
        skills_dir,
        skills_max_chars,
        &cwd,
        "",
        usize::MAX,
    )
}
