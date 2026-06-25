use std::path::{Path, PathBuf};

/// 解析 Web 工作区白名单：未配置或空列表时允许任意路径（返回空列表）；
/// 否则每项须为已存在目录的绝对或相对路径（相对路径相对**进程当前目录**）。
pub(super) fn resolve_workspace_allowed_roots(
    roots_opt: Option<Vec<String>>,
    _run_root: &Path,
) -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let Some(roots_in) = roots_opt.filter(|v| !v.is_empty()) else {
        // 未配置时返回空列表，表示允许任意路径
        return Ok(vec![]);
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
            "workspace_allowed_roots 配置为空：请省略该项（允许任意路径）或至少填写一个有效路径"
                .to_string(),
        );
    }
    out.sort();
    out.dedup();
    Ok(out)
}
