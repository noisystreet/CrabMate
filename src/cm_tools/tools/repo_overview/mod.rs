//! 仓库概览聚合：可选项目画像 + 主文档预览 + 源码树 + 构建脚本/清单路径汇总（只读）。

mod parse;
mod sweep;

pub use sweep::repo_overview_sweep;

/// 与 `docs_health_sweep` 默认文档预览列表相同（供文档健康聚合复用）。
pub fn default_health_sweep_doc_paths() -> Vec<String> {
    vec![
        "README.md".to_string(),
        "AGENTS.md".to_string(),
        "docs/开发文档.md".to_string(),
        "docs/配置说明.md".to_string(),
        "docs/命令行与路由.md".to_string(),
    ]
}
