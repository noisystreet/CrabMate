//! Markdown 内链接检查：校验工作区内相对目标是否存在；外链仅在配置允许前缀时发起 HEAD 探测。
//!
//! 路径与 `file` 工具一致：扫描根须为工作区相对路径，禁止 `..` 与绝对路径；解析目标时做词法归一化并限制在工作区根之下。

mod check;
mod core;

pub use check::markdown_check_links;

#[cfg(test)]
mod tests;
