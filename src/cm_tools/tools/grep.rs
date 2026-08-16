//! 在工作区内按正则/关键词搜索文件内容。
//!
//! 使用 `ignore` crate（ripgrep 同源）做 .gitignore 感知的文件遍历，
//! `regex` crate 做行级匹配，支持上下文行和 glob 过滤。
//! 实现见 [`super::grep_try`]。

use std::path::Path;

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    match super::grep_try::search_in_files_try(args_json, workspace_root) {
        Ok(s) => s,
        Err(e) => e.message,
    }
}
