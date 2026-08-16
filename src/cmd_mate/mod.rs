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

struct SplitState {
    words: Vec<String>,
    buf: String,
    /// 当前词是否包含「实质」：非空缓冲，或出现过非空的引号内字符，或出现过空的 `""`/`''` 对。
    word_nonempty: bool,
    phase: Phase,
    double_inner_char: bool,
    single_inner_char: bool,
}

impl SplitState {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            buf: String::new(),
            word_nonempty: false,
            phase: Phase::Outside,
            double_inner_char: false,
            single_inner_char: false,
        }
    }

    fn flush_word(&mut self) {
        if !self.buf.is_empty() || self.word_nonempty {
            self.words.push(std::mem::take(&mut self.buf));
            self.word_nonempty = false;
        }
    }

    fn push_literal(&mut self, ch: char) {
        self.buf.push(ch);
        self.word_nonempty = true;
    }

    fn feed_outside(&mut self, ch: char) {
        match ch {
            c if c.is_whitespace() => self.flush_word(),
            '\'' => {
                self.phase = Phase::Single;
                self.single_inner_char = false;
            }
            '"' => {
                self.phase = Phase::Double;
                self.double_inner_char = false;
            }
            '\\' => self.phase = Phase::EscapeOutside,
            c => self.push_literal(c),
        }
    }

    fn feed_single(&mut self, ch: char) {
        if ch == '\'' {
            if !self.single_inner_char {
                self.word_nonempty = true;
            }
            self.phase = Phase::Outside;
        } else {
            self.buf.push(ch);
            self.single_inner_char = true;
            self.word_nonempty = true;
        }
    }

    fn feed_double(&mut self, ch: char) {
        match ch {
            '"' => {
                if !self.double_inner_char {
                    self.word_nonempty = true;
                }
                self.phase = Phase::Outside;
            }
            '\\' => self.phase = Phase::EscapeDouble,
            c => {
                self.buf.push(c);
                self.double_inner_char = true;
                self.word_nonempty = true;
            }
        }
    }

    fn feed_escape_double(&mut self, ch: char) {
        match ch {
            '"' | '\\' | '$' | '`' => {
                self.buf.push(ch);
                self.double_inner_char = true;
                self.word_nonempty = true;
            }
            '\n' => {}
            c => {
                self.buf.push(c);
                self.double_inner_char = true;
                self.word_nonempty = true;
            }
        }
        self.phase = Phase::Double;
    }

    fn feed(&mut self, ch: char) {
        match self.phase {
            Phase::Outside => self.feed_outside(ch),
            Phase::EscapeOutside => {
                self.push_literal(ch);
                self.phase = Phase::Outside;
            }
            Phase::Single => self.feed_single(ch),
            Phase::Double => self.feed_double(ch),
            Phase::EscapeDouble => self.feed_escape_double(ch),
        }
    }

    fn finish(mut self) -> Vec<String> {
        match self.phase {
            Phase::EscapeOutside | Phase::EscapeDouble => {
                self.buf.push('\\');
                self.word_nonempty = true;
            }
            _ => {}
        }
        self.flush_word();
        self.words
    }
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
    let mut state = SplitState::new();
    for ch in input.chars() {
        state.feed(ch);
    }
    state.finish()
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
