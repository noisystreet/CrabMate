//! JVM 生态最小工具：Maven / Gradle（工作区根须存在对应构建描述文件）。

use std::path::Path;
use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 800;

fn has_pom(workspace_root: &Path) -> bool {
    workspace_root.join("pom.xml").is_file()
}

fn has_gradle(workspace_root: &Path) -> bool {
    workspace_root.join("build.gradle").is_file()
        || workspace_root.join("build.gradle.kts").is_file()
        || workspace_root.join("settings.gradle").is_file()
        || workspace_root.join("settings.gradle.kts").is_file()
}

/// 仅允许简短 Maven profile 名（无空白、无路径分隔）。
fn safe_maven_profile(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("profile 不能为空".to_string());
    }
    if t.chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\')
    {
        return Err("profile 含非法字符".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("profile 仅允许字母数字与 _-. ".to_string());
    }
    Ok(())
}

/// Gradle 任务名：字母数字、`_`、`:`、`.`、`-`。
fn safe_gradle_task_token(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("任务名不能为空".to_string());
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'))
    {
        return Err("任务名仅允许字母数字与 _:.-".to_string());
    }
    Ok(())
}

fn run_and_format(cmd: Command, max_output_len: usize, title: &str) -> String {
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
        output_util::CommandSpawnErrorStyle::CannotStartCommand,
    )
}

pub fn maven_compile(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_pom(workspace_root) {
        return "maven_compile: 跳过（未找到 pom.xml）".to_string();
    }

    if let Some(p) = v.get("profile").and_then(|x| x.as_str())
        && let Err(e) = safe_maven_profile(p)
    {
        return format!("错误：{}", e);
    }

    let mut cmd = Command::new("mvn");
    cmd.arg("-q").arg("compile");
    if let Some(p) = v.get("profile").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
    {
        cmd.arg("-P").arg(p);
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "mvn -q compile")
}

pub fn maven_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_pom(workspace_root) {
        return "maven_test: 跳过（未找到 pom.xml）".to_string();
    }

    if let Some(p) = v.get("profile").and_then(|x| x.as_str())
        && let Err(e) = safe_maven_profile(p)
    {
        return format!("错误：{}", e);
    }
    if let Some(t) = v.get("test").and_then(|x| x.as_str()) {
        let t = t.trim();
        if !t.is_empty()
            && !t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '#'))
        {
            return "错误：test 参数含非法字符（仅允许类名/方法片段等安全子集）".to_string();
        }
    }

    let mut cmd = Command::new("mvn");
    cmd.arg("-q").arg("test");
    if let Some(p) = v.get("profile").and_then(|x| x.as_str()).map(str::trim)
        && !p.is_empty()
    {
        cmd.arg("-P").arg(p);
    }
    if let Some(t) = v.get("test").and_then(|x| x.as_str()).map(str::trim)
        && !t.is_empty()
    {
        cmd.arg(format!("-Dtest={t}"));
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "mvn -q test")
}

fn gradle_command(workspace_root: &Path) -> Command {
    let gw = workspace_root.join("gradlew");
    if gw.is_file() {
        return Command::new(gw);
    }
    #[cfg(windows)]
    {
        let gwb = workspace_root.join("gradlew.bat");
        if gwb.is_file() {
            return Command::new(gwb);
        }
    }
    Command::new("gradle")
}

pub fn gradle_compile(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_gradle(workspace_root) {
        return "gradle_compile: 跳过（未找到 build.gradle / build.gradle.kts / settings.gradle*）"
            .to_string();
    }

    let tasks: Vec<String> = v
        .get("tasks")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(String::from)
                .collect()
        })
        .filter(|t: &Vec<String>| !t.is_empty())
        .unwrap_or_else(|| vec!["classes".to_string()]);

    for t in &tasks {
        if let Err(e) = safe_gradle_task_token(t) {
            return format!("错误：任务 `{}` {}", t, e);
        }
    }

    let mut cmd = gradle_command(workspace_root);
    cmd.arg("-q");
    for t in &tasks {
        cmd.arg(t);
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "gradle -q …")
}

pub fn gradle_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !has_gradle(workspace_root) {
        return "gradle_test: 跳过（未找到 build.gradle / build.gradle.kts / settings.gradle*）"
            .to_string();
    }

    let tasks: Vec<String> = v
        .get("tasks")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(String::from)
                .collect()
        })
        .filter(|t: &Vec<String>| !t.is_empty())
        .unwrap_or_else(|| vec!["test".to_string()]);

    for t in &tasks {
        if let Err(e) = safe_gradle_task_token(t) {
            return format!("错误：任务 `{}` {}", t, e);
        }
    }

    let mut cmd = gradle_command(workspace_root);
    cmd.arg("-q");
    for t in &tasks {
        cmd.arg(t);
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "gradle -q …")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_maven_profile_rejects_slash() {
        assert!(safe_maven_profile("a/b").is_err());
    }

    #[test]
    fn safe_gradle_task_accepts_colon() {
        assert!(safe_gradle_task_token(":compileJava").is_ok());
    }
}
