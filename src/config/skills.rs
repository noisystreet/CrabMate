use std::path::{Path, PathBuf};

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn load_skill_documents(cwd: &Path, skills_dir: &str) -> Result<Vec<(String, String)>, String> {
    let skills_dir = skills_dir.trim();
    if skills_dir.is_empty() {
        return Err("配置错误：skills_dir 不能为空".to_string());
    }
    let dir_path = {
        let p = Path::new(skills_dir);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    };
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

    let mut out: Vec<(String, String)> = Vec::new();
    for path in skill_files {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("无法读取技能文件 \"{}\": {}", path.display(), e))?;
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

fn render_skills_appendix(docs: &[(String, String)], max_chars: usize) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut body = String::from(
        "【项目技能（skills）】\n以下内容来自技能目录；若与更高优先级指令冲突，以更高优先级为准。\n",
    );
    for (path, content) in docs {
        body.push_str("\n\n---\n");
        body.push_str(&format!("技能文件: {}\n\n", path));
        body.push_str(content.trim());
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

pub(super) fn merge_system_prompt_with_skills(
    system_prompt: String,
    skills_enabled: bool,
    skills_dir: &str,
    skills_max_chars: usize,
) -> Result<String, String> {
    if !skills_enabled {
        return Ok(system_prompt);
    }
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let docs = load_skill_documents(&cwd, skills_dir)?;
    if docs.is_empty() {
        return Ok(system_prompt);
    }
    let appendix = render_skills_appendix(&docs, skills_max_chars);
    if appendix.is_empty() {
        return Ok(system_prompt);
    }
    Ok(format!("{}\n\n{}", system_prompt.trim_end(), appendix))
}
