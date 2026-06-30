//! IDE 按路径选择语法高亮（Rust / TOML / YAML / C / C++ / Python）。

use crate::ide_c_cpp_highlight::{highlight_c_to_html, highlight_cpp_to_html};
use crate::ide_python_highlight::highlight_python_to_html;
use crate::ide_rust_highlight::highlight_rust_to_html;
use crate::ide_toml_highlight::highlight_toml_to_html;
use crate::ide_yaml_highlight::highlight_yaml_to_html;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdeSyntaxLang {
    Rust,
    Toml,
    Yaml,
    C,
    Cpp,
    Python,
}

#[must_use]
pub fn ide_syntax_lang_for_path(path: Option<&str>) -> Option<IdeSyntaxLang> {
    let lower = path?.to_ascii_lowercase();
    ide_syntax_lang_for_lower_path(&lower)
}

fn path_ends_with_any(lower: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| lower.ends_with(suffix))
}

fn ide_syntax_lang_for_lower_path(lower: &str) -> Option<IdeSyntaxLang> {
    if path_ends_with_any(lower, &[".rs", ".rs.in"]) {
        return Some(IdeSyntaxLang::Rust);
    }
    if path_ends_with_any(lower, &[".toml", ".lock"]) {
        return Some(IdeSyntaxLang::Toml);
    }
    if path_ends_with_any(lower, &[".yaml", ".yml", ".yaml.tpl", ".yml.tpl"]) {
        return Some(IdeSyntaxLang::Yaml);
    }
    // C / C++ 头文件：`.h` 在 C 与 C++ 中通用，按 C++ 高亮（更宽容，覆盖 class/template/namespace）。
    if path_ends_with_any(
        lower,
        &[
            ".cpp", ".cc", ".cxx", ".c++", ".hpp", ".hh", ".hxx", ".h++", ".h",
        ],
    ) {
        return Some(IdeSyntaxLang::Cpp);
    }
    if path_ends_with_any(lower, &[".c", ".i", ".mi"]) {
        return Some(IdeSyntaxLang::C);
    }
    if path_ends_with_any(lower, &[".py", ".pyi", ".pyw", ".py.in"]) {
        return Some(IdeSyntaxLang::Python);
    }
    None
}

#[must_use]
pub fn ide_path_has_syntax_highlight(path: Option<&str>) -> bool {
    ide_syntax_lang_for_path(path).is_some()
}

#[must_use]
pub fn highlight_source_for_path(path: Option<&str>, source: &str) -> String {
    match ide_syntax_lang_for_path(path) {
        Some(IdeSyntaxLang::Rust) => highlight_rust_to_html(source),
        Some(IdeSyntaxLang::Toml) => highlight_toml_to_html(source),
        Some(IdeSyntaxLang::Yaml) => highlight_yaml_to_html(source),
        Some(IdeSyntaxLang::C) => highlight_c_to_html(source),
        Some(IdeSyntaxLang::Cpp) => highlight_cpp_to_html(source),
        Some(IdeSyntaxLang::Python) => highlight_python_to_html(source),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ide_c_cpp_highlight::{CppDialect, highlight_c_cpp_to_html};

    #[test]
    fn detects_languages_by_extension() {
        assert_eq!(
            ide_syntax_lang_for_path(Some("config/default_config.toml")),
            Some(IdeSyntaxLang::Toml)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("docker-compose.yml")),
            Some(IdeSyntaxLang::Yaml)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("src/lib.rs")),
            Some(IdeSyntaxLang::Rust)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("src/main.c")),
            Some(IdeSyntaxLang::C)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("include/foo.h")),
            Some(IdeSyntaxLang::Cpp)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("src/widget.cpp")),
            Some(IdeSyntaxLang::Cpp)
        );
        assert_eq!(
            ide_syntax_lang_for_path(Some("scripts/run.py")),
            Some(IdeSyntaxLang::Python)
        );
        assert_eq!(ide_syntax_lang_for_path(Some("readme.md")), None);
    }

    #[test]
    fn dispatches_to_correct_highlighter() {
        // 仅 smoke：确保不会 panic 并返回带 token 的 HTML。
        assert!(highlight_source_for_path(Some("a.rs"), "fn main() {}").contains("hl-kw"));
        assert!(highlight_source_for_path(Some("a.c"), "int main() {}").contains("hl-kw"));
        assert!(highlight_source_for_path(Some("a.cpp"), "class Foo {};").contains("hl-kw"));
        assert!(highlight_source_for_path(Some("a.py"), "def f():\n    pass\n").contains("hl-kw"));
        assert!(highlight_source_for_path(Some("a.md"), "# hi").is_empty());
    }

    #[test]
    fn c_cpp_dialect_smoke() {
        // 共享扫描器可按方言切换关键字集合。
        assert!(highlight_c_cpp_to_html("int x;", CppDialect::C).contains("hl-kw"));
        assert!(highlight_c_cpp_to_html("class X {};", CppDialect::Cpp).contains("hl-kw"));
    }
}
