//! 技能 frontmatter 解析（`---` 之间的 `name:` / `description:`）。
//!
//! 技能文件普遍写成 YAML 块标量（`description: >-` + 缩进长描述），而只取冒号后同一行会把
//! **指示符本身**当成值（UI 上显示成 `>-`），所以这里按块语义聚合：`>` 折叠成空格、
//! `|` 保留换行，并同时支持 chomping / 缩进变体（`>-`、`|2+`）与「冒号后换行接缩进块」的
//! 多行 plain scalar。只支持技能元数据真正用到的子集，不引入 YAML 依赖。

/// 解析技能 frontmatter，返回 `(name, description)`。
///
/// 非 `---` 开头、或块内没有对应键时返回 `None`；调用方负责「description 缺省回退正文首行」。
pub(super) fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = content.lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    let mut idx = 1;
    while idx < lines.len() {
        let t = lines[idx].trim();
        if t == "---" {
            break;
        }
        // 值可能跨多行，命中键后要把续行一并跳过。
        let consumed = if let Some(rest) = t.strip_prefix("name:")
            && name.is_none()
        {
            let (value, consumed) = parse_frontmatter_value(rest, &lines[idx + 1..]);
            name = value;
            consumed
        } else if let Some(rest) = t.strip_prefix("description:")
            && description.is_none()
        {
            let (value, consumed) = parse_frontmatter_value(rest, &lines[idx + 1..]);
            description = value;
            consumed
        } else {
            0
        };
        idx += 1 + consumed;
    }
    (name, description)
}

fn parse_frontmatter_scalar(raw: &str) -> Option<String> {
    let v = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// YAML 块标量指示符（`>` / `|` 及其 chomping、缩进变体，如 `>-`、`|2+`）。
///
/// 返回 `'>'`（折叠：行间折成空格）或 `'|'`（保留换行）；不是块标量则 `None`。
fn block_scalar_style(raw: &str) -> Option<char> {
    let t = raw.trim();
    let mut chars = t.chars();
    let head = chars.next()?;
    if head != '>' && head != '|' {
        return None;
    }
    // 指示符后只允许 chomping（`+` / `-`）与缩进数字，避免把 `>- foo` 之类误判成块头。
    chars
        .all(|c| c == '+' || c == '-' || c.is_ascii_digit())
        .then_some(head)
}

/// 收集 `key:` 之后的缩进续行（YAML 块标量 / 多行 plain scalar 的正文）。
///
/// 空白行算块内（段落分隔），顶格非空行即块结束 —— frontmatter 的 `---` 因此不会被吞掉。
/// 返回 `(行, 被消耗的行数)`：调用方必须把这批续行一并跳过，否则续行里恰好形如
/// `name:` 的文本会被当成另一个键。
fn collect_block_lines<'a>(following: &'a [&'a str]) -> (Vec<&'a str>, usize) {
    let mut out: Vec<&str> = Vec::new();
    for line in following {
        if line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty() {
            out.push(line);
        } else {
            break;
        }
    }
    let consumed = out.len();
    (out, consumed)
}

/// 聚合块标量正文：`|` 保留换行，`>` 折成空格（空行折成段落换行）。
fn join_block_lines(lines: &[&str], style: char) -> String {
    let mut out = String::new();
    let mut pending_blank = 0usize;
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            pending_blank += 1;
            continue;
        }
        if !out.is_empty() {
            if pending_blank == 0 {
                out.push(if style == '|' { '\n' } else { ' ' });
            } else {
                let breaks = if style == '|' { pending_blank + 1 } else { 1 };
                for _ in 0..breaks {
                    out.push('\n');
                }
            }
        }
        out.push_str(t);
        pending_blank = 0;
    }
    out
}

/// 解析 `key:` 之后的值，返回 `(值, 被消耗的续行数)`。
fn parse_frontmatter_value(rest: &str, following: &[&str]) -> (Option<String>, usize) {
    let inline = rest.trim();
    if let Some(style) = block_scalar_style(inline) {
        let (body, consumed) = collect_block_lines(following);
        let value = join_block_lines(&body, style);
        return (Some(value).filter(|v| !v.is_empty()), consumed);
    }
    if !inline.is_empty() {
        return (parse_frontmatter_scalar(inline), 0);
    }
    let (body, consumed) = collect_block_lines(following);
    let folded = join_block_lines(&body, '>');
    (Some(folded).filter(|v| !v.is_empty()), consumed)
}

#[cfg(test)]
mod tests {
    use super::parse_skill_frontmatter;
    use crate::cm_config::skills::{SkillDoc, skill_ui_description};

    /// 真实技能文件的常见写法：`description: >-` + 缩进多行（YAML 折叠块标量）。
    const BLOCK_DESC_SKILL: &str = "---\nname: api-design\ndescription: >-\n  Design and review APIs,\n  including REST/HTTP contracts.\n---\n# API Design\n\nBody.\n";

    #[test]
    fn block_scalar_folds_instead_of_keeping_indicator() {
        let (name, description) = parse_skill_frontmatter(BLOCK_DESC_SKILL);
        assert_eq!(name.as_deref(), Some("api-design"));
        assert_eq!(
            description.as_deref(),
            Some("Design and review APIs, including REST/HTTP contracts.")
        );
        // 回归点：`>-` 曾作为描述整段显示在 `/skills list` 与 Web 补全里。
        let doc = SkillDoc {
            display_path: "api-design/SKILL.md".to_string(),
            content: BLOCK_DESC_SKILL.to_string(),
            name,
            description,
        };
        assert_eq!(
            skill_ui_description(&doc),
            "Design and review APIs, including REST/HTTP contracts."
        );
    }

    #[test]
    fn literal_block_keeps_newlines() {
        let (_, description) = parse_skill_frontmatter(
            "---\nname: x\ndescription: |\n  line one\n  line two\n---\nbody\n",
        );
        assert_eq!(description.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn plain_multiline_and_quoted_single_line() {
        // `key:` 后直接换行接缩进块 = 多行 plain scalar，折成一行。
        let (_, description) =
            parse_skill_frontmatter("---\nname: x\ndescription:\n  folded a\n  folded b\n---\nb\n");
        assert_eq!(description.as_deref(), Some("folded a folded b"));
        // 旧行为保持：单行值与带引号值原样。
        let (_, quoted) =
            parse_skill_frontmatter("---\nname: x\ndescription: \"quoted desc\"\n---\nbody\n");
        assert_eq!(quoted.as_deref(), Some("quoted desc"));
    }

    #[test]
    fn block_scalar_does_not_swallow_keys_or_close() {
        // 顶格 `---` / 其它键结束块，不被吞进描述。
        let (name, description) = parse_skill_frontmatter(
            "---\nname: x\ndescription: >-\n  only desc\nother: 1\n---\nbody\n",
        );
        assert_eq!(name.as_deref(), Some("x"));
        assert_eq!(description.as_deref(), Some("only desc"));
        // 缩进行的 `name:` 属于块正文，不能再被当成 frontmatter 键。
        let (name, description) =
            parse_skill_frontmatter("---\nname: x\ndescription: |\n  name: not a key\n---\nbody\n");
        assert_eq!(name.as_deref(), Some("x"));
        assert_eq!(description.as_deref(), Some("name: not a key"));
    }

    #[test]
    fn empty_block_scalar_falls_back_to_body_first_line() {
        let content = "---\nname: x\ndescription: >-\n---\n# Body title\nmore\n";
        let (name, description) = parse_skill_frontmatter(content);
        assert_eq!(description, None);
        let doc = SkillDoc {
            display_path: "x/SKILL.md".to_string(),
            content: content.to_string(),
            name,
            description,
        };
        assert_eq!(skill_ui_description(&doc), "Body title");
    }
}
