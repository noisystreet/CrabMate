//! `skill_manage`：技能（skills）管理元工具 —— install / list / remove。
//!
//! 技能三层（系统 → 用户 → 工作区，同 id 工作区优先）见 [`crate::cm_config::skills`]；
//! 本工具只做**文件级管理**：
//! - `install`：写入 `<层目录>/<name>/SKILL.md`（自动生成 frontmatter）；同层同 id 冲突需 `force=true`。
//! - `list`：三层全量物理文件 + 跨层覆盖关系标注。
//! - `remove`：按可调用 id 删除技能文件（需 `confirm=true`；跨层歧义用 `layer` 消歧）。
//!
//! 技能在每轮对话构建 system 提示时重扫，安装 / 删除后**下一轮**自动生效，无需重启或 reload。

use std::collections::HashMap;
use std::path::Path;

use crate::cm_config::skills::{
    SkillLayer, SkillManagedDoc, SkillsListOpts, list_skills_by_layer, resolve_skill_layer_dir,
    skill_ui_description,
};
use crate::cm_config::skills_slash::skill_callable_id;
use crate::cm_config::SkillsConfigSection;
use crate::cm_tools::tools::tool_param_types::{
    SkillManageAction, SkillManageArgs, SkillManageLayer,
};

/// 入口：按 `action` 分发（参数校验错误均返回中文可读文案）。
pub(crate) fn skill_manage(
    args: SkillManageArgs,
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> String {
    let Some(action) = args.action else {
        return "错误：缺少 action 参数（install | list | remove）".to_string();
    };
    match action {
        SkillManageAction::List => list_action(skills, working_dir),
        SkillManageAction::Install => install_action(&args, skills, working_dir),
        SkillManageAction::Remove => remove_action(&args, skills, working_dir),
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn list_action(skills: &SkillsConfigSection, working_dir: &Path) -> String {
    let entries = match scan_entries(skills, working_dir) {
        Ok(v) => v,
        Err(e) => return format!("错误：{e}"),
    };
    if entries.is_empty() {
        return "当前没有任何技能文件（系统 / 用户 / 工作区三层均为空）。可用 install 动作安装技能。"
            .to_string();
    }
    // 跨层覆盖标注：生效优先级 workspace > user > system；倒序遍历（entries 为 system→user→workspace），
    // merge key 首次出现者为生效条目；被**更高优先级层**占据同 key 的条目标记为未生效（同层重复不算覆盖）。
    let mut claim: HashMap<String, SkillLayer> = HashMap::new();
    let mut shadowed: Vec<bool> = vec![false; entries.len()];
    for (idx, e) in entries.iter().enumerate().rev() {
        let key = e.merge_key();
        match claim.get(&key) {
            None => {
                claim.insert(key, e.layer);
            }
            Some(&higher) if higher != e.layer => shadowed[idx] = true,
            Some(_) => {}
        }
    }
    let mut out = format!(
        "当前共 {} 个技能文件（生效优先级：workspace > user > system）：\n",
        entries.len()
    );
    for (e, shadow) in entries.iter().zip(&shadowed) {
        out.push_str(&format!(
            "\n- [{}] {}：{}（`{}`）",
            e.layer.label(),
            skill_callable_id(&e.doc),
            skill_ui_description(&e.doc),
            e.doc.display_path
        ));
        if *shadow {
            out.push_str(" —— 被更高优先级层同名技能覆盖，**未生效**");
        }
    }
    out.push_str(
        "\n\n[提示] 正文用 read_file 读取；删除用 remove 动作（需 confirm=true）；跨工作区常驻技能建议安装到 user 层。",
    );
    out
}

// ---------------------------------------------------------------------------
// install
// ---------------------------------------------------------------------------

/// install 参数解析与校验：返回（name、正文、目标层、层目录）。
fn install_plan(
    args: &SkillManageArgs,
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> Result<(String, String, SkillLayer, std::path::PathBuf), String> {
    let name = args
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "错误：install 需要 name 参数（技能 id，如 `code-review`；小写连字符风格）".to_string()
        })?;
    validate_skill_id(name)?;
    let content = args
        .content
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "错误：install 需要 content 参数（Markdown 正文；frontmatter 由工具自动生成）".to_string()
        })?;
    // content 自带 frontmatter 会与工具生成的叠加：解析器只认第一个 `---` 块，原块沦为正文。
    if content.lines().next().map(str::trim) == Some("---") {
        return Err("错误：content 以 `---` frontmatter 开头；请剥离原有 frontmatter 只传 Markdown 正文（name / description 用参数提供，frontmatter 由本工具自动生成）".to_string());
    }
    let layer = install_layer(args.layer)?;
    let dir = layer_dir(skills, working_dir, layer)?;
    Ok((name.to_string(), content.to_string(), layer, dir))
}

/// install 目标层：仅 workspace（默认）/ user；system 拒绝。
fn install_layer(layer: Option<SkillManageLayer>) -> Result<SkillLayer, String> {
    match layer.unwrap_or(SkillManageLayer::Workspace) {
        SkillManageLayer::Workspace => Ok(SkillLayer::Workspace),
        SkillManageLayer::User => Ok(SkillLayer::User),
        SkillManageLayer::System => Err(
            "错误：install 不允许写入系统级层（system）；请用 workspace（默认）或 user".to_string(),
        ),
    }
}

/// 目标层内同 id（merge key）冲突的技能条目。
fn install_conflicts(
    skills: &SkillsConfigSection,
    working_dir: &Path,
    layer: SkillLayer,
    key: &str,
) -> Result<Vec<SkillManagedDoc>, String> {
    Ok(scan_entries(skills, working_dir)?
        .into_iter()
        .filter(|d| d.layer == layer && d.merge_key() == key)
        .collect())
}

fn install_conflict_error(layer: SkillLayer, name: &str, conflicts: &[SkillManagedDoc]) -> String {
    let listing = conflicts
        .iter()
        .map(|c| format!("`{}`", c.abs_path.display()))
        .collect::<Vec<_>>()
        .join("、");
    format!(
        "错误：{} 层已存在同 id 技能 `{name}`：{listing}。传 force=true 覆盖（仅覆盖本工具的规范路径 `<dir>/{name}/SKILL.md`）。",
        layer.label()
    )
}

/// frontmatter `description:`：参数优先；缺省回退正文首个非空行。
fn install_description(args: &SkillManageArgs, content: &str) -> Option<String> {
    args.description
        .as_deref()
        .map(single_line)
        .filter(|d| !d.is_empty())
        .or_else(|| fallback_description(content))
}

/// 组装 SKILL.md：自动 frontmatter + 正文。
fn build_skill_file_content(name: &str, description: Option<&str>, content: &str) -> String {
    let mut file = format!("---\nname: {name}\n");
    if let Some(d) = description {
        file.push_str(&format!("description: {d}\n"));
    }
    file.push_str("---\n\n");
    file.push_str(content.trim_end());
    file.push('\n');
    file
}

/// 写盘（含目录创建）。
fn write_skill_file(target_path: &Path, file_content: &str) -> Result<(), String> {
    let parent = target_path
        .parent()
        .ok_or_else(|| format!("错误：无效目标路径 `{}`", target_path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("错误：无法创建技能目录 `{}`：{e}", parent.display()))?;
    std::fs::write(target_path, file_content)
        .map_err(|e| format!("错误：写入技能文件 `{}` 失败：{e}", target_path.display()))
}

/// 跨层覆盖提示（install 成功后追加）。
fn install_cross_notes(
    out: &mut String,
    skills: &SkillsConfigSection,
    working_dir: &Path,
    layer: SkillLayer,
    key: &str,
) {
    let Ok(entries) = scan_entries(skills, working_dir) else {
        return;
    };
    let mut cross: Vec<&str> = entries
        .iter()
        .filter(|d| d.merge_key() == key && d.layer != layer)
        .map(|d| d.layer.label())
        .collect();
    cross.sort_unstable();
    cross.dedup();
    if cross.is_empty() {
        return;
    }
    if layer == SkillLayer::Workspace {
        out.push_str(&format!(
            "\n[提示] 本技能将覆盖 {} 层同名技能（工作区层优先）。",
            cross.join(" / ")
        ));
    } else {
        out.push_str(&format!(
            "\n[提示] 更高优先级层（{}）存在同名技能；在那些工作区内本条**不生效**。",
            cross.join(" / ")
        ));
    }
}

/// install 尾注：skills_enabled 提示 + 下一轮生效提示。
fn install_footer(out: &mut String, skills: &SkillsConfigSection, name: &str) {
    if !skills.skills_enabled {
        out.push_str(
            "\n[提示] skills_enabled=false：文件已写入，但不会注入 system 提示；启用后生效。",
        );
    }
    out.push_str(&format!(
        "\n[提示] 技能在**下一轮对话**自动生效，无需重启；可用 `/{name}` 显式调用。"
    ));
}

fn install_action(
    args: &SkillManageArgs,
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> String {
    let (name, content, layer, dir) = match install_plan(args, skills, working_dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = name.to_ascii_lowercase();
    let conflicts = match install_conflicts(skills, working_dir, layer, &key) {
        Ok(v) => v,
        Err(e) => return format!("错误：扫描目标层目录失败：{e}"),
    };
    if !conflicts.is_empty() && !args.force.unwrap_or(false) {
        return install_conflict_error(layer, &name, &conflicts);
    }
    let description = install_description(args, &content);
    let file_content = build_skill_file_content(&name, description.as_deref(), &content);
    let target_path = dir.join(&name).join("SKILL.md");
    let canonical_conflict = conflicts.iter().any(|c| c.abs_path == target_path);
    let others: Vec<String> = conflicts
        .iter()
        .filter(|c| c.abs_path != target_path)
        .map(|c| format!("`{}`", c.abs_path.display()))
        .collect();
    if let Err(e) = write_skill_file(&target_path, &file_content) {
        return e;
    }
    let mut out = format!(
        "已{}技能 `{name}` 到 {} 层：`{}`（{} 字符）",
        if canonical_conflict { "覆盖" } else { "安装" },
        layer.label(),
        target_path.display(),
        file_content.chars().count()
    );
    if let Some(d) = &description {
        out.push_str(&format!("\n描述：{d}"));
    }
    if !others.is_empty() {
        out.push_str(&format!(
            "\n[警告] 目标层仍有同 id 的其他文件：{}；同层同 id 会导致 `/{name}` 解析歧义，建议手动清理。",
            others.join("、")
        ));
    }
    install_cross_notes(&mut out, skills, working_dir, layer, &key);
    install_footer(&mut out, skills, &name);
    out
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

fn remove_action(
    args: &SkillManageArgs,
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> String {
    let Some(name) = args.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return "错误：remove 需要 name 参数（技能 id；可先用 list 动作查看）".to_string();
    };
    if !args.confirm.unwrap_or(false) {
        return format!("错误：删除技能 `{name}` 需 confirm=true（不可恢复，请先 list 确认目标）。");
    }
    let entries = match scan_entries(skills, working_dir) {
        Ok(v) => v,
        Err(e) => return format!("错误：{e}"),
    };
    let key = name.to_ascii_lowercase();
    let cands: Vec<_> = entries.iter().filter(|e| e.merge_key() == key).collect();
    if cands.is_empty() {
        return remove_not_found(name, &entries);
    }
    let scope = remove_scope(args.layer);
    match remove_pick_target(name, &cands, scope) {
        Err(e) => e,
        Ok(target) => remove_delete(skills, working_dir, name, target),
    }
}

/// remove 目标层：缺省即工作区层；系统层需显式 `layer=system`。
fn remove_scope(layer: Option<SkillManageLayer>) -> SkillLayer {
    match layer {
        Some(SkillManageLayer::User) => SkillLayer::User,
        Some(SkillManageLayer::System) => SkillLayer::System,
        _ => SkillLayer::Workspace,
    }
}

/// 未找到时的回显：列出当前可调用 id 帮助纠错。
fn remove_not_found(name: &str, entries: &[SkillManagedDoc]) -> String {
    let hints = entries
        .iter()
        .map(|e| skill_callable_id(&e.doc))
        .take(8)
        .collect::<Vec<_>>()
        .join(" · ");
    if hints.is_empty() {
        format!("错误：未找到技能 `{name}`（目录无技能文件）。")
    } else {
        format!("错误：未找到技能 `{name}`。当前技能：{hints}。")
    }
}

/// layer 消歧：scoped 为 0 → 提示各层分布；1 → 唯一删除目标；多个 → 要求先手动清理。
fn remove_pick_target<'a>(
    name: &str,
    cands: &[&'a SkillManagedDoc],
    scope: SkillLayer,
) -> Result<&'a SkillManagedDoc, String> {
    let scoped: Vec<_> = cands.iter().copied().filter(|e| e.layer == scope).collect();
    match scoped.as_slice() {
        [] => Err(remove_scope_mismatch(name, cands)),
        [only] => Ok(only),
        many => Err(remove_ambiguous(name, many, scope)),
    }
}

fn remove_scope_mismatch(name: &str, cands: &[&SkillManagedDoc]) -> String {
    let listing = cands
        .iter()
        .map(|c| format!("[{}] `{}`", c.layer.label(), c.abs_path.display()))
        .collect::<Vec<_>>()
        .join("；");
    let layers = cands
        .iter()
        .map(|c| c.layer.label())
        .collect::<Vec<_>>()
        .join(" / ");
    format!("错误：技能 `{name}` 存在于 {layers}；请用 layer 参数指定要删除的层：{listing}")
}

fn remove_ambiguous(name: &str, many: &[&SkillManagedDoc], scope: SkillLayer) -> String {
    let listing = many
        .iter()
        .map(|c| format!("`{}`", c.abs_path.display()))
        .collect::<Vec<_>>()
        .join("；");
    format!(
        "错误：{} 层内 `{name}` 匹配多个文件：{listing}；请先手动清理为唯一后再删除。",
        scope.label()
    )
}

/// 删除目标文件并组装成功回执。
fn remove_delete(
    skills: &SkillsConfigSection,
    working_dir: &Path,
    name: &str,
    target: &SkillManagedDoc,
) -> String {
    let path = &target.abs_path;
    if let Err(e) = std::fs::remove_file(path) {
        return format!("错误：删除 `{}` 失败：{e}", path.display());
    }
    let dir_note = remove_cleanup_dir(skills, working_dir, target);
    format!(
        "已删除 {} 层技能 `{name}`：`{}`{dir_note}\n[提示] 下一轮对话起不再注入；若更底层存在同名技能，该层副本将恢复生效。",
        target.layer.label(),
        path.display()
    )
}

/// 嵌套布局 `<id>/SKILL.md`：目录空则一并清理，返回注释片段（平铺文件无目录可清）。
fn remove_cleanup_dir(
    skills: &SkillsConfigSection,
    working_dir: &Path,
    target: &SkillManagedDoc,
) -> String {
    let Ok(layer_dir) = layer_dir(skills, working_dir, target.layer) else {
        return String::new();
    };
    let Some(parent) = target.abs_path.parent() else {
        return String::new();
    };
    if parent == layer_dir {
        return String::new();
    }
    let is_empty = std::fs::read_dir(parent)
        .map(|mut rd| rd.next().is_none())
        .unwrap_or(false);
    if is_empty
        && std::fs::remove_dir(parent).is_ok()
    {
        return format!("（空目录 `{}` 已一并清理）", parent.display());
    }
    String::new()
}

// ---------------------------------------------------------------------------
// 公共助手
// ---------------------------------------------------------------------------

/// 三层全量扫描（system → user → workspace，不去重）。
fn scan_entries(
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> Result<Vec<SkillManagedDoc>, String> {
    list_skills_by_layer(skills.list_opts(working_dir))
}

/// 解析指定层的 skills 目录。
fn layer_dir(
    skills: &SkillsConfigSection,
    working_dir: &Path,
    layer: SkillLayer,
) -> Result<std::path::PathBuf, String> {
    let configured = match layer {
        SkillLayer::Workspace => skills.skills_dir.as_str(),
        SkillLayer::User => skills.skills_user_dir.as_str(),
        SkillLayer::System => skills.skills_system_dir.as_str(),
    };
    resolve_skill_layer_dir(working_dir, configured, layer)
}

/// 技能 id 合法性：仅字母 / 数字 / `-` / `_`，≤64 字符，且不得与内建斜杠命令保留词冲突。
fn validate_skill_id(name: &str) -> Result<(), String> {
    if name.chars().count() > 64 {
        return Err("错误：name 过长（≤64 字符）".to_string());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!(
            "错误：name `{name}` 含非法字符：仅允许字母 / 数字 / `-` / `_`（不含空格与路径分隔符）"
        ));
    }
    if crate::cm_types::is_reserved_slash_head(&name.to_ascii_lowercase()) {
        return Err(format!(
            "错误：name `{name}` 是内建斜杠命令保留词，不能作为技能 id（否则 `/{name}` 无法调用）"
        ));
    }
    Ok(())
}

/// 压成单行标量（frontmatter `description:` 按单行解析）。
fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 描述缺省回退：取正文首个非空行（去 `#` 前缀），截 160 字符；正文无可用行则省略 description。
fn fallback_description(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim().trim_start_matches('#').trim();
        if !t.is_empty() {
            let s: String = single_line(t).chars().take(160).collect();
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn skills_section(user_dir: &str) -> SkillsConfigSection {
        SkillsConfigSection {
            skills_enabled: true,
            skills_dir: ".crabmate/skills".to_string(),
            skills_user_dir: user_dir.to_string(),
            skills_system_dir: String::new(),
            skills_max_chars: 8000,
            skills_top_k: 4,
        }
    }

    fn run(args_json: &str, skills: &SkillsConfigSection, ws: &Path) -> String {
        let args: SkillManageArgs = super::super::parse_args_typed(args_json).unwrap();
        skill_manage(args, skills, ws)
    }

    fn ws_skills_dir(ws: &Path) -> PathBuf {
        ws.join(".crabmate/skills")
    }

    #[test]
    fn missing_action_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        let out = run("{}", &skills, tmp.path());
        assert!(out.contains("缺少 action"), "out={out}");
    }

    #[test]
    fn install_list_remove_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        let skills = skills_section("");
        let out = run(
            r##"{"action":"install","name":"code-review","description":"代码评审技能","content":"# 评审\n按清单逐项检查。"}"##,
            &skills,
            ws,
        );
        assert!(out.contains("已安装"), "out={out}");
        let file = ws_skills_dir(ws).join("code-review/SKILL.md");
        assert!(file.is_file());
        let body = std::fs::read_to_string(&file).unwrap();
        assert!(body.starts_with("---\nname: code-review\n"));
        assert!(body.contains("description: 代码评审技能"));

        // frontmatter 可被技能扫描原样解析。
        let list = run(r#"{"action":"list"}"#, &skills, ws);
        assert!(list.contains("code-review"), "out={list}");
        assert!(list.contains("代码评审技能"), "out={list}");
        assert!(list.contains("[workspace]"), "out={list}");

        // remove 未 confirm 拒绝。
        let denied = run(r#"{"action":"remove","name":"code-review"}"#, &skills, ws);
        assert!(denied.contains("confirm=true"), "out={denied}");
        let removed = run(
            r#"{"action":"remove","name":"code-review","confirm":true}"#,
            &skills,
            ws,
        );
        assert!(removed.contains("已删除"), "out={removed}");
        assert!(!file.exists());
        assert!(!ws_skills_dir(ws).join("code-review").exists());
        let empty = run(r#"{"action":"list"}"#, &skills, ws);
        assert!(empty.contains("没有任何技能"), "out={empty}");
    }

    #[test]
    fn install_conflict_requires_force() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        let skills = skills_section("");
        let base = r#"{"action":"install","name":"dup","content":"v1"}"#;
        assert!(run(base, &skills, ws).contains("已安装"));
        let conflict = run(base, &skills, ws);
        assert!(conflict.contains("force=true"), "out={conflict}");
        let forced = run(
            r#"{"action":"install","name":"dup","content":"v2","force":true}"#,
            &skills,
            ws,
        );
        assert!(forced.contains("已覆盖"), "out={forced}");
        let body =
            std::fs::read_to_string(ws_skills_dir(ws).join("dup/SKILL.md")).unwrap();
        assert!(body.contains("v2"));
    }

    #[test]
    fn install_rejects_bad_names() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        for bad in ["../evil", "a/b", "has space", "help", ""] {
            let out = run(
                &format!(r#"{{"action":"install","name":"{bad}","content":"x"}}"#),
                &skills,
                tmp.path(),
            );
            assert!(out.starts_with("错误："), "name={bad:?} out={out}");
        }
    }

    #[test]
    fn install_user_layer_and_shadow_note() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        let user = tmp.path().join("user-skills");
        let skills = skills_section(user.to_str().unwrap());
        let out = run(
            r#"{"action":"install","name":"shared","content":"user body","layer":"user"}"#,
            &skills,
            ws,
        );
        assert!(out.contains("user 层"), "out={out}");
        assert!(user.join("shared/SKILL.md").is_file());
        // 工作区层同名 → user 层被覆盖标注。
        run(
            r#"{"action":"install","name":"shared","content":"ws body"}"#,
            &skills,
            ws,
        )
        ;
        let list = run(r#"{"action":"list"}"#, &skills, ws);
        assert!(list.contains("未生效"), "out={list}");
        // 跨层删除：默认仅删工作区层，layer=user 删用户层。
        let del_ws = run(
            r#"{"action":"remove","name":"shared","confirm":true}"#,
            &skills,
            ws,
        );
        assert!(del_ws.contains("workspace"), "out={del_ws}");
        assert!(user.join("shared/SKILL.md").is_file());
        let del_user = run(
            r#"{"action":"remove","name":"shared","confirm":true,"layer":"user"}"#,
            &skills,
            ws,
        );
        assert!(del_user.contains("已删除"), "out={del_user}");
        assert!(!user.join("shared/SKILL.md").exists());
    }

    #[test]
    fn remove_not_found_lists_hints() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        run(
            r#"{"action":"install","name":"alpha","content":"x"}"#,
            &skills,
            tmp.path(),
        );
        let out = run(
            r#"{"action":"remove","name":"nope","confirm":true}"#,
            &skills,
            tmp.path(),
        );
        assert!(out.contains("未找到") && out.contains("alpha"), "out={out}");
    }

    #[test]
    fn install_rejects_content_with_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        let out = run(
            r##"{"action":"install","name":"fm","content":"---\nname: fm\ndescription: x\n---\n\n正文"}"##,
            &skills,
            tmp.path(),
        );
        assert!(out.starts_with("错误："), "out={out}");
        assert!(out.contains("frontmatter"), "out={out}");
        assert!(!ws_skills_dir(tmp.path()).join("fm/SKILL.md").exists());
    }

    #[test]
    fn install_rejects_non_ascii_names() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        for bad in ["技能", "code‑review", "ａｂ"] {
            let out = run(
                &format!(r#"{{"action":"install","name":"{bad}","content":"x"}}"#),
                &skills,
                tmp.path(),
            );
            assert!(out.starts_with("错误："), "name={bad:?} out={out}");
        }
    }

    #[test]
    fn install_system_layer_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = skills_section("");
        let out = run(
            r#"{"action":"install","name":"x","content":"x","layer":"system"}"#,
            &skills,
            tmp.path(),
        );
        assert!(out.contains("system"), "out={out}");
    }
}
