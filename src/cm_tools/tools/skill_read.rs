//! `skill_read`：按可调用 id 读取技能正文——发现式加载（progressive disclosure）的读取端，见 #1326。
//!
//! **与 `read_file` 的分工**：`read_file` 的路径校验锁在**会话工作区**内
//! （[`crate::cm_tools::tools::file::path::resolve_for_read`]），而 skills 三层里的用户级
//! （`$XDG_CONFIG_HOME/crabmate/skills`）与系统级（`/etc/crabmate/skills`）在工作区之外。
//!
//! 因此本工具**不接受路径**：只收 callable id，解析走
//! [`crate::cm_config::skills_slash::resolve_skill_by_id`]。命中的文件必然来自配置里已声明的三层目录之一，
//! 无法用 `..` / symlink 指到别处；正文按 `skills_max_chars` 截断后返回。

use std::path::Path;

use crate::cm_config::SkillsConfigSection;
use crate::cm_config::skills::SkillDoc;
use crate::cm_config::skills_slash::{SkillSlashError, resolve_skill_by_id, skill_callable_id};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::SkillReadArgs;

/// 入口：解析 id → 渲染正文（失败路径全部显式返回 [`ToolError`]）。
#[allow(clippy::result_large_err)]
pub(crate) fn skill_read(
    args: SkillReadArgs,
    skills: &SkillsConfigSection,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let id = args.id.trim();
    if id.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：缺少 id 参数（技能可调用 id，例如 api-design）".to_string(),
        ));
    }
    let doc = resolve_skill_by_id(skills.list_opts(working_dir), id).map_err(map_resolve_error)?;
    Ok(render_skill_body(&doc, skills.skills_max_chars))
}

/// 解析失败一律归类为「参数/目标不合法」；只有扫描目录的 IO 失败算外部错误。
fn map_resolve_error(e: SkillSlashError) -> ToolError {
    match e {
        SkillSlashError::Io(msg) => {
            ToolError::external_code("skill_read_io_failed", format!("错误：{msg}"))
        }
        other => ToolError::invalid_args(format!("错误：{other}")),
    }
}

fn render_skill_body(doc: &SkillDoc, max_chars: usize) -> String {
    let out = format!(
        "技能：{}（`{}`）\n\n---\n\n{}",
        skill_callable_id(doc),
        doc.display_path,
        doc.content.trim()
    );
    if out.chars().count() <= max_chars {
        return out;
    }
    let mut truncated = crate::cm_config::text_util::truncate_str_to_max_chars(&out, max_chars);
    truncated.push_str(
        "\n\n[提示] 技能正文已按 skills_max_chars 截断。后续不得假定未出现在本结果中的条文。",
    );
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_skill(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    fn skills_section(max_chars: usize) -> SkillsConfigSection {
        let mut s: SkillsConfigSection = crate::cm_config::load_config_test_env::without_cm_env_overrides(
            || crate::cm_config::load_config(None).expect("embed default"),
        )
        .skills;
        s.skills_dir = ".crabmate/skills".to_string();
        s.skills_user_dir = String::new();
        s.skills_system_dir = String::new();
        s.skills_max_chars = max_chars;
        s
    }

    #[test]
    fn reads_workspace_skill_body_by_callable_id() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            ".crabmate/skills/api-design/SKILL.md",
            "---\nname: api-design\ndescription: Design and review APIs.\n---\n# API Design\n\nbody-line\n",
        );
        let out = skill_read(
            SkillReadArgs {
                id: "api-design".to_string(),
            },
            &skills_section(32_000),
            tmp.path(),
        )
        .expect("skill_read 应成功");
        assert!(out.contains("api-design"), "输出应含可调用 id：{out}");
        assert!(out.contains("body-line"), "输出应含正文：{out}");
    }

    #[test]
    fn unknown_id_is_invalid_args_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            ".crabmate/skills/known/SKILL.md",
            "---\nname: known\n---\nbody\n",
        );
        let err = skill_read(
            SkillReadArgs {
                id: "no-such-skill".to_string(),
            },
            &skills_section(32_000),
            tmp.path(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("未找到"), "应提示未找到：{err}");
    }

    #[test]
    fn body_is_truncated_at_skills_max_chars() {
        let tmp = tempfile::tempdir().unwrap();
        let body = "x".repeat(4_000);
        write_skill(
            tmp.path(),
            ".crabmate/skills/big/SKILL.md",
            &format!("---\nname: big\n---\n{body}\n"),
        );
        let out = skill_read(
            SkillReadArgs {
                id: "big".to_string(),
            },
            &skills_section(1_024),
            tmp.path(),
        )
        .unwrap();
        assert!(out.chars().count() < 4_000, "应被截断");
        assert!(out.contains("skills_max_chars"), "应带截断提示");
    }

    /// 三层（工作区 / 用户级 / 系统级）都要能按 id 读到正文：后两层在工作区之外，
    /// 正是 `read_file` 到不了、必须由本工具承担的原因（#1326 验收项）。
    #[test]
    fn reads_user_and_system_layer_skills_outside_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        write_skill(
            ws.path(),
            ".crabmate/skills/ws-skill/SKILL.md",
            "---\nname: ws-skill\n---\nworkspace body\n",
        );
        write_skill(
            user.path(),
            "user-skill/SKILL.md",
            "---\nname: user-skill\n---\nuser body\n",
        );
        write_skill(
            system.path(),
            "system-skill/SKILL.md",
            "---\nname: system-skill\n---\nsystem body\n",
        );
        let mut section = skills_section(32_000);
        section.skills_user_dir = user.path().to_str().unwrap().to_string();
        section.skills_system_dir = system.path().to_str().unwrap().to_string();

        for (id, marker) in [
            ("ws-skill", "workspace body"),
            ("user-skill", "user body"),
            ("system-skill", "system body"),
        ] {
            let out = skill_read(
                SkillReadArgs { id: id.to_string() },
                &section,
                ws.path(),
            )
            .unwrap_or_else(|e| panic!("`{id}` 应可读：{e}"));
            assert!(out.contains(marker), "`{id}` 输出应含正文：{out}");
        }
    }
}
