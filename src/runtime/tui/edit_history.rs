//! 聊天输入 / Prompt 的撤销与重做（简单快照栈）。

use super::state::TuiState;

const MAX_UNDO: usize = 100;

fn trim_undo_stack<T>(v: &mut Vec<T>) {
    while v.len() > MAX_UNDO {
        v.remove(0);
    }
}

pub(super) fn clear_input_history(state: &mut TuiState) {
    state.input_undo.clear();
    state.input_redo.clear();
}

pub(super) fn clear_prompt_history(state: &mut TuiState) {
    state.prompt_undo.clear();
    state.prompt_redo.clear();
}

pub(super) fn push_input_undo(state: &mut TuiState) {
    state.input_redo.clear();
    state
        .input_undo
        .push((state.input.clone(), state.input_cursor));
    trim_undo_stack(&mut state.input_undo);
}

pub(super) fn push_prompt_undo(state: &mut TuiState) {
    state.prompt_redo.clear();
    state
        .prompt_undo
        .push((state.prompt.clone(), state.prompt_cursor));
    trim_undo_stack(&mut state.prompt_undo);
}

pub(super) fn input_undo(state: &mut TuiState) -> bool {
    let Some((s, c)) = state.input_undo.pop() else {
        return false;
    };
    let cur = (state.input.clone(), state.input_cursor);
    state.input_redo.push(cur);
    state.input = s;
    state.input_cursor = c;
    true
}

pub(super) fn input_redo(state: &mut TuiState) -> bool {
    let Some((s, c)) = state.input_redo.pop() else {
        return false;
    };
    let cur = (state.input.clone(), state.input_cursor);
    state.input_undo.push(cur);
    state.input = s;
    state.input_cursor = c;
    true
}

pub(super) fn prompt_undo(state: &mut TuiState) -> bool {
    let Some((s, c)) = state.prompt_undo.pop() else {
        return false;
    };
    let cur = (state.prompt.clone(), state.prompt_cursor);
    state.prompt_redo.push(cur);
    state.prompt = s;
    state.prompt_cursor = c;
    true
}

pub(super) fn prompt_redo(state: &mut TuiState) -> bool {
    let Some((s, c)) = state.prompt_redo.pop() else {
        return false;
    };
    let cur = (state.prompt.clone(), state.prompt_cursor);
    state.prompt_undo.push(cur);
    state.prompt = s;
    state.prompt_cursor = c;
    true
}
