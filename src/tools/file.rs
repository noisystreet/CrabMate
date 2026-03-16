//! 工作区文件创建与修改工具
//!
//! 路径均为**相对于工作目录**的相对路径（与 main 中 workspace 文件 API 一致，基于 run_command_working_dir）。

use std::path::{Path, PathBuf};

/// 解析用于读取或修改的路径（目标必须存在；path 必须为相对工作目录的相对路径）
fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let joined = base.join(sub);
    joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))
}

/// 解析用于写入的路径（目标可不存在；path 必须为相对工作目录的相对路径，且不能通过 .. 超出工作目录）
fn resolve_for_write(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))?;
    let joined = base_canonical.join(sub);
    // 规范化 .. 和 . 并确保仍在 base 下（路径穿越检查）
    let normalized = normalize_path(&joined);
    if !normalized.starts_with(&base_canonical) {
        return Err("路径不能超出工作目录".to_string());
    }
    Ok(normalized)
}

/// 简单规范化：去掉 . 和 .. 段（不访问文件系统）
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// 创建文件：仅在文件不存在时创建；若已存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn create_file(args_json: &str, working_dir: &Path) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if target.exists() {
        return "错误：文件已存在，无法仅创建".to_string();
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("创建目录失败: {}", e);
            }
        }
    }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => format!("已创建文件: {}", target.display()),
        Err(e) => format!("写入文件失败: {}", e),
    }
}

/// 修改文件：仅在文件已存在时覆盖内容；若不存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn modify_file(args_json: &str, working_dir: &Path) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法仅修改".to_string();
    }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => format!("已修改文件: {}", target.display()),
        Err(e) => format!("写入文件失败: {}", e),
    }
}

fn parse_path_content(args_json: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(String::from)
        .ok_or_else(|| "缺少 path 参数".to_string())?;
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((path, content))
}
