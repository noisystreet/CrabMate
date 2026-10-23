//! 会话工作区根 `AGENTS.md` → 首条 `system` 的追加段（L3 基底之后）。
//!
//! 取代原先「`.cursor/rules/*.mdc` + `AGENTS.md` 在 `finalize` 时按**进程 cwd** 拼进 system」的做法：
//! 现在只认 `AGENTS.md`，按**会话工作区根**读取，并在组装首条 `system` 时追加。
//!
//! 放在 `system` 而非首轮 `user`：规则需要每轮都在最高权威层生效；首轮 `user` 在长会话里
//! 会进入上下文折叠区间而失效。注意 Web 续聊**每轮都会重算首条 `system`**
//! （`refresh_existing_turn_system`），因此 `AGENTS.md` 的改动会在下一轮生效，同时使该会话的
//! `system` 前缀缓存失效——「规则实时生效」与「前缀命中率」之间当前选前者。

use std::path::{Path, PathBuf};

/// 规则文件名（固定位于会话工作区根）。
pub const AGENTS_MD_FILE: &str = "AGENTS.md";

/// 解析 `<root>/AGENTS.md`，并挡住**逃出工作区**的软链。
///
/// 该文件内容会进入 `system` 并发往模型，因此一个指向白名单外文件的 symlink
/// 等于让工作区把任意本机文件（如 `/etc/passwd`）读进对话。工作区**内部**的软链仍允许。
/// 非 Unix 平台不做该检查（沿用读取失败即忽略的旧行为）。
#[cfg(unix)]
fn resolve_agents_md_path(workspace_root: &Path) -> Option<PathBuf> {
    let path = workspace_root.join(AGENTS_MD_FILE);
    let real = std::fs::canonicalize(path).ok()?;
    let root = std::fs::canonicalize(workspace_root).ok()?;
    if real.starts_with(&root) {
        return Some(real);
    }
    log::debug!(
        target: "crabmate",
        "AGENTS.md 解析到工作区外（symlink 逃逸？），已忽略：{}",
        real.display()
    );
    None
}

#[cfg(not(unix))]
fn resolve_agents_md_path(workspace_root: &Path) -> Option<PathBuf> {
    Some(workspace_root.join(AGENTS_MD_FILE))
}

/// 读取 `<workspace_root>/AGENTS.md` 并渲染为可追加到 `system` 的段落。
///
/// 返回 `None`：未启用、`max_chars == 0`（配置校验要求 `agents_md_max_chars >= 1024`，正常路径
/// 不可达，仅作防御）、工作区路径为空、文件缺失或不可读、或正文为空
/// （与旧行为一致：没有规则文件时 system 保持原样，不报错）。
#[must_use]
pub fn load_agents_md_block(
    workspace_root: &Path,
    enabled: bool,
    max_chars: usize,
) -> Option<String> {
    if !enabled || max_chars == 0 || workspace_root.as_os_str().is_empty() {
        return None;
    }
    let raw = std::fs::read_to_string(resolve_agents_md_path(workspace_root)?).ok()?;
    let body = raw.trim();
    if body.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## 项目规则（AGENTS.md）\n\n以下内容来自工作区根 `AGENTS.md`；若与更高优先级指令冲突，以更高优先级为准。\n\n",
    );
    out.push_str(body);
    if out.chars().count() <= max_chars {
        return Some(out);
    }
    const TRUNCATION_NOTE: &str =
        "\n\n[提示] 规则内容已按 `agents_md_max_chars` 截断。后续不得假定未出现在本 system 中的规则条文。";
    let note_len = TRUNCATION_NOTE.chars().count();
    if max_chars <= note_len {
        // 防御：配置校验保证 max_chars >= 1024，正常不会走到这里。
        return Some(crate::cm_config::text_util::truncate_str_to_max_chars(
            &out, max_chars,
        ));
    }
    // 预留提示长度，保证返回段落整体不超过 max_chars。
    let mut truncated =
        crate::cm_config::text_util::truncate_str_to_max_chars(&out, max_chars - note_len);
    truncated.push_str(TRUNCATION_NOTE);
    Some(truncated)
}

#[cfg(test)]
mod tests {
    use super::load_agents_md_block;

    fn workspace_with_agents(body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("AGENTS.md"), body).expect("write AGENTS.md");
        dir
    }

    #[test]
    fn loads_agents_md_from_workspace_root() {
        let ws = workspace_with_agents("构建：cargo test\n禁止：直接改 main\n");
        let block = load_agents_md_block(ws.path(), true, 16_000).expect("block");
        assert!(block.contains("项目规则（AGENTS.md）"), "{block}");
        assert!(block.contains("构建：cargo test"), "{block}");
    }

    #[test]
    fn missing_or_empty_file_yields_none() {
        let empty_ws = tempfile::tempdir().expect("tempdir");
        assert!(load_agents_md_block(empty_ws.path(), true, 16_000).is_none());

        let blank = workspace_with_agents("   \n\t\n");
        assert!(load_agents_md_block(blank.path(), true, 16_000).is_none());
    }

    #[test]
    fn disabled_or_zero_budget_yields_none() {
        let ws = workspace_with_agents("规则正文\n");
        assert!(load_agents_md_block(ws.path(), false, 16_000).is_none());
        assert!(load_agents_md_block(ws.path(), true, 0).is_none());
        assert!(load_agents_md_block(std::path::Path::new(""), true, 16_000).is_none());
    }

    #[test]
    fn truncates_and_marks_when_over_budget() {
        let body = "x".repeat(2_000);
        let ws = workspace_with_agents(&body);
        let block = load_agents_md_block(ws.path(), true, 1_024).expect("block");
        assert!(block.contains("agents_md_max_chars"), "{block}");
        // 截断 + 提示后整体仍不得超过上限（提示长度已预留）。
        assert!(
            block.chars().count() <= 1_024,
            "超上限：{}",
            block.chars().count()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_agents_md_symlinked_outside_workspace() {
        // 内容会进 `system` 并发往模型：指向白名单外文件的软链必须被忽略。
        let ws = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "外部内容").expect("write");
        std::os::unix::fs::symlink(&secret, ws.path().join("AGENTS.md")).expect("symlink");
        assert!(load_agents_md_block(ws.path(), true, 16_000).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn follows_agents_md_symlink_inside_workspace() {
        let ws = tempfile::tempdir().expect("tempdir");
        std::fs::write(ws.path().join("real.md"), "内部规则\n").expect("write");
        std::os::unix::fs::symlink(ws.path().join("real.md"), ws.path().join("AGENTS.md"))
            .expect("symlink");
        let block = load_agents_md_block(ws.path(), true, 16_000).expect("block");
        assert!(block.contains("内部规则"), "{block}");
    }

    #[test]
    fn ignores_cursor_rules_mdc_dir() {
        // 回归守卫：不再兼容 `.cursor/rules/*.mdc`。
        let ws = tempfile::tempdir().expect("tempdir");
        let rules = ws.path().join(".cursor/rules");
        std::fs::create_dir_all(&rules).expect("mkdir");
        std::fs::write(rules.join("a.mdc"), "mdc-only rule").expect("write mdc");
        assert!(load_agents_md_block(ws.path(), true, 16_000).is_none());
    }
}
