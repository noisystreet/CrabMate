use std::path::{Path, PathBuf};

fn load_cursor_rule_documents(
    cwd: &Path,
    rules_dir: &str,
    include_agents_md: bool,
) -> Result<Vec<(String, String)>, String> {
    let rules_dir = rules_dir.trim();
    if rules_dir.is_empty() {
        return Err("配置错误：cursor_rules_dir 不能为空".to_string());
    }
    let dir_path = {
        let p = Path::new(rules_dir);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    };
    if dir_path.exists() && !dir_path.is_dir() {
        return Err(format!(
            "配置错误：cursor_rules_dir \"{}\" 不是目录",
            dir_path.display()
        ));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if include_agents_md {
        let agents = cwd.join("AGENTS.md");
        if agents.is_file() {
            files.push(agents);
        }
    }
    if dir_path.is_dir() {
        let mut mdc_files: Vec<PathBuf> = std::fs::read_dir(&dir_path)
            .map_err(|e| {
                format!(
                    "无法读取 cursor_rules_dir \"{}\": {}",
                    dir_path.display(),
                    e
                )
            })?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mdc"))
            })
            .collect();
        mdc_files.sort();
        files.extend(mdc_files);
    }

    let mut out: Vec<(String, String)> = Vec::new();
    for path in files {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("无法读取规则文件 \"{}\": {}", path.display(), e))?;
        if content.trim().is_empty() {
            continue;
        }
        let display_path = path
            .strip_prefix(cwd)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        out.push((display_path, content));
    }
    Ok(out)
}

fn render_cursor_rules_appendix(docs: &[(String, String)], max_chars: usize) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut body = String::from(
        "【项目规则（Cursor-like）】\n以下内容来自工作区规则文件；若与更高优先级指令冲突，以更高优先级为准。\n",
    );
    for (path, content) in docs {
        body.push_str("\n\n---\n");
        body.push_str(&format!("规则文件: {}\n\n", path));
        body.push_str(content.trim());
    }
    if body.chars().count() <= max_chars {
        return body;
    }
    let mut truncated = super::super::text_util::truncate_str_to_max_chars(&body, max_chars);
    truncated.push_str(
        "\n\n[提示] 规则内容已按 cursor_rules_max_chars 截断。后续不得假定未出现在本 system 中的规则条文；不得仅凭「应有某规则」下结论。",
    );
    truncated
}

pub(super) fn merge_system_prompt_with_cursor_rules(
    system_prompt: String,
    cursor_rules_enabled: bool,
    cursor_rules_dir: &str,
    cursor_rules_include_agents_md: bool,
    cursor_rules_max_chars: usize,
) -> Result<String, String> {
    if !cursor_rules_enabled {
        return Ok(system_prompt);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let docs = load_cursor_rule_documents(&cwd, cursor_rules_dir, cursor_rules_include_agents_md)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_cursor_rules_appendix(&docs, cursor_rules_max_chars);
    if appendix.is_empty() {
        return Ok(system_prompt);
    }
    Ok(format!("{}\n\n{}", system_prompt.trim_end(), appendix))
}

#[cfg(test)]
mod tests {
    use super::merge_system_prompt_with_cursor_rules;
    use std::path::{Path, PathBuf};

    struct CwdGuard {
        prev: PathBuf,
    }

    impl CwdGuard {
        fn change_to(path: &Path) -> Self {
            let prev = std::env::current_dir().expect("get cwd");
            std::env::set_current_dir(path).expect("set cwd");
            Self { prev }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "crabmate-cursor-rules-test-{}-{}-{}",
            name,
            std::process::id(),
            ts
        ));
        std::fs::create_dir_all(&dir).expect("mkdir temp");
        dir
    }

    #[test]
    fn merge_system_prompt_appends_agents_and_sorted_rules() {
        let ws = temp_workspace("merge-order");
        std::fs::write(
            ws.join("AGENTS.md"),
            "agents_rule: must follow project instruction",
        )
        .expect("write agents");
        let rules_dir = ws.join(".cursor/rules");
        std::fs::create_dir_all(&rules_dir).expect("mkdir rules");
        std::fs::write(rules_dir.join("b_rule.mdc"), "b-rule").expect("write b");
        std::fs::write(rules_dir.join("a_rule.mdc"), "a-rule").expect("write a");
        let _cwd = CwdGuard::change_to(&ws);

        let merged = merge_system_prompt_with_cursor_rules(
            "BASE_PROMPT".to_string(),
            true,
            ".cursor/rules",
            true,
            20_000,
        )
        .expect("merge ok");

        assert!(merged.starts_with("BASE_PROMPT"));
        let p_agents = merged.find("规则文件: AGENTS.md").expect("agents marker");
        let p_a = merged.find("a_rule.mdc").expect("a marker");
        let p_b = merged.find("b_rule.mdc").expect("b marker");
        assert!(p_agents < p_a);
        assert!(p_a < p_b);
        assert!(merged.contains("agents_rule: must follow project instruction"));
        assert!(merged.contains("a-rule"));
        assert!(merged.contains("b-rule"));

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn merge_system_prompt_truncates_when_over_limit() {
        let ws = temp_workspace("merge-truncate");
        let rules_dir = ws.join(".cursor/rules");
        std::fs::create_dir_all(&rules_dir).expect("mkdir rules");
        std::fs::write(rules_dir.join("rule.mdc"), "x".repeat(4096)).expect("write rule");
        let _cwd = CwdGuard::change_to(&ws);

        let merged = merge_system_prompt_with_cursor_rules(
            "BASE_PROMPT".to_string(),
            true,
            ".cursor/rules",
            false,
            180,
        )
        .expect("merge ok");

        assert!(merged.contains("BASE_PROMPT"));
        assert!(merged.contains("规则内容已按 cursor_rules_max_chars 截断"));
        assert!(merged.len() > "BASE_PROMPT".len());

        let _ = std::fs::remove_dir_all(&ws);
    }
}
