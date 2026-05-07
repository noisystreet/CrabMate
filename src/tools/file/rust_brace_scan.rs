//! `extract_rust_brace_block` 用的 Rust 源码花括号扫描状态（注释/字符串内不计入配对）。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RustBraceScanState {
    Normal,
    LineComment,
    BlockComment,
    StringLit { escape: bool },
    CharLit { escape: bool },
    RawString { hash_count: usize },
}

/// 自 Normal：进入注释 / 字符串 / 原始字符串；无则 None。
pub(super) fn rust_brace_try_leave_normal_for_literal(
    chars: &[char],
    pos: usize,
) -> Option<(RustBraceScanState, usize)> {
    let ch = *chars.get(pos)?;
    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
        return Some((RustBraceScanState::LineComment, pos + 2));
    }
    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '*' {
        return Some((RustBraceScanState::BlockComment, pos + 2));
    }
    if ch == 'r' || ch == 'R' {
        if pos + 1 < chars.len() && chars[pos + 1] == '"' {
            return Some((RustBraceScanState::RawString { hash_count: 0 }, pos + 2));
        }
        if pos + 1 < chars.len() && chars[pos + 1] == '#' {
            let mut hash_count = 0usize;
            let mut j = pos + 1;
            while j < chars.len() && chars[j] == '#' {
                hash_count += 1;
                j += 1;
            }
            if j < chars.len() && chars[j] == '"' {
                return Some((RustBraceScanState::RawString { hash_count }, j + 1));
            }
        }
    }
    if ch == '"' {
        return Some((RustBraceScanState::StringLit { escape: false }, pos + 1));
    }
    if ch == '\'' {
        return Some((RustBraceScanState::CharLit { escape: false }, pos + 1));
    }
    None
}

/// 在 Normal 下更新花括号栈；若块闭合返回 true（应跳出内层字符循环）。
pub(super) fn rust_brace_stack_on_normal_char(
    ch: char,
    started: &mut bool,
    brace_count: &mut i32,
    line_idx: usize,
    end_line: &mut Option<usize>,
) -> bool {
    if !*started {
        if ch == '{' {
            *started = true;
            *brace_count = 1;
        }
        false
    } else if ch == '{' {
        *brace_count += 1;
        false
    } else if ch == '}' {
        *brace_count -= 1;
        if *brace_count == 0 {
            *end_line = Some(line_idx);
            true
        } else {
            false
        }
    } else {
        false
    }
}

/// 单行扫描共享可变上下文（避免 `rust_brace_scan_step` 形参过多）。
pub(super) struct RustBraceScanCtx<'a> {
    pub line_idx: usize,
    pub chars: &'a [char],
    pub started: &'a mut bool,
    pub brace_count: &'a mut i32,
    pub end_line: &'a mut Option<usize>,
}

/// 单行内扫描一步：更新 `state`/`pos`，或要求结束当前行/字符循环。
pub(super) enum RustBraceLineStep {
    Continue {
        state: RustBraceScanState,
        pos: usize,
    },
    BreakCharLoop,
    BreakLineScan,
}

pub(super) fn rust_brace_scan_step(
    state: RustBraceScanState,
    pos: usize,
    ch: char,
    ctx: &mut RustBraceScanCtx<'_>,
) -> RustBraceLineStep {
    match state {
        RustBraceScanState::Normal => rust_brace_step_normal(pos, ch, ctx),
        RustBraceScanState::LineComment => RustBraceLineStep::BreakLineScan,
        RustBraceScanState::BlockComment => rust_brace_step_block_comment(pos, ch, ctx.chars),
        RustBraceScanState::StringLit { escape } => rust_brace_step_string_lit(escape, pos, ch),
        RustBraceScanState::CharLit { escape } => rust_brace_step_char_lit(escape, pos, ch),
        RustBraceScanState::RawString { hash_count } => {
            rust_brace_step_raw_string(hash_count, pos, ch, ctx.chars)
        }
    }
}

fn rust_brace_step_normal(
    pos: usize,
    ch: char,
    ctx: &mut RustBraceScanCtx<'_>,
) -> RustBraceLineStep {
    if let Some((next, np)) = rust_brace_try_leave_normal_for_literal(ctx.chars, pos) {
        return RustBraceLineStep::Continue {
            state: next,
            pos: np,
        };
    }
    if rust_brace_stack_on_normal_char(ch, ctx.started, ctx.brace_count, ctx.line_idx, ctx.end_line)
    {
        return RustBraceLineStep::BreakCharLoop;
    }
    RustBraceLineStep::Continue {
        state: RustBraceScanState::Normal,
        pos: pos + 1,
    }
}

fn rust_brace_step_block_comment(pos: usize, ch: char, chars: &[char]) -> RustBraceLineStep {
    if ch == '*' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
        RustBraceLineStep::Continue {
            state: RustBraceScanState::Normal,
            pos: pos + 2,
        }
    } else {
        RustBraceLineStep::Continue {
            state: RustBraceScanState::BlockComment,
            pos: pos + 1,
        }
    }
}

fn rust_brace_step_string_lit(escape: bool, pos: usize, ch: char) -> RustBraceLineStep {
    if escape {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::StringLit { escape: false },
            pos: pos + 1,
        };
    }
    if ch == '\\' {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::StringLit { escape: true },
            pos: pos + 1,
        };
    }
    if ch == '"' {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::Normal,
            pos: pos + 1,
        };
    }
    RustBraceLineStep::Continue {
        state: RustBraceScanState::StringLit { escape: false },
        pos: pos + 1,
    }
}

fn rust_brace_step_char_lit(escape: bool, pos: usize, ch: char) -> RustBraceLineStep {
    if escape {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::CharLit { escape: false },
            pos: pos + 1,
        };
    }
    if ch == '\\' {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::CharLit { escape: true },
            pos: pos + 1,
        };
    }
    if ch == '\'' {
        return RustBraceLineStep::Continue {
            state: RustBraceScanState::Normal,
            pos: pos + 1,
        };
    }
    RustBraceLineStep::Continue {
        state: RustBraceScanState::CharLit { escape: false },
        pos: pos + 1,
    }
}

fn rust_brace_step_raw_string(
    hash_count: usize,
    pos: usize,
    ch: char,
    chars: &[char],
) -> RustBraceLineStep {
    if ch == '"' {
        let mut ok = true;
        for k in 0..hash_count {
            if pos + 1 + k >= chars.len() || chars[pos + 1 + k] != '#' {
                ok = false;
                break;
            }
        }
        if ok {
            return RustBraceLineStep::Continue {
                state: RustBraceScanState::Normal,
                pos: pos + 1 + hash_count,
            };
        }
    }
    RustBraceLineStep::Continue {
        state: RustBraceScanState::RawString { hash_count },
        pos: pos + 1,
    }
}
