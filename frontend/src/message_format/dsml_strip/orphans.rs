use super::tags::{DSML_CLOSE_ASCII, DSML_CLOSE_FW, DSML_OPEN_ASCII, DSML_OPEN_FW};

pub(super) fn strip_orphan_open_fw(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let rel = match out[scan..].find(DSML_OPEN_FW) {
            Some(r) => r,
            None => break,
        };
        let start = scan + rel;
        let after = &out[start + DSML_OPEN_FW.len()..];
        let mut content_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut advance_one = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                advance_one = true;
                break;
            }
            if c == '>' {
                if content_chars <= 300 {
                    remove_to = Some(start + DSML_OPEN_FW.len() + i + c.len_utf8());
                } else {
                    advance_one = true;
                }
                break;
            }
            content_chars += 1;
            if content_chars > 300 {
                advance_one = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if advance_one {
            scan = start + 1;
        } else {
            scan = start + DSML_OPEN_FW.len();
        }
    }
    out
}

pub(super) fn strip_orphan_close_fw(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let rel = match out[scan..].find(DSML_CLOSE_FW) {
            Some(r) => r,
            None => break,
        };
        let start = scan + rel;
        let after = &out[start + DSML_CLOSE_FW.len()..];
        let mut inner_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut bump_scan = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                bump_scan = true;
                break;
            }
            if c == '>' {
                if (1..=80).contains(&inner_chars) {
                    remove_to = Some(start + DSML_CLOSE_FW.len() + i + c.len_utf8());
                } else {
                    bump_scan = true;
                }
                break;
            }
            inner_chars += 1;
            if inner_chars > 80 {
                bump_scan = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if bump_scan {
            scan = start + 1;
        } else {
            scan = start + DSML_CLOSE_FW.len();
        }
    }
    out
}

pub(super) fn strip_orphan_open_ascii(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let rel = match out[scan..].find(DSML_OPEN_ASCII) {
            Some(r) => r,
            None => break,
        };
        let start = scan + rel;
        let after = &out[start + DSML_OPEN_ASCII.len()..];
        let mut content_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut advance_one = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                advance_one = true;
                break;
            }
            if c == '>' {
                if content_chars <= 300 {
                    remove_to = Some(start + DSML_OPEN_ASCII.len() + i + c.len_utf8());
                } else {
                    advance_one = true;
                }
                break;
            }
            content_chars += 1;
            if content_chars > 300 {
                advance_one = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if advance_one {
            scan = start + 1;
        } else {
            scan = start + DSML_OPEN_ASCII.len();
        }
    }
    out
}

pub(super) fn strip_orphan_close_ascii(s: &str) -> String {
    let mut out = s.to_string();
    let mut scan = 0usize;
    while scan < out.len() {
        let rel = match out[scan..].find(DSML_CLOSE_ASCII) {
            Some(r) => r,
            None => break,
        };
        let start = scan + rel;
        let after = &out[start + DSML_CLOSE_ASCII.len()..];
        let mut inner_chars = 0usize;
        let mut remove_to: Option<usize> = None;
        let mut bump_scan = false;
        for (i, c) in after.char_indices() {
            if c == '\n' {
                bump_scan = true;
                break;
            }
            if c == '>' {
                if (1..=80).contains(&inner_chars) {
                    remove_to = Some(start + DSML_CLOSE_ASCII.len() + i + c.len_utf8());
                } else {
                    bump_scan = true;
                }
                break;
            }
            inner_chars += 1;
            if inner_chars > 80 {
                bump_scan = true;
                break;
            }
        }
        if let Some(end) = remove_to {
            out.replace_range(start..end, "");
            scan = start;
        } else if bump_scan {
            scan = start + 1;
        } else {
            scan = start + DSML_CLOSE_ASCII.len();
        }
    }
    out
}
