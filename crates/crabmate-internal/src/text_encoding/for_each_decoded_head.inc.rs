fn drain_pending_decoder_lines<F>(
    line_no: &mut usize,
    pending: &mut String,
    on_line: &mut F,
) -> Result<std::ops::ControlFlow<()>, String>
where
    F: FnMut(usize, &str) -> std::ops::ControlFlow<()>,
{
    while let Some(pos) = pending.find('\n') {
        *line_no += 1;
        let mut line = pending[..pos].to_string();
        if line.ends_with('\r') {
            line.pop();
        }
        pending.drain(..=pos);
        if on_line(*line_no, &line).is_break() {
            return Ok(std::ops::ControlFlow::Break(()));
        }
    }
    Ok(std::ops::ControlFlow::Continue(()))
}

fn emit_decoder_pending_tail_line<F>(line_no: &mut usize, mut pending: String, on_line: &mut F)
where
    F: FnMut(usize, &str) -> std::ops::ControlFlow<()>,
{
    if pending.is_empty() {
        return;
    }
    *line_no += 1;
    if pending.ends_with('\r') {
        pending.pop();
    }
    let _ = on_line(*line_no, &pending);
}
