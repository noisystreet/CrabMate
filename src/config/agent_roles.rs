//! 可选多角色（`config/agent_roles.toml`）：每条角色在通用 L0 基底上叠加增量正文，再合并 cursor rules 与 skills。
//! 工程 / 审阅 / 科学等角色另叠加 **`coding_workbench_increment`**；陪聊 / 哲学 / 文学角色不叠加该层。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::agent_role_spec::AgentRoleSpec;
use super::cursor_rules;
use super::skills;

pub(super) type AgentRoleCatalogBuilt = Arc<HashMap<String, AgentRoleSpec>>;

#[derive(Debug, Default, Clone)]
pub(super) struct AgentRoleEntryBuilder {
    pub(super) system_prompt: Option<String>,
    pub(super) system_prompt_file: Option<String>,
    /// 非空：仅允许列出的工具；含字面量 **`mcp`** 表示允许所有 `mcp__*`。空数组表示不允许任何内置工具（仍可按上条规则放行 MCP）。
    pub(super) allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentRolesToml {
    agent_roles: Option<AgentRolesSection>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentRolesSection {
    default_role: Option<String>,
    roles: Option<HashMap<String, AgentRoleEntryToml>>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct AgentRoleEntryToml {
    system_prompt: Option<String>,
    system_prompt_file: Option<String>,
    allowed_tools: Option<Vec<String>>,
}

/// 将 `config/agent_roles.toml` 合并进 [`super::builder::ConfigBuilder`]（多文件时后加载的覆盖同 id 字段）。
pub(super) fn merge_agent_roles_file_into_builder(
    content: &str,
    default_role_slot: &mut Option<String>,
    entries: &mut HashMap<String, AgentRoleEntryBuilder>,
) -> Result<(), String> {
    let parsed: AgentRolesToml =
        toml::from_str(content).map_err(|e| format!("agent_roles.toml TOML 解析失败: {e}"))?;
    let Some(section) = parsed.agent_roles else {
        return Ok(());
    };
    if let Some(d) = section.default_role {
        let d = d.trim().to_string();
        if !d.is_empty() {
            *default_role_slot = Some(d);
        }
    }
    if let Some(map) = section.roles {
        for (id, row) in map {
            let id = id.trim().to_string();
            if id.is_empty() {
                continue;
            }
            let slot = entries.entry(id).or_default();
            if let Some(p) = row.system_prompt {
                let p = p.trim().to_string();
                if !p.is_empty() {
                    slot.system_prompt = Some(p);
                }
            }
            if let Some(f) = row.system_prompt_file {
                let f = f.trim().to_string();
                if !f.is_empty() {
                    slot.system_prompt_file = Some(f);
                }
            }
            if let Some(list) = row.allowed_tools {
                slot.allowed_tools = Some(list);
            }
        }
    }
    Ok(())
}

fn normalize_allowed_tools(
    raw: Option<Vec<String>>,
) -> Option<std::sync::Arc<std::collections::HashSet<String>>> {
    let list = raw?;
    let mut set = std::collections::HashSet::new();
    for s in list {
        let t = s.trim();
        if t.is_empty() {
            continue;
        }
        set.insert(t.to_string());
    }
    if set.is_empty() {
        None
    } else {
        Some(std::sync::Arc::new(set))
    }
}

fn read_system_prompt_file_resolved(
    raw: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    let raw = raw.trim();
    let path = Path::new(raw);
    if path.is_absolute() {
        return std::fs::read_to_string(path).map_err(|e| {
            format!(
                "无法读取角色 system_prompt_file \"{}\": {}",
                path.display(),
                e
            )
        });
    }

    let mut tried: Vec<String> = Vec::new();

    if let Ok(s) = std::fs::read_to_string(path) {
        return Ok(s);
    }
    tried.push(
        std::env::current_dir()
            .map(|cwd| cwd.join(path).display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
    );

    for base in config_bases.iter().rev() {
        let candidate = base.join(path);
        if let Ok(s) = std::fs::read_to_string(&candidate) {
            return Ok(s);
        }
        tried.push(candidate.display().to_string());
    }

    let work_candidate = run_command_working_dir.join(path);
    if let Ok(s) = std::fs::read_to_string(&work_candidate) {
        return Ok(s);
    }
    tried.push(work_candidate.display().to_string());

    Err(format!(
        "无法读取角色 system_prompt_file \"{}\"（相对路径）。已尝试: {}",
        raw,
        tried.join(" | ")
    ))
}

/// 与 [`crate::config::embedded_coding_workbench_increment`] 相对：不叠加编程工作台层的命名角色 id。
fn is_non_coding_agent_role(role_id: &str) -> bool {
    matches!(role_id, "companion" | "philosopher" | "literary")
}

fn l0_stack_before_role_delta(
    universal_l0: &str,
    coding_workbench_increment: &str,
    role_id: &str,
    role_delta: &str,
) -> String {
    let with_coding = if is_non_coding_agent_role(role_id) {
        universal_l0.to_string()
    } else {
        prepend_l0_base_to_role_body(universal_l0, coding_workbench_increment)
    };
    prepend_l0_base_to_role_body(&with_coding, role_delta)
}

/// 角色专用 `system_prompt` / `system_prompt_file` 叠加在通用 L0（及可选编程层）之后（再经 L1/L2 合并）。
pub(super) fn prepend_l0_base_to_role_body(base_l0: &str, role_body: &str) -> String {
    let base = base_l0.trim();
    let role = role_body.trim();
    match (base.is_empty(), role.is_empty()) {
        (true, true) => String::new(),
        (true, false) => role.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}\n\n{role}"),
    }
}

/// 由累加后的角色条目生成 `id -> 已合并 cursor rules 的 system`；并校验 `default_role_id`。
pub(super) struct FinalizeAgentRoleCatalogParams<'a> {
    pub entries: HashMap<String, AgentRoleEntryBuilder>,
    pub default_role_id: Option<String>,
    pub global_effective_system_prompt: &'a str,
    /// 通用 L0（`system_prompt_file`，默认 `base_system_prompt.md`），尚未合并编程层 / cursor rules / skills。
    pub universal_l0_system_prompt: &'a str,
    /// 编程工作台增量（`coding_workbench_increment.md`）；非陪聊/哲学/文学角色与默认全局会话叠加。
    pub coding_workbench_increment: &'a str,
    pub system_prompt_search_bases: &'a [PathBuf],
    pub run_command_working_dir: &'a Path,
    pub cursor_rules_enabled: bool,
    pub cursor_rules_dir: &'a str,
    pub cursor_rules_include_agents_md: bool,
    pub cursor_rules_max_chars: usize,
    pub skills_enabled: bool,
    pub skills_dir: &'a str,
    pub skills_max_chars: usize,
    pub skills_top_k: usize,
}

pub(super) fn finalize_agent_role_catalog(
    p: FinalizeAgentRoleCatalogParams<'_>,
) -> Result<(Option<String>, AgentRoleCatalogBuilt), String> {
    let FinalizeAgentRoleCatalogParams {
        entries,
        default_role_id,
        global_effective_system_prompt,
        universal_l0_system_prompt,
        coding_workbench_increment,
        system_prompt_search_bases,
        run_command_working_dir,
        cursor_rules_enabled,
        cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars,
        skills_enabled,
        skills_dir,
        skills_max_chars,
        skills_top_k,
    } = p;
    let mut out: HashMap<String, AgentRoleSpec> = HashMap::with_capacity(entries.len());
    for (id, b) in entries {
        let allowed_tools = normalize_allowed_tools(b.allowed_tools);
        let merged = if let Some(ref path) = b.system_prompt_file {
            let raw = read_system_prompt_file_resolved(
                path,
                system_prompt_search_bases,
                run_command_working_dir,
            )?;
            if raw.trim().is_empty() {
                return Err(format!(
                    "配置错误：角色 \"{id}\" 的 system_prompt_file 加载后为空"
                ));
            }
            let combined = l0_stack_before_role_delta(
                universal_l0_system_prompt,
                coding_workbench_increment,
                id.as_str(),
                raw.trim(),
            );
            let with_rules = cursor_rules::merge_system_prompt_with_cursor_rules(
                combined,
                cursor_rules_enabled,
                cursor_rules_dir,
                cursor_rules_include_agents_md,
                cursor_rules_max_chars,
            )?;
            skills::merge_system_prompt_with_skills_selected(
                with_rules,
                skills_enabled,
                skills_dir,
                skills_max_chars,
                run_command_working_dir,
                "",
                skills_top_k,
            )?
        } else if let Some(ref s) = b.system_prompt {
            if s.trim().is_empty() {
                global_effective_system_prompt.to_string()
            } else {
                let combined = l0_stack_before_role_delta(
                    universal_l0_system_prompt,
                    coding_workbench_increment,
                    id.as_str(),
                    s.trim(),
                );
                let with_rules = cursor_rules::merge_system_prompt_with_cursor_rules(
                    combined,
                    cursor_rules_enabled,
                    cursor_rules_dir,
                    cursor_rules_include_agents_md,
                    cursor_rules_max_chars,
                )?;
                skills::merge_system_prompt_with_skills_selected(
                    with_rules,
                    skills_enabled,
                    skills_dir,
                    skills_max_chars,
                    run_command_working_dir,
                    "",
                    skills_top_k,
                )?
            }
        } else {
            global_effective_system_prompt.to_string()
        };
        if merged.trim().is_empty() {
            return Err(format!("配置错误：角色 \"{id}\" 合并后 system 为空"));
        }
        out.insert(
            id,
            AgentRoleSpec {
                system_prompt: merged,
                allowed_tools,
            },
        );
    }

    let default_role_id = default_role_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(ref dr) = default_role_id
        && !out.contains_key(dr)
    {
        return Err(format!(
            "配置错误：default_agent_role / default_role 指向未知角色 id \"{dr}\"（请在 agent_roles.toml 的 [agent_roles.roles] 中定义）"
        ));
    }

    if out.is_empty() {
        Ok((None, Arc::new(HashMap::new())))
    } else {
        Ok((default_role_id, Arc::new(out)))
    }
}

#[cfg(test)]
mod tests {
    use super::prepend_l0_base_to_role_body;

    #[test]
    fn prepend_l0_base_joins_with_blank_line() {
        let out = prepend_l0_base_to_role_body("BASE", "ROLE");
        assert_eq!(out, "BASE\n\nROLE");
    }

    #[test]
    fn l0_stack_omits_coding_for_companion() {
        let out = super::l0_stack_before_role_delta("UNI", "CODE", "companion", "ROLE");
        assert_eq!(out, "UNI\n\nROLE");
        assert!(!out.contains("CODE"));
    }

    #[test]
    fn l0_stack_includes_coding_for_engineer() {
        let out = super::l0_stack_before_role_delta("UNI", "CODE", "engineer", "ROLE");
        assert!(out.starts_with("UNI\n\nCODE"));
        assert!(out.ends_with("ROLE"));
    }
}
