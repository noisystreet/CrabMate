//! 将单行命令字符串拆成 **argv 词序列**（不经 shell、不展开 `$` / 反引号）。
//!
//! 用于把模型误写在 `command` 整段里的 `prog arg1 …` 规范成 `Command::new(prog).args([…])`。
//! 规则：**类 POSIX 的引号与反斜杠**；引号内外片段**拼接**为同一 argv 词，直到外侧空白。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Outside,
    Single,
    Double,
    EscapeOutside,
    EscapeDouble,
}

/// 将 `input` 拆成若干词（每个词对应一个 `argv` 元素）。
///
/// - 外侧空白：结束当前词（若该词「有内容」——见下）。
/// - `'…'`：单引号内字面量；`''` 表示空片段，可与其它片段拼成一词（如 `x''y` → `xy`）。
/// - `"…"`：双引号内；`\"`、`\\`、`$`、`` ` `` 在双引号内按字面保留下一字符；`\`+换行 吞掉换行（续行）。
/// - 外侧 `\`：下一字符字面进入当前词。
/// - 未闭合引号：读到结尾即结束该词（容错）。
/// - 全空白：返回空 `Vec`。
#[must_use]
pub fn split_command_line(input: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut buf = String::new();
    // 当前词是否包含「实质」：非空缓冲，或出现过非空的引号内字符，或出现过空的 `""`/`''` 对。
    let mut word_nonempty = false;
    let mut phase = Phase::Outside;
    let mut double_inner_char = false;
    let mut single_inner_char = false;

    let flush_word = |words: &mut Vec<String>, buf: &mut String, word_nonempty: &mut bool| {
        if !buf.is_empty() || *word_nonempty {
            words.push(std::mem::take(buf));
            *word_nonempty = false;
        }
    };

    for ch in input.chars() {
        match phase {
            Phase::Outside => match ch {
                c if c.is_whitespace() => {
                    flush_word(&mut words, &mut buf, &mut word_nonempty);
                }
                '\'' => {
                    phase = Phase::Single;
                    single_inner_char = false;
                }
                '"' => {
                    phase = Phase::Double;
                    double_inner_char = false;
                }
                '\\' => phase = Phase::EscapeOutside,
                c => {
                    buf.push(c);
                    word_nonempty = true;
                }
            },
            Phase::EscapeOutside => {
                buf.push(ch);
                word_nonempty = true;
                phase = Phase::Outside;
            }
            Phase::Single => match ch {
                '\'' => {
                    if !single_inner_char {
                        word_nonempty = true;
                    }
                    phase = Phase::Outside;
                }
                c => {
                    buf.push(c);
                    single_inner_char = true;
                    word_nonempty = true;
                }
            },
            Phase::Double => match ch {
                '"' => {
                    if !double_inner_char {
                        word_nonempty = true;
                    }
                    phase = Phase::Outside;
                }
                '\\' => phase = Phase::EscapeDouble,
                c => {
                    buf.push(c);
                    double_inner_char = true;
                    word_nonempty = true;
                }
            },
            Phase::EscapeDouble => {
                match ch {
                    '"' | '\\' | '$' | '`' => {
                        buf.push(ch);
                        double_inner_char = true;
                        word_nonempty = true;
                    }
                    '\n' => {}
                    c => {
                        buf.push(c);
                        double_inner_char = true;
                        word_nonempty = true;
                    }
                }
                phase = Phase::Double;
            }
        }
    }

    match phase {
        Phase::EscapeOutside => {
            buf.push('\\');
            word_nonempty = true;
        }
        Phase::EscapeDouble => {
            buf.push('\\');
            word_nonempty = true;
        }
        _ => {}
    }

    flush_word(&mut words, &mut buf, &mut word_nonempty);
    words
}

#[cfg(test)]
mod tests {
    use super::split_command_line;

    #[test]
    fn splits_unquoted_git_log() {
        assert_eq!(
            split_command_line("git log -5 --oneline"),
            vec!["git", "log", "-5", "--oneline"]
        );
    }

    #[test]
    fn double_quoted_format_keeps_percent_placeholders_one_argv() {
        let w = split_command_line(r#"git log -5 --format="%h %ai %an%n%s%n%b""#);
        assert_eq!(w, vec!["git", "log", "-5", "--format=%h %ai %an%n%s%n%b",]);
    }

    #[test]
    fn single_quoted_format_with_spaces() {
        let w = split_command_line("git log -5 --pretty=format:'%h %ad %an' --date=short");
        assert_eq!(
            w,
            vec![
                "git",
                "log",
                "-5",
                "--pretty=format:%h %ad %an",
                "--date=short",
            ]
        );
    }

    #[test]
    fn pre_commit_embedded() {
        assert_eq!(
            split_command_line("pre-commit run --all-files"),
            vec!["pre-commit", "run", "--all-files"]
        );
    }

    #[test]
    fn echo_hello_world() {
        assert_eq!(
            split_command_line("echo hello world"),
            vec!["echo", "hello", "world"]
        );
    }

    #[test]
    fn echo_empty_double_quotes_second_word() {
        assert_eq!(split_command_line(r#"echo """#), vec!["echo", ""]);
    }

    #[test]
    fn escaped_space_outside_quotes() {
        assert_eq!(
            split_command_line(r#"echo a\ b c"#),
            vec!["echo", "a b", "c"]
        );
    }

    #[test]
    fn trailing_backslash_outside_appends_backslash() {
        assert_eq!(split_command_line("echo hi\\"), vec!["echo", "hi\\"]);
    }

    #[test]
    fn only_whitespace_yields_empty() {
        assert!(split_command_line("   \t  ").is_empty());
    }
}
