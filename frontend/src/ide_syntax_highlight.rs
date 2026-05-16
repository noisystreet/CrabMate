//! IDE 按路径选择语法高亮（Rust / TOML / YAML）。

use crate::ide_rust_highlight::highlight_rust_to_html;
use crate::ide_toml_highlight::highlight_toml_to_html;
use crate::ide_yaml_highlight::highlight_yaml_to_html;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdeSyntaxLang {
    Rust,
    Toml,
    Yaml,
}

#[must_use]
pub fn ide_syntax_lang_for_path(path: Option<&str>) -> Option<IdeSyntaxLang> {
    let p = path?;
    let lower = p.to_ascii_lowercase();
    if lower.ends_with(".rs") || lower.ends_with(".rs.in") {
        return Some(IdeSyntaxLang::Rust);
    }
    if lower.ends_with(".toml") || lower.ends_with(".lock") {
        return Some(IdeSyntaxLang::Toml);
    }
    if lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".yaml.tpl")
        || lower.ends_with(".yml.tpl")
    {
        return Some(IdeSyntaxLang::Yaml);
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
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(ide_syntax_lang_for_path(Some("readme.md")), None);
    }
}
