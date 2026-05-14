//! 剥离常见 ANSI 转义（CSI / OSC），便于 Web 上以纯文本展示伪终端输出。

pub(crate) fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut it = input.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.peek() {
            Some('[') => {
                it.next();
                for ch in it.by_ref() {
                    if matches!(ch, '\x40'..='\x7e') {
                        break;
                    }
                }
            }
            Some(']') => {
                it.next();
                while let Some(ch) = it.next() {
                    if ch == '\x07' {
                        break;
                    }
                    if ch == '\x1b' && matches!(it.peek(), Some('\\')) {
                        it.next();
                        break;
                    }
                }
            }
            Some('(') | Some(')') => {
                it.next();
                let _ = it.next();
            }
            Some(_) => {
                let _ = it.next();
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::strip_ansi_codes;

    #[test]
    fn strip_csi_color() {
        let s = "a\x1b[31mred\x1b[0mb".to_string();
        assert_eq!(strip_ansi_codes(&s), "aredb");
    }

    #[test]
    fn strip_osc_title_bel() {
        let s = "x\x1b]0;title\x07y".to_string();
        assert_eq!(strip_ansi_codes(&s), "xy");
    }

    #[test]
    fn strip_osc_st() {
        let s = "pre\x1b]8;;https://x.example\x1b\\post";
        assert_eq!(strip_ansi_codes(s), "prepost");
    }

    #[test]
    fn plain_utf8() {
        assert_eq!(strip_ansi_codes("你好\r\n"), "你好\r\n");
    }
}
