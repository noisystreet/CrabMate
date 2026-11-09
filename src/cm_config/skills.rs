use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cm_config::skills_frontmatter::parse_skill_frontmatter;
use crate::cm_config::types::SkillsConfigSection;

#[derive(Debug, Clone)]
pub struct SkillDoc {
    pub display_path: String,
    pub content: String,
    pub name: Option<String>,
    /// Frontmatter `description:`（YAML 块标量已折叠为单行文本）；缺省时 UI 可用正文首行作回退。
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

/// Options for [`merge_system_prompt_with_skills_selected_with_meta`]：只承载 `/<id>` 强制注入所需入参。
///
/// 旧的按轮打分预注入（`top_k` / `user_text` / 三层目录扫描）已删除（#1326）：正文只有两条来源——
/// `/<id>` 强制注入（本结构）与 `skill_read` 按需读取。
#[derive(Debug, Clone, Copy)]
pub struct SkillsSelectedMergeOpts<'a> {
    pub skills_enabled: bool,
    pub skills_max_chars: usize,
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

pub(crate) fn skill_merge_key(doc: &SkillDoc) -> String {
    if let Some(n) = doc.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return n.to_ascii_lowercase();
    }
    skill_path_stem(&doc.display_path)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| doc.display_path.to_ascii_lowercase())
}

/// 技能所在层（`skill_manage` 等管理场景的输出与写入定位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLayer {
    System,
    User,
    Workspace,
}

impl SkillLayer {
    /// 层标签（小写，与配置文档用语一致）。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Workspace => "workspace",
        }
    }
}

/// 带层标签与绝对路径的技能条目：管理工具需要看到**全部物理文件**，不做跨层去重。
#[derive(Debug, Clone)]
pub struct SkillManagedDoc {
    pub layer: SkillLayer,
    pub abs_path: PathBuf,
    pub doc: SkillDoc,
}

impl SkillManagedDoc {
    /// 合并键（同 id 覆盖判定）：frontmatter `name` 小写，否则路径 stem 小写。
    #[must_use]
    pub fn merge_key(&self) -> String {
        skill_merge_key(&self.doc)
    }
}

/// 解析单个技能层目录（供写入 / 管理定位）：空串 = 该层未启用 → `Err`；相对路径相对 `base_dir`。
pub fn resolve_skill_layer_dir(
    base_dir: &Path,
    configured: &str,
    layer: SkillLayer,
) -> Result<PathBuf, String> {
    resolve_optional_layer_dir(base_dir, configured)?
        .ok_or_else(|| format!("{} 层技能目录未启用（配置为空串）", layer.label()))
}

/// 按层（系统 → 用户 → 工作区）扫描全部技能文件；**不做**跨层 / 同层去重。
///
/// 单层 IO 失败：系统 / 用户层跳过并 `warn`；工作区层返回 `Err`（与 [`list_skills`] 一致）。
pub fn list_skills_by_layer(opts: SkillsListOpts<'_>) -> Result<Vec<SkillManagedDoc>, String> {
    let mut out: Vec<SkillManagedDoc> = Vec::new();
    if let Some(system_dir) =
        resolve_optional_layer_dir(opts.workspace_base_dir, opts.skills_system_dir)?
    {
        for doc in list_skills_layer_lenient(&system_dir, &system_dir, "system") {
            out.push(SkillManagedDoc {
                layer: SkillLayer::System,
                abs_path: system_dir.join(&doc.display_path),
                doc,
            });
        }
    }
    if let Some(user_dir) = resolve_optional_layer_dir(opts.workspace_base_dir, opts.skills_user_dir)?
    {
        for doc in list_skills_layer_lenient(&user_dir, &user_dir, "user") {
            out.push(SkillManagedDoc {
                layer: SkillLayer::User,
                abs_path: user_dir.join(&doc.display_path),
                doc,
            });
        }
    }
    let workspace_dir = resolve_skills_dir(opts.workspace_base_dir, opts.skills_dir)?;
    // 工作区层失败仍上抛：相对路径配置错误应可见。
    for doc in list_skills_in_resolved_dir(&workspace_dir, opts.workspace_base_dir)? {
        // display_path 相对工作区根（skills_dir 为绝对路径且在根外时即为绝对路径，join 语义仍正确）。
        out.push(SkillManagedDoc {
            layer: SkillLayer::Workspace,
            abs_path: opts.workspace_base_dir.join(&doc.display_path),
            doc,
        });
    }
    Ok(out)
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

/// 索引即「发现面」：列出**全部**技能（`id — description`），正文不进 `system`，由 `skill_read` 按需读取。
///
/// 条目数**不设上限**：旧版既把条目数绑在 `skills_top_k` 上、又按轮注入正文，导致大部分技能对模型不可见、
/// 首条 `system` 逐轮变化（#1326）；那套打分预注入已删除。预算只由 `skills_max_chars` 约束。
fn render_skills_index_appendix(docs: &[SkillDoc], max_chars: usize) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut body = format!(
        "【项目技能索引（skills）】\n当前共 {} 条技能。需要某条技能的条文时，用 `skill_read` 按 `id` 按需读取；正文不在本 system 中。\n",
        docs.len()
    );
    for d in docs {
        body.push_str("\n- ");
        body.push_str(&crate::cm_config::skills_slash::skill_callable_id(d));
        let description = skill_ui_description(d);
        if !description.trim().is_empty() {
            body.push_str(" — ");
            body.push_str(description.trim());
        }
        body.push_str(" (`");
        body.push_str(&d.display_path);
        body.push_str("`)");
    }
    body.push_str(
        "\n\n[提示] 索引只给名称与描述；具体条文以 `skill_read` 读到的正文为准，不得假定未读取的技能条文已进入上下文。",
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
    // 无 `/<id>` 强制注入时什么都不做：正文只来自 `skill_read`（按需）或 `/<id>`（显式）。
    // 旧的按轮打分预注入已删除（#1326）——这里既不扫描目录也不改 `system`，首条消息因此逐轮稳定。
    Ok((system_prompt, SkillsSelectionMeta::default()))
}

pub(crate) fn merge_system_prompt_with_skills_index(
    system_prompt: String,
    list_opts: SkillsListOpts<'_>,
    skills_enabled: bool,
    skills_max_chars: usize,
) -> Result<String, String> {
    if !skills_enabled {
        return Ok(system_prompt);
    }
    let docs = list_skills(list_opts)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_skills_index_appendix(&docs, skills_max_chars);
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

    /// 真实技能文件的常见写法：`description: >-` + 缩进多行（YAML 折叠块标量）。
    const BLOCK_DESC_SKILL: &str = "---\nname: api-design\ndescription: >-\n  Design and review APIs,\n  including REST/HTTP contracts.\n---\n# API Design\n\nBody.\n";
    #[test]
    fn block_scalar_description_survives_the_layer_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(&skills.join("api-design"), "SKILL.md", BLOCK_DESC_SKILL);
        let docs = list_skills(SkillsListOpts {
            workspace_base_dir: tmp.path(),
            skills_dir: ".crabmate/skills",
            skills_user_dir: "",
            skills_system_dir: "",
        })
        .unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs[0].description.as_deref(),
            Some("Design and review APIs, including REST/HTTP contracts.")
        );
    }

    #[test]
    fn index_lists_every_skill_with_description() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        for (name, desc) in [("alpha", "Alpha desc"), ("beta", "Beta desc"), ("gamma", "Gamma desc")] {
            write_skill(
                &skills,
                &format!("{name}.md"),
                &format!("---\nname: {name}\ndescription: {desc}\n---\n{name} body\n"),
            );
        }
        let docs = list_skills(SkillsListOpts {
            workspace_base_dir: tmp.path(),
            skills_dir: ".crabmate/skills",
            skills_user_dir: "",
            skills_system_dir: "",
        })
        .unwrap();
        assert_eq!(docs.len(), 3);
        let idx = render_skills_index_appendix(&docs, 32_000);
        assert!(idx.contains("当前共 3 条技能"), "应报全量条数：{idx}");
        for (name, desc) in [("alpha", "Alpha desc"), ("beta", "Beta desc"), ("gamma", "Gamma desc")] {
            assert!(idx.contains(name), "索引应含 {name}：{idx}");
            assert!(idx.contains(desc), "索引应含 {name} 的描述：{idx}");
        }
        assert!(idx.contains("skill_read"), "索引应提示用 skill_read 取正文");
    }

    /// 没有 `/<id>` 强制注入时既不追加正文、也不扫描目录——首条 `system` 因此逐轮稳定。
    #[test]
    fn no_forced_skill_leaves_system_untouched() {
        let (out, meta) = merge_system_prompt_with_skills_selected_with_meta(
            "base".to_string(),
            SkillsSelectedMergeOpts {
                skills_enabled: true,
                skills_max_chars: 32_000,
                forced_skill: None,
            },
        )
        .unwrap();
        assert_eq!(out, "base", "无强制注入时不应追加正文");
        assert!(meta.selected_labels.is_empty());
        assert_eq!(meta.total_docs, 0, "无强制注入时不应扫描目录");
    }
}
