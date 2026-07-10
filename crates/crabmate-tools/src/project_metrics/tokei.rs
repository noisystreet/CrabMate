use std::path::Path;

use tokei::LanguageType;

use super::{LangStat, WorkspaceCodeStats};

pub fn gather(root: &Path, excluded: &[&str]) -> WorkspaceCodeStats {
    let config = tokei::Config::default();
    let mut languages = tokei::Languages::new();
    let path_str = root.to_string_lossy().to_string();
    let paths = &[path_str];
    let excluded: Vec<&str> = excluded.to_vec();
    languages.get_statistics(paths, &excluded, &config);

    let mut sorted: Vec<_> = languages
        .iter()
        .filter(|(_, lang)| lang.code > 0 || lang.comments > 0 || lang.blanks > 0)
        .collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.1.code));

    WorkspaceCodeStats {
        languages: sorted
            .into_iter()
            .map(|(lang_type, lang)| LangStat {
                language: language_label(*lang_type),
                files: lang.reports.len(),
                code: lang.code,
                comments: lang.comments,
                blanks: lang.blanks,
            })
            .collect(),
    }
}

fn language_label(t: LanguageType) -> String {
    match t {
        LanguageType::Rust => "Rust".to_string(),
        LanguageType::TypeScript => "TypeScript".to_string(),
        LanguageType::JavaScript => "JavaScript".to_string(),
        LanguageType::Python => "Python".to_string(),
        LanguageType::Go => "Go".to_string(),
        LanguageType::Cpp => "C++".to_string(),
        LanguageType::C => "C".to_string(),
        LanguageType::Css => "CSS".to_string(),
        LanguageType::Html => "HTML".to_string(),
        LanguageType::Json => "JSON".to_string(),
        LanguageType::Markdown => "Markdown".to_string(),
        LanguageType::Toml => "TOML".to_string(),
        _ => format!("{t}"),
    }
}
