use std::path::{Path, PathBuf};

/// 解析 Web 工作区白名单：未配置或空列表时仅允许 `run_command_working_dir`；否则每项须为已存在目录的绝对或相对路径（相对路径相对**进程当前目录**）。
pub(super) fn resolve_workspace_allowed_roots(
    roots_opt: Option<Vec<String>>,
    run_root: &Path,
) -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let Some(roots_in) = roots_opt.filter(|v| !v.is_empty()) else {
        return Ok(vec![run_root.to_path_buf()]);
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for s in roots_in {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let p = Path::new(s);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        let canon = joined
            .canonicalize()
            .map_err(|e| format!("workspace_allowed_roots 项 {:?} 无法解析为目录: {}", s, e))?;
        if !canon.is_dir() {
            return Err(format!(
                "workspace_allowed_roots 项 {} 不是目录",
                canon.display()
            ));
        }
        out.push(canon);
    }
    if out.is_empty() {
        return Err(
            "workspace_allowed_roots 配置为空：请省略该项（仅用 run_command_working_dir）或至少填写一个有效路径"
                .to_string(),
        );
    }
    out.sort();
    out.dedup();
    if !out.iter().any(|root| run_root.starts_with(root)) {
        return Err(format!(
            "配置错误：run_command_working_dir（{}）不在任何 workspace_allowed_roots 之下；请调整配置使默认工作目录落在某一允许根目录下",
            run_root.display()
        ));
    }
    Ok(out)
}
