//! 工作区代码行数统计：`project_metrics` feature 启用 **tokei**；否则按扩展名 walk 内置粗估。

use std::path::Path;

#[cfg(not(feature = "project_metrics"))]
mod builtin;
#[cfg(feature = "project_metrics")]
mod tokei;

#[derive(Debug, Clone)]
pub struct LangStat {
    pub language: String,
    pub files: usize,
    pub code: usize,
    pub comments: usize,
    pub blanks: usize,
}

impl LangStat {
    pub fn total_lines(&self) -> usize {
        self.code + self.comments + self.blanks
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceCodeStats {
    pub languages: Vec<LangStat>,
}

impl WorkspaceCodeStats {
    pub fn total_files(&self) -> usize {
        self.languages.iter().map(|l| l.files).sum()
    }

    pub fn total_code(&self) -> usize {
        self.languages.iter().map(|l| l.code).sum()
    }

    pub fn total_comments(&self) -> usize {
        self.languages.iter().map(|l| l.comments).sum()
    }

    pub fn total_blanks(&self) -> usize {
        self.languages.iter().map(|l| l.blanks).sum()
    }

    pub fn total_lines(&self) -> usize {
        self.total_code() + self.total_comments() + self.total_blanks()
    }
}

/// 与 `code_stats` / 项目画像共用的排除目录。
pub const DEFAULT_EXCLUDED_DIRS: &[&str] =
    &["target", "node_modules", "vendor", "dist", "build", ".git"];

/// 扫描 `root` 下源码规模（tokei 或内置 walk）。
pub fn gather_workspace_code_stats(root: &Path, excluded: &[&str]) -> WorkspaceCodeStats {
    #[cfg(feature = "project_metrics")]
    {
        tokei::gather(root, excluded)
    }
    #[cfg(not(feature = "project_metrics"))]
    {
        builtin::gather(root, excluded)
    }
}

/// 项目画像 Markdown 中的小节标题后缀（标明数据源）。
pub fn profile_stats_heading_suffix() -> &'static str {
    #[cfg(feature = "project_metrics")]
    {
        "tokei，已排除 target/node_modules 等"
    }
    #[cfg(not(feature = "project_metrics"))]
    {
        "内置扩展名统计（未启用 project_metrics/tokei）"
    }
}
