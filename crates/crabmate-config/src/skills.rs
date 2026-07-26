use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillDoc {
    pub display_path: String,
    pub content: String,
    pub name: Option<String>,
    /// Frontmatter `description:`（单行）；缺省时 UI 可用正文首行作回退。
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillsSelectionMeta {
    pub total_docs: usize,
    pub selected_labels: Vec<String>,
}

/// Options for [`merge_system_prompt_with_skills_selected_with_meta`].
#[derive(Debug, Clone, Copy)]
pub struct SkillsSelectedMergeOpts<'a> {
    pub skills_enabled: bool,
    pub skills_dir: &'a str,
    pub skills_max_chars: usize,
    pub base_dir: &'a Path,
    pub user_text: &'a str,
    pub top_k: usize,
    pub forced_skill: Option<&'a SkillDoc>,
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Cursor 同形：`<skills_dir>/<id>/SKILL.md`（文件名大小写不敏感）。
fn is_skill_md_basename(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("SKILL.md"))
}

/// 无 frontmatter `name` 时用于 `/<id>` 的 stem：平铺 `foo.md` → `foo`；`<id>/SKILL.md` → 父目录名。
pub(crate) fn skill_path_stem(display_path: &str) -> Option<String> {
    let path = Path::new(display_path);
    if is_skill_md_basename(path) {
        return path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_frontmatter_scalar(rest: &str) -> Option<String> {
    let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("name:") {
            if name.is_none() {
                name = parse_frontmatter_scalar(rest);
            }
        } else if let Some(rest) = t.strip_prefix("description:")
            && description.is_none()
        {
            description = parse_frontmatter_scalar(rest);
        }
    }
    (name, description)
}

/// UI / 补全用短描述：优先 frontmatter `description`，否则取正文首个非空行（去 `#` 标题前缀）。
pub fn skill_ui_description(doc: &SkillDoc) -> String {
    const MAX: usize = 160;
    if let Some(d) = doc
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return truncate_chars(d, MAX);
    }
    let body = strip_yaml_frontmatter(&doc.content);
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let t = t.trim_start_matches('#').trim();
        if t.is_empty() {
            continue;
        }
        return truncate_chars(t, MAX);
    }
    String::new()
}

fn strip_yaml_frontmatter(content: &str) -> &str {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return content;
    }
    let Some(first_nl) = content.find('\n') else {
        return content;
    };
    let mut offset = first_nl + 1;
    while offset <= content.len() {
        let rest = &content[offset..];
        let line_end = rest.find('\n').map(|i| offset + i).unwrap_or(content.len());
        let line = content.get(offset..line_end).unwrap_or("");
        let after = if line_end < content.len() {
            line_end + 1
        } else {
            line_end
        };
        if line.trim() == "---" {
            return content.get(after..).unwrap_or("");
        }
        if line_end == content.len() {
            break;
        }
        offset = after;
    }
    content
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
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

/// Cursor 同形：`<id>/SKILL.md`（同目录多个大小写变体时取排序后第一个）。
fn find_nested_skill_md(skill_dir: &Path) -> Option<PathBuf> {
    let Ok(sub_entries) = std::fs::read_dir(skill_dir) else {
        return None;
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for sub in sub_entries {
        let Ok(sub) = sub else {
            continue;
        };
        let p = sub.path();
        if p.is_file() && is_skill_md_basename(&p) {
            candidates.push(p);
        }
    }
    candidates.sort();
    candidates.into_iter().next()
}

fn collect_skill_file_paths(dir_path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut skill_files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir_path)
        .map_err(|e| format!("无法读取 skills_dir \"{}\": {}", dir_path.display(), e))?
    {
        let Ok(entry) = entry else {
            continue;
        };
        let child = entry.path();
        if child.is_file() && is_markdown_file(&child) {
            skill_files.push(child);
        } else if child.is_dir()
            && let Some(p) = find_nested_skill_md(&child)
        {
            skill_files.push(p);
        }
    }
    skill_files.sort();
    Ok(skill_files)
}

fn skill_doc_from_path(base_dir: &Path, path: &Path) -> Result<Option<SkillDoc>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("无法读取技能文件 \"{}\": {}", path.display(), e))?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    let display_path = path
        .strip_prefix(base_dir)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string());
    let (name, description) = parse_skill_frontmatter(&content);
    Ok(Some(SkillDoc {
        display_path,
        content,
        name,
        description,
    }))
}

pub fn list_skills_from_base(base_dir: &Path, skills_dir: &str) -> Result<Vec<SkillDoc>, String> {
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

    let skill_files = collect_skill_file_paths(&dir_path)?;
    let mut out: Vec<SkillDoc> = Vec::new();
    for path in skill_files {
        if let Some(doc) = skill_doc_from_path(base_dir, &path)? {
            out.push(doc);
        }
    }
    Ok(out)
}

fn render_skills_appendix(docs: &[SkillDoc], max_chars: usize) -> String {
    render_skills_appendix_with_title(
        docs,
        max_chars,
        "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n",
    )
}

fn render_forced_skill_appendix(doc: &SkillDoc, max_chars: usize) -> String {
    let title = format!(
        "【用户显式选用技能（/{}）】\n以下内容由用户通过斜杠命令强制注入；若与更高优先级指令冲突，以更高优先级为准。\n",
        crate::skills_slash::skill_callable_id(doc)
    );
    render_skills_appendix_with_title(std::slice::from_ref(doc), max_chars, &title)
}

fn render_skills_appendix_with_title(docs: &[SkillDoc], max_chars: usize, title: &str) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut body = String::from(title);
    for d in docs {
        body.push_str("\n\n---\n");
        body.push_str(&format!("技能文件: {}\n\n", d.display_path));
        body.push_str(d.content.trim());
    }
    if body.chars().count() <= max_chars {
        return body;
    }
    let mut truncated = crate::text_util::truncate_str_to_max_chars(&body, max_chars);
    truncated.push_str(
        "\n\n[提示] 技能内容已按 skills_max_chars 截断。后续不得假定未出现在本 system 中的技能条文。",
    );
    truncated
}

fn render_skills_index_appendix(docs: &[SkillDoc], max_chars: usize, max_entries: usize) -> String {
    if docs.is_empty() || max_entries == 0 {
        return String::new();
    }
    let listed = docs.len().min(max_entries);
    let mut body = format!(
        "【项目技能索引（skills）】\n当前检测到 {} 条技能，以下展示前 {} 条索引；详细正文在按轮（L5）按用户消息动态注入。\n",
        docs.len(),
        listed
    );
    for d in docs.iter().take(max_entries) {
        body.push('\n');
        if let Some(name) = d.name.as_deref().filter(|n| !n.trim().is_empty()) {
            body.push_str("- ");
            body.push_str(name.trim());
            body.push_str(" (`");
            body.push_str(&d.display_path);
            body.push_str("`)");
        } else {
            body.push_str("- `");
            body.push_str(&d.display_path);
            body.push('`');
        }
    }
    body.push_str(
        "\n\n[提示] 若当前任务依赖具体技能条文，以本轮动态注入内容为准，不得假定索引之外的正文已进入上下文。",
    );
    if body.chars().count() <= max_chars {
        return body;
    }
    let mut truncated = crate::text_util::truncate_str_to_max_chars(&body, max_chars);
    truncated.push_str(
        "\n\n[提示] 技能索引已按 skills_max_chars 截断。后续不得假定未出现在本 system 中的技能条目。",
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

pub fn merge_system_prompt_with_skills_selected_with_meta(
    system_prompt: String,
    opts: SkillsSelectedMergeOpts<'_>,
) -> Result<(String, SkillsSelectionMeta), String> {
    if !opts.skills_enabled {
        return Ok((system_prompt, SkillsSelectionMeta::default()));
    }
    if let Some(doc) = opts.forced_skill {
        let mut meta = SkillsSelectionMeta {
            total_docs: 1,
            selected_labels: vec![format!(
                "{} ({}) [forced]",
                crate::skills_slash::skill_callable_id(doc),
                doc.display_path
            )],
        };
        let appendix = render_forced_skill_appendix(doc, opts.skills_max_chars);
        if appendix.is_empty() {
            meta.selected_labels.clear();
            return Ok((system_prompt, meta));
        }
        return Ok((
            format!("{}\n\n{}", system_prompt.trim_end(), appendix),
            meta,
        ));
    }
    let docs = list_skills_from_base(opts.base_dir, opts.skills_dir)?;
    if docs.is_empty() {
        return Ok((system_prompt, SkillsSelectionMeta::default()));
    }
    let mut meta = SkillsSelectionMeta {
        total_docs: docs.len(),
        selected_labels: Vec::new(),
    };
    let selected = select_skills_top_k(&docs, opts.user_text, opts.top_k);
    if selected.is_empty() {
        return Ok((system_prompt, meta));
    }
    meta.selected_labels = selected
        .iter()
        .map(|d| {
            d.name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| format!("{n} ({})", d.display_path))
                .unwrap_or_else(|| d.display_path.clone())
        })
        .collect();
    let appendix = render_skills_appendix(&selected, opts.skills_max_chars);
    if appendix.is_empty() {
        return Ok((system_prompt, meta));
    }
    Ok((
        format!("{}\n\n{}", system_prompt.trim_end(), appendix),
        meta,
    ))
}

pub(crate) fn merge_system_prompt_with_skills_index(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
    base_dir: &Path,
    max_entries: usize,
) -> Result<String, String> {
    if !skills_enabled {
        return Ok(system_prompt);
    }
    let docs = list_skills_from_base(base_dir, skills_dir)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_skills_index_appendix(&docs, skills_max_chars, max_entries);
    if appendix.is_empty() {
        return Ok(system_prompt);
    }
    Ok(format!("{}\n\n{}", system_prompt.trim_end(), appendix))
}
