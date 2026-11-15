//! Markdown 内链接检查：校验工作区内相对目标是否存在；外链仅在配置允许前缀时发起 HEAD 探测。
//!
//! 路径与 `file` 工具一致：扫描根须为工作区相对路径，禁止 `..` 与绝对路径；解析目标时做词法归一化并限制在工作区根之下。

mod check;
mod core;

pub use check::{markdown_check_links_report_try, markdown_check_links_try};

/// 内容层面发现了多少处问题（本地路径 / 锚点 + 外链探测失败）。
///
/// 刻意**不**从正文里 grep「结论: 发现 N 处问题」：那是 `render_text_report` 的排版，
/// 换成 `output_format=json` / `sarif` 时正文里根本没有这句话，按正文判会静默变成
/// 「全部通过」。数量在扫描结果上数，与渲染格式无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownLinkVerdict {
    pub problem_count: usize,
}

impl MarkdownLinkVerdict {
    #[must_use]
    pub fn is_clean(self) -> bool {
        self.problem_count == 0
    }
}

/// 一次链接检查：渲染正文 + 结构化结论（两者互不推导）。
#[derive(Debug, Clone)]
pub struct MarkdownCheckReport {
    /// 按 `output_format` 渲染的正文（text / json / sarif）。
    pub output: String,
    pub verdict: MarkdownLinkVerdict,
}

#[cfg(test)]
mod tests;
