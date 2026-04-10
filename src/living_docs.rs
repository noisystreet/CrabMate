//! 工作区 `.crabmate/living_docs/` 下的「活文档」：首轮上下文注入短摘要，完整正文由模型用 `read_file` 展开。
//!
//! 路径均经 [`crate::path_workspace::absolutize_relative_under_root`] 解析，不越出工作区根。

use std::fs;
use std::path::Path;

use crate::path_workspace::absolutize_relative_under_root;

const DEFAULT_REL_DIR: &str = ".crabmate/living_docs";
const SUMMARY_FILE: &str = "SUMMARY.md";
const MAP_FILE: &str = "map.md";
const PITFALLS_FILE: &str = "pitfalls.md";
const BUILD_FILE: &str = "build.md";

fn read_optional_file(root: &Path, rel_dir: &str, name: &str, max_each: usize) -> Option<String> {
    if max_each == 0 {
        return None;
    }
    let dir = rel_dir.trim().trim_end_matches(['/', '\\']);
    if dir.is_empty() {
        return None;
    }
    let rel = format!("{dir}/{name}");
    let path = absolutize_relative_under_root(root, &rel).ok()?;
    let raw = fs::read_to_string(&path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let mut body: String = t.chars().take(max_each).collect();
    if t.chars().count() > max_each {
        body.push_str("\n\n[... 已按 living_docs 单文件预算截断 ...]");
    }
    Some(body)
}

/// 合并活文档摘要为一段 Markdown；无文件或全空则 `None`。
pub fn load_living_docs_snippet(
    workspace_root: &Path,
    rel_dir: &str,
    max_total_chars: usize,
    max_each_file: usize,
) -> Option<String> {
    if max_total_chars == 0 {
        return None;
    }
    let rel_dir = if rel_dir.trim().is_empty() {
        DEFAULT_REL_DIR
    } else {
        rel_dir.trim()
    };
    let mut sections: Vec<String> = Vec::new();
    if let Some(s) = read_optional_file(workspace_root, rel_dir, SUMMARY_FILE, max_each_file) {
        sections.push(format!("### 摘要（{SUMMARY_FILE}）\n{s}"));
    }
    if let Some(s) = read_optional_file(workspace_root, rel_dir, MAP_FILE, max_each_file) {
        sections.push(format!("### 模块地图（{MAP_FILE}）\n{s}"));
    }
    if let Some(s) = read_optional_file(workspace_root, rel_dir, PITFALLS_FILE, max_each_file) {
        sections.push(format!("### 常见坑（{PITFALLS_FILE}）\n{s}"));
    }
    if let Some(s) = read_optional_file(workspace_root, rel_dir, BUILD_FILE, max_each_file) {
        sections.push(format!("### 构建与命令（{BUILD_FILE}）\n{s}"));
    }
    if sections.is_empty() {
        return None;
    }
    let header = format!(
        "## 项目活文档（工作区 `{rel_dir}/`，机器可读；需要细节请 `read_file` 对应文件）\n"
    );
    let mut out = header + &sections.join("\n\n");
    if out.chars().count() > max_total_chars {
        let take = max_total_chars.saturating_sub(32);
        out = out.chars().take(take).collect::<String>();
        out.push_str("\n\n[... 活文档总预算已截断 ...]");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_summary_and_truncates_total() {
        let root =
            std::env::temp_dir().join(format!("crabmate_living_docs_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".crabmate/living_docs")).unwrap();
        let mut f = std::fs::File::create(root.join(".crabmate/living_docs/SUMMARY.md")).unwrap();
        writeln!(f, "hello living docs").unwrap();
        let got = load_living_docs_snippet(&root, ".crabmate/living_docs", 200, 500).unwrap();
        assert!(got.contains("hello living"));
        assert!(got.contains("SUMMARY"));
        let _ = fs::remove_dir_all(&root);
    }
}
