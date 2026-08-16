use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cm_config::types::SkillsConfigSection;

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

/// 三层 skills 扫描选项：系统 → 用户 → 工作区；同 id 后者覆盖前者。
#[derive(Debug, Clone, Copy)]
pub struct SkillsListOpts<'a> {
    pub workspace_base_dir: &'a Path,
    pub skills_dir: &'a str,
    /// 空串表示关闭该层。
    pub skills_user_dir: &'a str,
    /// 空串表示关闭该层。
    pub skills_system_dir: &'a str,
}

impl SkillsConfigSection {
    /// 从已 finalize 的配置构造列表选项（相对 `skills_dir` 相对工作区根解析）。
    #[must_use]
    pub fn list_opts<'a>(&'a self, workspace_base_dir: &'a Path) -> SkillsListOpts<'a> {
        SkillsListOpts {
            workspace_base_dir,
            skills_dir: self.skills_dir.as_str(),
            skills_user_dir: self.skills_user_dir.as_str(),
            skills_system_dir: self.skills_system_dir.as_str(),
        }
    }
}

/// Options for [`merge_system_prompt_with_skills_selected_with_meta`].
#[derive(Debug, Clone, Copy)]
pub struct SkillsSelectedMergeOpts<'a> {
    pub skills_enabled: bool,
    pub skills_dir: &'a str,
    pub skills_user_dir: &'a str,
    pub skills_system_dir: &'a str,
    pub skills_max_chars: usize,
    pub base_dir: &'a Path,
    pub user_text: &'a str,
    pub top_k: usize,
    pub forced_skill: Option<&'a SkillDoc>,
}

impl SkillsSelectedMergeOpts<'_> {
    fn list_opts(&self) -> SkillsListOpts<'_> {
        SkillsListOpts {
            workspace_base_dir: self.base_dir,
            skills_dir: self.skills_dir,
            skills_user_dir: self.skills_user_dir,
            skills_system_dir: self.skills_system_dir,
        }
    }
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

/// 用户/系统层：空串关闭；相对路径相对 `workspace_base_dir`（通常应写绝对路径）。
fn resolve_optional_layer_dir(
    base_dir: &Path,
    configured: &str,
) -> Result<Option<PathBuf>, String> {
    let configured = configured.trim();
    if configured.is_empty() {
        return Ok(None);
    }
    Ok(Some(resolve_skills_dir(base_dir, configured)?))
}

/// Finalize：省略 → 约定路径；空串 / `-` / `none` → 关闭该层。
pub(crate) fn resolve_skills_layer_dir_setting(
    configured: Option<&String>,
    default_path: impl FnOnce() -> PathBuf,
) -> String {
    match configured.map(|s| s.trim()) {
        None => default_path().to_string_lossy().into_owned(),
        Some(t) if t.is_empty() || t == "-" || t.eq_ignore_ascii_case("none") => String::new(),
        Some(t) => t.to_string(),
    }
}

/// 默认用户级 skills 目录（`$XDG_CONFIG_HOME/crabmate/skills`）。
///
/// 与「源码树是否自动加载 XDG `config.toml`」解耦：skills 是跨工作区附加能力，
/// 目录不存在即为空；测试/CI 需要隔离时用 **`CM_SKILLS_USER_DIR=-`**（或空串）。
pub(crate) fn default_skills_user_dir() -> PathBuf {
    crate::cm_config::user_config_xdg::user_config_dir().join("skills")
}

/// 默认系统级 skills 目录（`/etc/crabmate/skills`）。
///
/// 目录不存在即为空；测试/CI 需要隔离时用 **`CM_SKILLS_SYSTEM_DIR=-`**（或空串）。
pub(crate) fn default_skills_system_dir() -> PathBuf {
    PathBuf::from(crate::cm_config::user_config_xdg::SYSTEM_CONFIG_DIR).join("skills")
}

fn skill_merge_key(doc: &SkillDoc) -> String {
    if let Some(n) = doc.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return n.to_ascii_lowercase();
    }
    skill_path_stem(&doc.display_path)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| doc.display_path.to_ascii_lowercase())
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

fn list_skills_in_resolved_dir(
    dir_path: &Path,
    display_strip_base: &Path,
) -> Result<Vec<SkillDoc>, String> {
    if dir_path.exists() && !dir_path.is_dir() {
        return Err(format!(
            "配置错误：skills_dir \"{}\" 不是目录",
            dir_path.display()
        ));
    }
    if !dir_path.is_dir() {
        return Ok(Vec::new());
    }
    let skill_files = collect_skill_file_paths(dir_path)?;
    let mut out: Vec<SkillDoc> = Vec::new();
    for path in skill_files {
        if let Some(doc) = skill_doc_from_path(display_strip_base, &path)? {
            out.push(doc);
        }
    }
    Ok(out)
}

/// 仅扫描工作区层（测试与兼容入口）；不含用户/系统层。
pub fn list_skills_from_base(base_dir: &Path, skills_dir: &str) -> Result<Vec<SkillDoc>, String> {
    list_skills(SkillsListOpts {
        workspace_base_dir: base_dir,
        skills_dir,
        skills_user_dir: "",
        skills_system_dir: "",
    })
}

/// 合并系统 / 用户 / 工作区三层 skills；同 callable id **工作区 > 用户 > 系统**。
///
/// - **跨层**：较高优先级层中出现的 merge key 会移除较低层的全部同名条目。
/// - **同层**：保留全部条目（同 id 仍可由 `/id` 解析为歧义，与单层行为一致）。
/// - **单层 IO 失败**：跳过该层并 `warn`，不拖垮其余层（工作区层失败仍返回 Err）。
pub fn list_skills(opts: SkillsListOpts<'_>) -> Result<Vec<SkillDoc>, String> {
    let mut out: Vec<SkillDoc> = Vec::new();

    if let Some(system_dir) =
        resolve_optional_layer_dir(opts.workspace_base_dir, opts.skills_system_dir)?
    {
        let layer = list_skills_layer_lenient(&system_dir, &system_dir, "system");
        merge_skills_layer(&mut out, layer);
    }
    if let Some(user_dir) =
        resolve_optional_layer_dir(opts.workspace_base_dir, opts.skills_user_dir)?
    {
        let layer = list_skills_layer_lenient(&user_dir, &user_dir, "user");
        merge_skills_layer(&mut out, layer);
    }

    let workspace_dir = resolve_skills_dir(opts.workspace_base_dir, opts.skills_dir)?;
    // 工作区层失败仍上抛：相对路径配置错误应可见。
    let workspace_layer = list_skills_in_resolved_dir(&workspace_dir, opts.workspace_base_dir)?;
    merge_skills_layer(&mut out, workspace_layer);

    out.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(out)
}

fn list_skills_layer_lenient(
    dir_path: &Path,
    display_strip_base: &Path,
    layer_label: &str,
) -> Vec<SkillDoc> {
    match list_skills_in_resolved_dir(dir_path, display_strip_base) {
        Ok(docs) => docs,
        Err(e) => {
            log::warn!(
                "skills {layer_label} layer skipped ({}): {e}",
                dir_path.display()
            );
            Vec::new()
        }
    }
}

/// 用 `layer` 覆盖 `out` 中相同 merge key 的条目；同层多份同 key 全部保留。
fn merge_skills_layer(out: &mut Vec<SkillDoc>, layer: Vec<SkillDoc>) {
    if layer.is_empty() {
        return;
    }
    let keys: HashSet<String> = layer.iter().map(skill_merge_key).collect();
    out.retain(|d| !keys.contains(&skill_merge_key(d)));
    out.extend(layer);
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
        crate::cm_config::skills_slash::skill_callable_id(doc)
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
    let mut truncated = crate::cm_config::text_util::truncate_str_to_max_chars(&body, max_chars);
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
    let mut truncated = crate::cm_config::text_util::truncate_str_to_max_chars(&body, max_chars);
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
                crate::cm_config::skills_slash::skill_callable_id(doc),
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
    let docs = list_skills(opts.list_opts())?;
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
    list_opts: SkillsListOpts<'_>,
    skills_enabled: bool,
    skills_max_chars: usize,
    max_entries: usize,
) -> Result<String, String> {
    if !skills_enabled {
        return Ok(system_prompt);
    }
    let docs = list_skills(list_opts)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_skills_index_appendix(&docs, skills_max_chars, max_entries);
    if appendix.is_empty() {
        return Ok(system_prompt);
    }
    Ok(format!("{}\n\n{}", system_prompt.trim_end(), appendix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_skill(dir: &Path, file: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let mut f = std::fs::File::create(dir.join(file)).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn layered_merge_workspace_overrides_user_and_system() {
        let tmp = tempfile::tempdir().unwrap();
        let system = tmp.path().join("system");
        let user = tmp.path().join("user");
        let workspace = tmp.path().join("ws");
        let ws_skills = workspace.join(".crabmate/skills");
        write_skill(
            &system,
            "shared.md",
            "---\nname: shared\n---\nfrom system\n",
        );
        write_skill(&system, "sys-only.md", "---\nname: sys-only\n---\nsys\n");
        write_skill(&user, "shared.md", "---\nname: shared\n---\nfrom user\n");
        write_skill(&user, "user-only.md", "---\nname: user-only\n---\nuser\n");
        write_skill(
            &ws_skills,
            "shared.md",
            "---\nname: shared\n---\nfrom workspace\n",
        );

        let docs = list_skills(SkillsListOpts {
            workspace_base_dir: &workspace,
            skills_dir: ".crabmate/skills",
            skills_user_dir: user.to_str().unwrap(),
            skills_system_dir: system.to_str().unwrap(),
        })
        .unwrap();
        let shared: Vec<_> = docs
            .iter()
            .filter(|d| d.name.as_deref() == Some("shared"))
            .collect();
        assert_eq!(shared.len(), 1);
        assert!(shared[0].content.contains("from workspace"));
        assert!(
            docs.iter()
                .any(|d| d.name.as_deref() == Some("user-only") && d.content.contains("user"))
        );
        assert!(
            docs.iter()
                .any(|d| d.name.as_deref() == Some("sys-only") && d.content.contains("sys"))
        );
        assert_eq!(docs.len(), 3);
    }

    #[test]
    fn same_layer_duplicate_ids_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let ws_skills = tmp.path().join(".crabmate/skills");
        write_skill(&ws_skills, "a.md", "---\nname: dup\n---\nfrom a\n");
        write_skill(&ws_skills, "b.md", "---\nname: dup\n---\nfrom b\n");
        let docs = list_skills(SkillsListOpts {
            workspace_base_dir: tmp.path(),
            skills_dir: ".crabmate/skills",
            skills_user_dir: "",
            skills_system_dir: "",
        })
        .unwrap();
        assert_eq!(docs.len(), 2);
        let err = crate::cm_config::skills_slash::resolve_skill_by_id(
            SkillsListOpts {
                workspace_base_dir: tmp.path(),
                skills_dir: ".crabmate/skills",
                skills_user_dir: "",
                skills_system_dir: "",
            },
            "dup",
        )
        .unwrap_err();
        assert!(err.to_string().contains("匹配多条"));
    }

    #[test]
    fn broken_system_layer_does_not_block_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let system_file = tmp.path().join("system-not-a-dir");
        std::fs::write(&system_file, b"x").unwrap();
        let ws_skills = tmp.path().join(".crabmate/skills");
        write_skill(&ws_skills, "ok.md", "---\nname: ok\n---\nbody\n");
        let docs = list_skills(SkillsListOpts {
            workspace_base_dir: tmp.path(),
            skills_dir: ".crabmate/skills",
            skills_user_dir: "",
            skills_system_dir: system_file.to_str().unwrap(),
        })
        .unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name.as_deref(), Some("ok"));
    }
}
