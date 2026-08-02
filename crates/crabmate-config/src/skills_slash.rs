//! 斜杠显式选用 skill：`/<id> [任务]`（与 Cursor `/skill-name` 同形）。

use std::fmt;
use std::path::Path;

use super::skills::{SkillDoc, SkillsListOpts, list_skills, skill_ui_description};

/// 嵌入 turn-build `String` 错误中的标记，供 Web 映射为 `SKILL_INVOKE_FAILED`。
const TURN_ERR_TAG: &str = "__crabmate_skill_slash__";

/// REPL / Web / TUI 内建斜杠命令词头（小写、无 `/`），不可当作 skill id。
pub fn is_reserved_slash_head(head: &str) -> bool {
    matches!(
        head.to_ascii_lowercase().as_str(),
        "?" | "agent"
            | "api-base"
            | "api-key"
            | "apikey"
            | "apibase"
            | "branch"
            | "cd"
            | "clear"
            | "config"
            | "conv"
            | "doctor"
            | "export"
            | "help"
            | "mcp"
            | "model"
            | "models"
            | "probe"
            | "save-session"
            | "skill"
            | "skills"
            | "tools"
            | "version"
            | "workspace"
    )
}

/// 可供 `/<id>` 调用的标识：优先 frontmatter `name`，否则路径 stem
///（平铺 `foo.md` → `foo`；`<id>/SKILL.md` → 父目录名）。
pub fn skill_callable_id(doc: &SkillDoc) -> String {
    if let Some(n) = doc.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    super::skills::skill_path_stem(&doc.display_path).unwrap_or_else(|| "skill".to_string())
}

fn skill_stem(doc: &SkillDoc) -> Option<String> {
    super::skills::skill_path_stem(&doc.display_path)
}

/// Skill `/` 解析失败（用户可见文案经 [`Display`]；Web 经 [`SkillSlashError::into_turn_err`] 打标）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSlashError {
    EmptyId,
    Disabled { head: String },
    NotFound { id: String, hints: String },
    Ambiguous { id: String, candidates: String },
    Io(String),
}

impl fmt::Display for SkillSlashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "skill id 为空"),
            Self::Disabled { head } => write!(
                f,
                "skills 已关闭（skills_enabled=false），无法调用 `/{head}`。"
            ),
            Self::NotFound { id, hints } => {
                if hints.is_empty() {
                    write!(
                        f,
                        "未找到 skill `{id}`（目录无技能文件）。可用 `/skills list` 查看。"
                    )
                } else {
                    write!(
                        f,
                        "未找到 skill `{id}`。可调用示例：{hints}。完整列表：`/skills list`。"
                    )
                }
            }
            Self::Ambiguous { id, candidates } => write!(
                f,
                "skill id `{id}` 匹配多条，请改用更明确的 name：{candidates}"
            ),
            Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl SkillSlashError {
    /// 写入 `build_messages_for_turn` 等 `Result<_, String>` 错误通道。
    #[must_use]
    pub fn into_turn_err(self) -> String {
        format!("{TURN_ERR_TAG}{self}")
    }

    /// 若为 skill slash 打标错误，返回去掉标记后的用户可见文案。
    #[must_use]
    pub fn strip_turn_err(s: &str) -> Option<&str> {
        s.strip_prefix(TURN_ERR_TAG)
    }
}

fn catalog_display_path(doc: &SkillDoc) -> String {
    let p = doc.display_path.as_str();
    let path = Path::new(p);
    if path.is_absolute() {
        path.file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "skill.md".to_string())
    } else {
        p.to_string()
    }
}

fn hints_from_docs(docs: &[SkillDoc]) -> String {
    docs.iter()
        .filter(|d| {
            let id = skill_callable_id(d);
            !is_reserved_slash_head(&id)
        })
        .take(8)
        .map(|d| format!("/{}", skill_callable_id(d)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 按 id 解析唯一 skill。
///
/// 优先精确匹配 [`skill_callable_id`]；若无命中，再允许用**文件 stem** 作为别名（且仅当唯一）。
/// 避免「A 的 stem = B 的 name」时 catalog 列出的 id 在发送时歧义失败。
pub fn resolve_skill_by_id(
    list_opts: SkillsListOpts<'_>,
    id: &str,
) -> Result<SkillDoc, SkillSlashError> {
    let docs = list_skills(list_opts).map_err(SkillSlashError::Io)?;
    resolve_skill_in_docs(&docs, id)
}

fn resolve_skill_in_docs(docs: &[SkillDoc], id: &str) -> Result<SkillDoc, SkillSlashError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(SkillSlashError::EmptyId);
    }
    let id_l = id.to_ascii_lowercase();

    let mut by_callable: Vec<&SkillDoc> = docs
        .iter()
        .filter(|d| skill_callable_id(d).eq_ignore_ascii_case(&id_l))
        .collect();
    by_callable.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    by_callable.dedup_by(|a, b| a.display_path == b.display_path);
    match by_callable.len() {
        1 => return Ok(by_callable.remove(0).clone()),
        n if n > 1 => {
            let candidates = by_callable
                .iter()
                .map(|d| format!("{} (`{}`)", skill_callable_id(d), catalog_display_path(d)))
                .collect::<Vec<_>>()
                .join("；");
            return Err(SkillSlashError::Ambiguous {
                id: id.to_string(),
                candidates,
            });
        }
        _ => {}
    }

    let mut by_stem_alias: Vec<&SkillDoc> = docs
        .iter()
        .filter(|d| {
            !skill_callable_id(d).eq_ignore_ascii_case(&id_l)
                && skill_stem(d)
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&id_l))
        })
        .collect();
    by_stem_alias.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    by_stem_alias.dedup_by(|a, b| a.display_path == b.display_path);
    match by_stem_alias.len() {
        1 => Ok(by_stem_alias.remove(0).clone()),
        n if n > 1 => {
            let candidates = by_stem_alias
                .iter()
                .map(|d| format!("{} (`{}`)", skill_callable_id(d), catalog_display_path(d)))
                .collect::<Vec<_>>()
                .join("；");
            Err(SkillSlashError::Ambiguous {
                id: id.to_string(),
                candidates,
            })
        }
        _ => Err(SkillSlashError::NotFound {
            id: id.to_string(),
            hints: hints_from_docs(docs),
        }),
    }
}

/// 列出可用于 `/id` 补全的 callable id（已排序去重）。
pub fn list_skill_callable_ids(list_opts: SkillsListOpts<'_>) -> Result<Vec<String>, String> {
    Ok(list_skill_catalog_entries(list_opts)?
        .into_iter()
        .map(|e| e.id)
        .collect())
}

/// 供 UI / Tab 补全的 skill 条目（id + 短描述）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCatalogEntry {
    pub id: String,
    pub name: Option<String>,
    pub description: String,
    pub path: String,
}

/// 列出 skill 目录条目：仅含 **resolve 唯一成功** 且非保留词的 callable id。
pub fn list_skill_catalog_entries(
    list_opts: SkillsListOpts<'_>,
) -> Result<Vec<SkillCatalogEntry>, String> {
    let docs = list_skills(list_opts)?;
    let mut out: Vec<SkillCatalogEntry> = Vec::new();
    for doc in &docs {
        let id = skill_callable_id(doc);
        if is_reserved_slash_head(&id) {
            continue;
        }
        if out.iter().any(|e| e.id.eq_ignore_ascii_case(id.as_str())) {
            continue;
        }
        // 与 resolve 契约一致：只列出能唯一解析的 id。
        if resolve_skill_in_docs(&docs, &id).is_err() {
            continue;
        }
        out.push(SkillCatalogEntry {
            id,
            name: doc.name.clone(),
            description: skill_ui_description(doc),
            path: catalog_display_path(doc),
        });
    }
    out.sort_by_key(|a| a.id.to_ascii_lowercase());
    Ok(out)
}

/// 去掉 `/id` 后的用户任务；空则给默认句。
pub fn default_skill_user_task() -> &'static str {
    "请按该技能执行。"
}

/// 解析用户输入：若为显式 `/<skill-id> …` 则解析 skill 并剥离前缀。
#[derive(Debug, Clone)]
pub struct PreparedUserSkills {
    /// 送入模型的 user 正文（已去掉 `/id` 前缀）。
    pub user_message: String,
    /// 本轮强制注入的 skill（显式选用时）。
    pub forced_skill: Option<SkillDoc>,
    /// 命中的 callable id（便于日志）。
    pub invoked_id: Option<String>,
}

/// 将原始用户输入准备为「可能强制挂载的 skill + 任务正文」。
///
/// - 非 `/` 开头、或命中内建保留词：不强制 skill，原文返回。
/// - `/<未知内建且像 skill>`：解析成功则剥离；失败返回 `Err`（调用方应提示用户）。
pub fn prepare_user_message_for_skills(
    raw_user: &str,
    list_opts: SkillsListOpts<'_>,
    skills_enabled: bool,
) -> Result<PreparedUserSkills, SkillSlashError> {
    let raw = raw_user.trim();
    if !raw.starts_with('/') {
        return Ok(PreparedUserSkills {
            user_message: raw_user.to_string(),
            forced_skill: None,
            invoked_id: None,
        });
    }
    let rest = raw[1..].trim_start();
    if rest.is_empty() {
        return Ok(PreparedUserSkills {
            user_message: raw_user.to_string(),
            forced_skill: None,
            invoked_id: None,
        });
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("").trim();
    let task = parts.next().unwrap_or("").trim();
    if head.is_empty() || is_reserved_slash_head(head) {
        return Ok(PreparedUserSkills {
            user_message: raw_user.to_string(),
            forced_skill: None,
            invoked_id: None,
        });
    }
    if !skills_enabled {
        return Err(SkillSlashError::Disabled {
            head: head.to_string(),
        });
    }
    let doc = resolve_skill_by_id(list_opts, head)?;
    let invoked = skill_callable_id(&doc);
    let user_message = if task.is_empty() {
        default_skill_user_task().to_string()
    } else {
        task.to_string()
    };
    Ok(PreparedUserSkills {
        user_message,
        forced_skill: Some(doc),
        invoked_id: Some(invoked),
    })
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

    fn ws_opts(base: &Path) -> SkillsListOpts<'_> {
        SkillsListOpts {
            workspace_base_dir: base,
            skills_dir: ".crabmate/skills",
            skills_user_dir: "",
            skills_system_dir: "",
        }
    }

    #[test]
    fn resolve_by_frontmatter_name_and_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(
            &skills,
            "review.md",
            "---\nname: code-review\n---\n# Review\nDo review.\n",
        );
        let by_name = resolve_skill_by_id(ws_opts(tmp.path()), "code-review").unwrap();
        assert!(by_name.content.contains("Do review"));
        let by_stem = resolve_skill_by_id(ws_opts(tmp.path()), "review").unwrap();
        assert_eq!(by_stem.display_path, by_name.display_path);
    }

    #[test]
    fn resolve_prefers_callable_id_over_stem_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(
            &skills,
            "review.md",
            "---\nname: code-review\n---\nfrom review.md\n",
        );
        write_skill(
            &skills,
            "other.md",
            "---\nname: review\n---\nfrom other.md\n",
        );
        let doc = resolve_skill_by_id(ws_opts(tmp.path()), "review").unwrap();
        assert!(doc.content.contains("from other.md"));
        let entries = list_skill_catalog_entries(ws_opts(tmp.path())).unwrap();
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"review"));
        assert!(ids.contains(&"code-review"));
        for e in &entries {
            assert!(resolve_skill_by_id(ws_opts(tmp.path()), &e.id).is_ok());
        }
    }

    #[test]
    fn catalog_skips_reserved_callable_id() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(&skills, "x.md", "---\nname: help\n---\nbody\n");
        write_skill(&skills, "ok.md", "---\nname: ok-skill\n---\nbody\n");
        let entries = list_skill_catalog_entries(ws_opts(tmp.path())).unwrap();
        assert!(entries.iter().all(|e| e.id != "help"));
        assert!(entries.iter().any(|e| e.id == "ok-skill"));
    }

    #[test]
    fn prepare_strips_prefix_and_forces() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(&skills, "cr.md", "---\nname: code-review\n---\nbody\n");
        let p =
            prepare_user_message_for_skills("/code-review 看看 diff", ws_opts(tmp.path()), true)
                .unwrap();
        assert_eq!(p.user_message, "看看 diff");
        assert!(p.forced_skill.is_some());
        assert_eq!(p.invoked_id.as_deref(), Some("code-review"));
    }

    #[test]
    fn prepare_reserved_leaves_raw() {
        let tmp = tempfile::tempdir().unwrap();
        let p = prepare_user_message_for_skills("/skills list", ws_opts(tmp.path()), true).unwrap();
        assert!(p.forced_skill.is_none());
        assert_eq!(p.user_message.trim(), "/skills list");
    }

    #[test]
    fn prepare_unknown_errors_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(&skills, "a.md", "---\nname: alpha\n---\nx\n");
        let err = prepare_user_message_for_skills("/nope", ws_opts(tmp.path()), true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("未找到"));
        assert!(msg.contains("/alpha"));
        let tagged = err.into_turn_err();
        assert!(SkillSlashError::strip_turn_err(&tagged).is_some());
    }

    #[test]
    fn callable_ids_list() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(&skills, "a.md", "---\nname: alpha\n---\nx\n");
        write_skill(&skills, "b.md", "plain\n");
        let ids = list_skill_callable_ids(ws_opts(tmp.path())).unwrap();
        assert!(ids.iter().any(|i| i == "alpha"));
        assert!(ids.iter().any(|i| i == "b"));
    }

    #[test]
    fn catalog_includes_description() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        write_skill(
            &skills,
            "a.md",
            "---\nname: alpha\ndescription: Alpha skill for tests\n---\n# Title\nBody\n",
        );
        let entries = list_skill_catalog_entries(ws_opts(tmp.path())).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "alpha");
        assert!(entries[0].description.contains("Alpha skill"));
    }

    #[test]
    fn nested_skill_md_layout_resolves_by_dir_name() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".crabmate/skills");
        let nested = skills.join("cpp-programming");
        write_skill(
            &nested,
            "SKILL.md",
            "# C++ 编程规范与技巧\n\nUse modern C++.\n",
        );
        write_skill(
            &skills,
            "flat.md",
            "---\nname: flat-skill\n---\nflat body\n",
        );
        let docs = crate::skills::list_skills(ws_opts(tmp.path())).unwrap();
        assert_eq!(docs.len(), 2);

        let by_dir = resolve_skill_by_id(ws_opts(tmp.path()), "cpp-programming").unwrap();
        assert!(by_dir.content.contains("C++"));
        assert_eq!(skill_callable_id(&by_dir), "cpp-programming");

        let entries = list_skill_catalog_entries(ws_opts(tmp.path())).unwrap();
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"cpp-programming"));
        assert!(ids.contains(&"flat-skill"));
        assert!(
            entries
                .iter()
                .any(|e| e.id == "cpp-programming" && e.description.contains("C++"))
        );
    }

    #[test]
    fn slash_resolve_sees_user_layer() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user-skills");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".crabmate/skills")).unwrap();
        write_skill(
            &user,
            "from-user.md",
            "---\nname: from-user\n---\nuser body\n",
        );
        let opts = SkillsListOpts {
            workspace_base_dir: &ws,
            skills_dir: ".crabmate/skills",
            skills_user_dir: user.to_str().unwrap(),
            skills_system_dir: "",
        };
        let doc = resolve_skill_by_id(opts, "from-user").unwrap();
        assert!(doc.content.contains("user body"));
    }
}
