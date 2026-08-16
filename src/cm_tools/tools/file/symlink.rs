//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{canonical_workspace_root, tool_user_error_from_workspace_path};

use crate::cm_tools::tools::tool_param_types::SymlinkInfoArgs;

fn parse_symlink_rel_path(args_json: &str) -> Result<String, String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    let args: SymlinkInfoArgs =
        serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))?;
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    if Path::new(&path).is_absolute() || path.contains("..") {
        return Err("错误：path 必须是相对路径，且不能包含 ..".to_string());
    }
    Ok(path)
}

fn symlink_target_flags(target: &Path, link_target: &Path, base_canonical: &Path) -> (bool, bool) {
    let resolved = target.parent().unwrap_or(base_canonical).join(link_target);
    let dangling = !resolved.exists();
    let outside_workspace = resolved
        .canonicalize()
        .map(|c| !c.starts_with(base_canonical))
        .unwrap_or(true);
    (dangling, outside_workspace)
}

pub fn symlink_info(args_json: &str, working_dir: &Path) -> String {
    let path = match parse_symlink_rel_path(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let base_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    let target = base_canonical.join(&path);

    let meta = match std::fs::symlink_metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("无法读取路径元数据：{}", e),
    };

    if !meta.is_symlink() {
        return format!(
            "{} 不是符号链接（类型：{}）",
            path,
            non_symlink_kind_label(&meta)
        );
    }

    let link_target = match std::fs::read_link(&target) {
        Ok(t) => t,
        Err(e) => return format!("无法读取符号链接目标：{}", e),
    };

    let resolved = target
        .parent()
        .unwrap_or(&base_canonical)
        .join(&link_target);
    let (dangling, outside_workspace) =
        symlink_target_flags(&target, &link_target, &base_canonical);
    format_symlink_report(&path, &link_target, &resolved, dangling, outside_workspace)
}

fn non_symlink_kind_label(meta: &std::fs::Metadata) -> &'static str {
    if meta.is_dir() {
        "目录"
    } else if meta.is_file() {
        "文件"
    } else {
        "其他"
    }
}

fn format_symlink_report(
    path: &str,
    link_target: &Path,
    resolved: &Path,
    dangling: bool,
    outside_workspace: bool,
) -> String {
    let mut out = format!("符号链接：{path}\n");
    out.push_str(&format!("  目标：{}\n", link_target.display()));
    out.push_str(&format!(
        "  状态：{}\n",
        if dangling {
            "悬空（目标不存在）"
        } else {
            "有效"
        }
    ));
    if !dangling {
        out.push_str(&format!(
            "  工作区外：{}\n",
            if outside_workspace { "是" } else { "否" }
        ));
        if let Ok(canonical) = resolved.canonicalize() {
            out.push_str(&format!("  解析后路径：{}\n", canonical.display()));
        }
    }
    out.trim_end().to_string()
}
