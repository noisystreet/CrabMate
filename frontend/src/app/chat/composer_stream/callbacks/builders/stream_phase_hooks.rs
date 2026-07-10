//! 将「同步 [`super::super::super::stream_control_reducer`]」的闭包移出 `assemble` 中 `build_chat_stream_callbacks`，避免其 `nloc` 顶穿棘轮。

use std::rc::Rc;

use crate::app::stream_shell_busy::StreamShellBusyOp;
use crate::conversation_hydrate::TiktokenPromptTokensSnapshot;
use crate::conversation_prompt_tokens_apply::apply_conversation_prompt_tokens_from_sse;

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;
use super::super::super::shell_abort::clear_abort_slot;
use super::super::super::stream_control_reducer::StreamControlEvent;

pub(in super::super) fn make_on_tool_status_with_stream_phase(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(bool)> {
    Rc::new(move |b: bool| {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.scratch.apply_stream_control_event(
            &stream_ctx.shell.stream,
            StreamControlEvent::ToolRunning(b),
        );
        stream_ctx
            .shell
            .stream
            .apply_busy_op(StreamShellBusyOp::MirrorToolRunning(b));
    })
}

pub(in super::super) fn make_on_stream_ended_with_stream_phase(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(String, Option<TiktokenPromptTokensSnapshot>)> {
    Rc::new(
        move |reason: String, tiktoken: Option<TiktokenPromptTokensSnapshot>| {
            if stream_ctx.is_stale() {
                return;
            }
            stream_ctx.scratch.apply_stream_control_event(
                &stream_ctx.shell.stream,
                StreamControlEvent::StreamEnded,
            );
            accum.set_stream_end_reason(reason.clone());
            stream_ctx.chat.clear_stream_resume_handles();
            if let (Some(snap), Some(cid)) =
                (tiktoken, stream_ctx.server_conversation_id_for_tokens())
            {
                apply_conversation_prompt_tokens_from_sse(stream_ctx.chat, &cid, snap);
            }
            // `stream_ended` 表示服务端已结束本轮流式任务：无论 `reason` 是否能解析为已知枚举，
            // 都应回落 busy，避免状态栏长期停在「模型生成中」。（未知 reason 仍写入 stream_end_reason 供 diagnostics。）
            stream_ctx
                .shell
                .stream
                .apply_release_turn_and_stream_run(stream_ctx.attach_generation);
            clear_abort_slot(&stream_ctx.shell);
        },
    )
}

pub(in super::super) fn make_on_assistant_answer_phase_with_stream_phase(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.scratch.apply_stream_control_event(
            &stream_ctx.shell.stream,
            StreamControlEvent::AssistantAnswerPhase,
        );
        // 重复 answer_phase 将车道切入 PendingFollowup；轮换由 `on_delta` / `on_done` 消费。
        stream_ctx.scratch.on_assistant_answer_phase();
    })
}
