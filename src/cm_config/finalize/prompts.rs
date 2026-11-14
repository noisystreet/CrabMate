use super::*;

/// 读取 `system_prompt_file`：绝对路径直接读；否则依次尝试 cwd、各配置目录（后加载的优先）、`run_command_working_dir`。
pub(super) fn read_system_prompt_file_resolved(
    raw: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    let raw = raw.trim();
    let path = Path::new(raw);
    if path.is_absolute() {
        return std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 system_prompt_file \"{}\": {}", path.display(), e));
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
        "无法读取 system_prompt_file \"{}\"（相对路径）。已尝试: {}",
        raw,
        tried.join(" | ")
    ))
}

/// 内置默认附录（与仓库 **`config/prompts/thinking_avoid_echo_appendix.md`** 一致；未配置路径或读盘失败时采用）。
const EMBEDDED_THINKING_AVOID_ECHO_APPENDIX: &str =
    include_str!("../../../config/prompts/thinking_avoid_echo_appendix.md");

/// 编程工作台增量（与 **`config/prompts/coding_workbench_increment.md`** 一致）。
const EMBEDDED_CODING_WORKBENCH_INCREMENT: &str =
    include_str!("../../../config/prompts/coding_workbench_increment.md");

const EMBEDDED_MODE_ASK: &str = include_str!("../../../config/prompts/mode_ask.md");
const EMBEDDED_MODE_PLAN: &str = include_str!("../../../config/prompts/mode_plan.md");
const EMBEDDED_MODE_ACT: &str = include_str!("../../../config/prompts/mode_act.md");

/// 上下文 LLM 摘要 system（与 **`config/prompts/context_summary_system.md`** 一致）。
const EMBEDDED_CONTEXT_SUMMARY_SYSTEM: &str =
    include_str!("../../../config/prompts/context_summary_system.md");

/// 上下文 LLM 摘要 user 模板（占位符 `{max_tokens}` / `{transcript}`；`{max_chars}` 为别名）。
const EMBEDDED_CONTEXT_SUMMARY_USER: &str =
    include_str!("../../../config/prompts/context_summary_user.md");

/// 与 `resolve_thinking_avoid_echo_appendix` 使用的内置正文一致；供 `augment_system_prompt` 等在运行时附录字段为空时回退。
pub fn embedded_thinking_avoid_echo_appendix() -> &'static str {
    EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim()
}

/// 内置上下文摘要 system；读盘失败或空文件时回退。
pub fn embedded_context_summary_system() -> &'static str {
    EMBEDDED_CONTEXT_SUMMARY_SYSTEM.trim()
}

/// 内置上下文摘要 user 模板；读盘失败或空文件时回退。
pub fn embedded_context_summary_user_template() -> &'static str {
    EMBEDDED_CONTEXT_SUMMARY_USER.trim()
}

/// 会话模式附录（Ask / Plan / Act）；供首条 system 组装。
pub fn embedded_session_mode_appendix(mode: crate::cm_types::SessionMode) -> &'static str {
    match mode {
        crate::cm_types::SessionMode::Ask => EMBEDDED_MODE_ASK.trim(),
        crate::cm_types::SessionMode::Plan => EMBEDDED_MODE_PLAN.trim(),
        crate::cm_types::SessionMode::Act => EMBEDDED_MODE_ACT.trim(),
    }
}

/// 默认全局会话与工程向角色共用的编程层增量正文。
pub(crate) fn embedded_coding_workbench_increment() -> &'static str {
    EMBEDDED_CODING_WORKBENCH_INCREMENT.trim()
}

/// 读盘解析编程工作台增量；禁用时或读盘失败（回退嵌入默认）返回空或默认正文。
pub(crate) fn resolve_coding_workbench_increment(
    enabled: bool,
    file: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    if !enabled {
        return Ok(String::new());
    }
    let file = file.trim();
    let path = if file.is_empty() {
        "config/prompts/coding_workbench_increment.md"
    } else {
        file
    };
    match read_system_prompt_file_resolved(path, config_bases, run_command_working_dir) {
        Ok(raw) => {
            let t = raw.trim().to_string();
            if t.is_empty() {
                Ok(embedded_coding_workbench_increment().to_string())
            } else {
                Ok(t)
            }
        }
        Err(_) => Ok(embedded_coding_workbench_increment().to_string()),
    }
}

const DEFAULT_CONTEXT_SUMMARY_SYSTEM_FILE: &str = "config/prompts/context_summary_system.md";
const DEFAULT_CONTEXT_SUMMARY_USER_FILE: &str = "config/prompts/context_summary_user.md";
const CONTEXT_SUMMARY_PROMPT_MAX_CHARS: usize = 32_768;

fn resolve_prompt_file_or_embedded(
    file: Option<&str>,
    default_file: &str,
    embedded: &str,
    label: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> String {
    let path = file
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_file);
    let body = match read_system_prompt_file_resolved(path, config_bases, run_command_working_dir) {
        Ok(raw) => {
            let t = raw.trim().to_string();
            if t.is_empty() {
                log::warn!(
                    target: "crabmate",
                    "{label} 读盘为空（{path}），使用内置正文"
                );
                embedded.to_string()
            } else {
                t
            }
        }
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "{e}，使用内置 {label}"
            );
            embedded.to_string()
        }
    };
    if body.chars().count() > CONTEXT_SUMMARY_PROMPT_MAX_CHARS {
        log::warn!(
            target: "crabmate",
            "{label} 超过 {CONTEXT_SUMMARY_PROMPT_MAX_CHARS} 字符，改用内置正文"
        );
        return embedded.to_string();
    }
    body
}

/// 解析上下文摘要 system / user 模板（读盘优先，失败回退嵌入）。
pub(crate) fn resolve_context_summary_prompts(
    system_file: Option<&str>,
    user_file: Option<&str>,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> (String, String) {
    let system = resolve_prompt_file_or_embedded(
        system_file,
        DEFAULT_CONTEXT_SUMMARY_SYSTEM_FILE,
        embedded_context_summary_system(),
        "context_summary_system",
        config_bases,
        run_command_working_dir,
    );
    let user = resolve_prompt_file_or_embedded(
        user_file,
        DEFAULT_CONTEXT_SUMMARY_USER_FILE,
        embedded_context_summary_user_template(),
        "context_summary_user",
        config_bases,
        run_command_working_dir,
    );
    (system, user)
}

pub(super) fn resolve_thinking_avoid_echo_appendix(
    enabled: bool,
    inline: Option<&str>,
    file: Option<&str>,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    const MAX_CHARS: usize = 32_768;
    if !enabled {
        return Ok(String::new());
    }
    let body = if let Some(path) = file.map(str::trim).filter(|s| !s.is_empty()) {
        match read_system_prompt_file_resolved(path, config_bases, run_command_working_dir) {
            Ok(raw) => {
                let t = raw.trim().to_string();
                if t.is_empty() {
                    log::warn!(
                        target: "crabmate",
                        "thinking_avoid_echo_appendix_file 读盘为空，使用内置附录"
                    );
                    EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
                } else {
                    t
                }
            }
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "{e}，使用内置 thinking_avoid_echo 附录"
                );
                EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
            }
        }
    } else if let Some(s) = inline.map(str::trim).filter(|s| !s.is_empty()) {
        s.to_string()
    } else {
        EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
    };
    if body.chars().count() > MAX_CHARS {
        return Err(format!(
            "配置错误：thinking_avoid_echo 附录正文超过 {MAX_CHARS} 字符"
        ));
    }
    Ok(body)
}
