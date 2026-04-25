use super::Locale;

// --- 流式 / 停止 ---

pub fn stream_empty_reply(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复)",
        Locale::En => "(No reply)",
    }
}

pub fn stream_empty_reply_no_answer_phase(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复：未进入正文阶段)",
        Locale::En => "(No reply: answer phase not entered)",
    }
}

pub fn stream_empty_reply_no_delta(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复：未收到正文片段)",
        Locale::En => "(No reply: no answer delta received)",
    }
}

pub fn stream_empty_reply_diag_line(
    l: Locale,
    stream_end_reason: Option<&str>,
    answer_phase: bool,
    delta_chars: usize,
) -> String {
    let reason = stream_end_reason.unwrap_or("unknown");
    match l {
        Locale::ZhHans => format!(
            "诊断：stream_ended={reason}, answer_phase={answer_phase}, delta_chars={delta_chars}"
        ),
        Locale::En => format!(
            "Diagnostic: stream_ended={reason}, answer_phase={answer_phase}, delta_chars={delta_chars}"
        ),
    }
}

pub fn timeline_tool_step_started_title(l: Locale, tool_name: &str) -> String {
    match l {
        Locale::ZhHans => format!("准备执行工具: {tool_name}"),
        Locale::En => format!("Preparing tool: {tool_name}"),
    }
}

pub fn timeline_tool_step_finished_title(l: Locale, tool_name: &str) -> String {
    match l {
        Locale::ZhHans => format!("工具执行完成: {tool_name}"),
        Locale::En => format!("Tool finished: {tool_name}"),
    }
}

pub fn stream_stopped_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "\n\n[已停止]",
        Locale::En => "\n\n[Stopped]",
    }
}

pub fn stream_stopped_inline(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已停止",
        Locale::En => "Stopped",
    }
}

pub fn chat_failed_banner(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "对话失败",
        Locale::En => "Chat failed",
    }
}

/// 流式已生成正文但 `stream_ended` 仍为 unknown 时的用户提示首段。
pub fn stream_partial_finalize_missing_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "本轮回复已生成，但流式收尾信号缺失（stream_ended=unknown）。请点击“重试”获取完整收尾。"
        }
        Locale::En => {
            "Reply content was generated, but stream finalization signal is missing (stream_ended=unknown). Click Retry to finish cleanly."
        }
    }
}

pub fn stream_completed_missing_final_summary_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "本轮执行已完成，但最终总结消息缺失。你可以点击“重试”让助手补发最终汇总。"
        }
        Locale::En => {
            "Execution finished, but final summary message is missing. Click Retry to regenerate the final summary."
        }
    }
}

pub fn stream_err_impact_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前回合无法调用模型，助手不会继续生成。",
        Locale::En => "The model call cannot proceed for this turn.",
    }
}

pub fn stream_err_hint_api_key(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "检查右侧设置中的 API Key 是否已填写且有效，然后点击“重试”。",
        Locale::En => "Verify API key in Settings is present and valid, then click Retry.",
    }
}

pub fn stream_err_impact_timeout(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本次请求超时中断，当前回复未完整生成。",
        Locale::En => "The request timed out and the reply is incomplete.",
    }
}

pub fn stream_err_hint_timeout(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "稍后重试，或缩小本次请求范围后再发送。",
        Locale::En => "Retry later or send a narrower request.",
    }
}

pub fn stream_err_impact_generic(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本轮流式对话已中止，后续步骤未执行。",
        Locale::En => "The streaming turn stopped and follow-up steps were skipped.",
    }
}

pub fn stream_err_hint_generic(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "点击“重试”；若仍失败，请补充报错上下文以便排查。",
        Locale::En => "Click Retry; if it persists, share more error context.",
    }
}

pub fn sse_protocol_version_mismatch(l: Locale, server_v: u8, client_v: u8, hint: &str) -> String {
    match l {
        Locale::ZhHans => format!(
            "SSE 协议版本不匹配：服务端 supported_sse_v={server_v}，本页 {client_v} ({hint})"
        ),
        Locale::En => format!(
            "SSE protocol version mismatch: server supported_sse_v={server_v}, this page {client_v} ({hint})"
        ),
    }
}
