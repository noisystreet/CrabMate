//! 终态后业务帧检测（#1333 B4/P1）：帧角色分类 + 违规计数状态机。
//!
//! 权威语义见 `docs/SSE协议.md`「终态顺序」与 `docs/Turn布局设计.md` §16.6 / §16.7：
//! 终态（`RUN_FINISHED` / `RUN_ERROR`）之后**不应**再有业务帧。本模块只做**观测**
//! （计数 + 日志），不拦帧、不重排、不改变任何下行序列——它是 B4/E4 收缩前的证据来源。
//!
//! 分类同时认两族线形状：
//! - **V2 / AG-UI**（`default_encoder` 的现行输出）：`{"type":"RUN_FINISHED"|"RUN_ERROR",…}`
//! - **V1 兼容形状**（`sse-replay`、旧 replay dump）：`{"v":1,"stream_ended":{…}}`、
//!   `{"error":"…","code":"…"}`
//!
//! 一帧可含多行（[`crate::cm_sse_protocol::sse::encode_message`] 用 `\n` 拼接多个 AG-UI 事件），
//! **任一行**是终态即整帧终态。非 JSON 行（模型正文 delta 直接下发，不经 `SsePayload`）
//! 一律算非终态业务帧。
//!
//! ## 为什么终态要分 `Finished` / `Error` 两类
//!
//! 服务端有既定的「错误 / 取消 / 冲突收尾形状」：先发一帧 `RUN_ERROR`（`STREAM_CANCELLED`、
//! `CONVERSATION_CONFLICT`、agent 错误码），worker 随后再补 `RUN_FINISHED`（见
//! `chat_job_queue::worker::stream_job` 的 `if !stream_ended_sent`）。客户端接受这个顺序，
//! 它不是违规。若把「终态后再来终态」一律当重复，那么**每次取消 / 冲突 / 回合失败**
//! 都会 +1，指标永远干净不了，真信号被淹没。
//!
//! 因此：`Error` 之后的 `Finished` 记 [`TerminalOrderWatch`] 的 `error_then_finished`（只观测、
//! 不算违规，留给 P3/P4 收敛时取数据）；`Finished` 之后的任何终态、以及 `Error` 之后的
//! `Error` 才是 `repeated_terminal`。

use serde_json::Value;

/// 单帧（可能是多行 payload）在终态序里的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseFrameRole {
    /// `RUN_FINISHED` / V1 `stream_ended`：回合正常结束。
    Finished,
    /// `RUN_ERROR` / V1 带非空 `code` 的 `error`：回合异常结束。
    Error,
    /// 其余一切（含正文 delta、工具帧、`conversation_saved`、`STATE_SNAPSHOT`）。
    ///
    /// **有意从宽**：§16.7 验收 1 要求终态是「最后一条业务事件」，`sse_capabilities` /
    /// `STATE_SNAPSHOT` 之类同样是业务帧。若将来确有终态后必须存在的帧，须先在
    /// `docs/SSE协议.md` 写明再改这里，否则门禁会失去意义。
    NonTerminal,
}

impl SseFrameRole {
    /// 是否为终态（`Finished` / `Error`）。
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, SseFrameRole::Finished | SseFrameRole::Error)
    }
}

/// 一帧的观测结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalOrderObservation {
    /// 终态之后又来了业务帧（§16.7 验收 1 被破坏）。
    pub event_after_terminal: bool,
    /// 真重复终态：`Finished` 之后的任意终态，或 `Error` 之后的 `Error`。
    pub repeated_terminal: bool,
    /// 既有收尾形状：`Error` 之后的 `Finished`。**不算违规**，单独计数。
    pub error_then_finished: bool,
    /// 本帧是否为**首次**违规——调用方据此只 `warn` 一次，避免逐帧刷屏。
    pub first_after_terminal: bool,
}

/// 单条流的累计报告（与 `event_after_terminal_count` 指标同名同义）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalOrderReport {
    /// 是否见过终态帧。
    pub terminal_seen: bool,
    /// `event_after_terminal_count`：终态后业务帧条数。关闭 B4 要求一个版本区间恒为 0。
    pub event_after_terminal: u64,
    /// 真重复终态条数（§16.6 告警里的「`on_done` 重复」上溯信号）。
    pub repeated_terminal: u64,
    /// `Error` → `Finished` 的既有收尾形状条数。**不计入违规**，只用于 P3/P4 取数。
    pub error_then_finished: u64,
}

impl TerminalOrderReport {
    /// 无违规（`event_after_terminal_count == 0 && repeated_terminal == 0`）。
    ///
    /// `error_then_finished` **不参与**：那是设计内的收尾顺序，清不掉也不该清。
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.event_after_terminal == 0 && self.repeated_terminal == 0
    }
}

/// 按终态序规则分类一帧 payload。
///
/// 热路径安全：正文 delta 每 token 一帧，绝大多数帧连 JSON 都不是，先用子串预筛挡住。
#[must_use]
pub fn classify_sse_frame_role(payload: &str) -> SseFrameRole {
    if !payload_may_be_terminal(payload) {
        return SseFrameRole::NonTerminal;
    }
    for line in payload.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if let Some(role) = line_terminal_role(line) {
            return role;
        }
    }
    SseFrameRole::NonTerminal
}

/// 廉价预筛：只有可能承载终态的帧才值得做 JSON 解析。
fn payload_may_be_terminal(payload: &str) -> bool {
    payload.contains('{')
        && (payload.contains("stream_ended")
            || payload.contains("RUN_FINISHED")
            || payload.contains("RUN_ERROR")
            || payload.contains("\"code\""))
}

/// 单行 JSON 的终态角色；非终态返回 `None`。
///
/// 只看**顶层键**：正文 delta 里出现 `RUN_FINISHED` 字样、或工具输出里嵌 `{"code":…}`
/// 都不算（它们不是顶层 `type` / `stream_ended` / `error`）。
fn line_terminal_role(line: &str) -> Option<SseFrameRole> {
    let v: Value = serde_json::from_str(line).ok()?;
    let obj = v.as_object()?;
    if let Some(type_str) = obj.get("type").and_then(Value::as_str) {
        return match type_str {
            "RUN_FINISHED" => Some(SseFrameRole::Finished),
            "RUN_ERROR" => Some(SseFrameRole::Error),
            _ => None,
        };
    }
    if obj.contains_key("stream_ended") {
        return Some(SseFrameRole::Finished);
    }
    // V1 错误帧：`error` 与 `code` 均非空才算协议终止。仅 `{"error":…}` 与模型正文里的
    // 示例 JSON 同形，`sse::line` 的分类器同样不把它当协议错误，这里保持一致。
    if non_empty_str(obj.get("error")) && non_empty_str(obj.get("code")) {
        return Some(SseFrameRole::Error);
    }
    None
}

fn non_empty_str(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// 单条流的终态序状态机：吃帧、报违规。纯逻辑，可直接被 golden 驱动。
#[derive(Debug, Default)]
pub struct TerminalOrderWatch {
    /// 首次见到的终态种类。**只记第一次**：`Finished` 之后再来的终态一律按重复处理。
    seen_terminal: Option<SseFrameRole>,
    event_after_terminal: u64,
    repeated_terminal: u64,
    error_then_finished: u64,
}

impl TerminalOrderWatch {
    /// 观测一帧；返回本帧造成的计数增量。
    pub fn observe(&mut self, payload: &str) -> TerminalOrderObservation {
        self.observe_role(classify_sse_frame_role(payload))
    }

    /// 同 [`Self::observe`]，但由调用方已分类（省一次分类）。
    pub fn observe_role(&mut self, role: SseFrameRole) -> TerminalOrderObservation {
        let mut obs = TerminalOrderObservation::default();
        if !role.is_terminal() {
            if self.seen_terminal.is_some() {
                obs.first_after_terminal = self.event_after_terminal == 0;
                self.event_after_terminal += 1;
                obs.event_after_terminal = true;
            }
            return obs;
        }
        match self.seen_terminal {
            None => self.seen_terminal = Some(role),
            // 取消 / 冲突 / 失败路径的既有收尾形状：`RUN_ERROR` 后补 `RUN_FINISHED`。
            Some(SseFrameRole::Error) if role == SseFrameRole::Finished => {
                self.error_then_finished += 1;
                obs.error_then_finished = true;
            }
            Some(_) => {
                self.repeated_terminal += 1;
                obs.repeated_terminal = true;
            }
        }
        obs
    }

    /// 当前累计。
    #[must_use]
    pub fn report(&self) -> TerminalOrderReport {
        TerminalOrderReport {
            terminal_seen: self.seen_terminal.is_some(),
            event_after_terminal: self.event_after_terminal,
            repeated_terminal: self.repeated_terminal,
            error_then_finished: self.error_then_finished,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SseFrameRole, TerminalOrderWatch, classify_sse_frame_role};
    use std::path::PathBuf;

    /// 反向门禁（#1333 P1）：`fixtures/sse_terminal_order_golden.jsonl` 给出**帧序列**与
    /// 期望计数。与 `golden_ag_ui_classify_matches_expected` 互补——那个只看单帧分类，
    /// 这个看**顺序**。
    ///
    /// 列：`描述 \t 帧 JSON 数组 \t event_after_terminal \t repeated_terminal \t error_then_finished`。
    #[test]
    fn golden_terminal_order_matches_expected() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("fixtures/sse_terminal_order_golden.jsonl");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut checked = 0usize;
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = t.splitn(5, '\t').collect();
            assert_eq!(
                parts.len(),
                5,
                "{}:{}: expected 5 tab columns, got {}",
                path.display(),
                line_no + 1,
                parts.len(),
            );
            let frames: Vec<String> = serde_json::from_str(parts[1].trim()).unwrap_or_else(|e| {
                panic!(
                    "{}:{}: frames must be a JSON string array ({e}): {}",
                    path.display(),
                    line_no + 1,
                    parts[1]
                )
            });
            let want: Vec<u64> = parts[2..5]
                .iter()
                .map(|c| {
                    c.trim().parse::<u64>().unwrap_or_else(|e| {
                        panic!(
                            "{}:{}: bad expected count ({e})",
                            path.display(),
                            line_no + 1
                        )
                    })
                })
                .collect();
            let mut watch = TerminalOrderWatch::default();
            for frame in &frames {
                let _ = watch.observe(frame);
            }
            let got = watch.report();
            let got_counts = [
                got.event_after_terminal,
                got.repeated_terminal,
                got.error_then_finished,
            ];
            assert_eq!(
                got_counts,
                want.as_slice(),
                "{}:{}: counts mismatch\n  desc: {}\n  frames: {frames:?}\n  \
                 (event_after_terminal, repeated_terminal, error_then_finished)",
                path.display(),
                line_no + 1,
                parts[0],
            );
            checked += 1;
        }
        assert!(checked >= 12, "golden 至少要有 12 条用例，实际 {checked}");
    }

    #[test]
    fn ag_ui_terminal_roles() {
        assert_eq!(
            classify_sse_frame_role(r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r"}"#),
            SseFrameRole::Finished
        );
        assert_eq!(
            classify_sse_frame_role(r#"{"type":"RUN_ERROR","message":"x"}"#),
            SseFrameRole::Error
        );
        assert_eq!(
            classify_sse_frame_role(r#"{"type":"TEXT_MESSAGE_CONTENT","delta":"hi"}"#),
            SseFrameRole::NonTerminal
        );
    }

    #[test]
    fn v1_shapes_are_recognized() {
        assert_eq!(
            classify_sse_frame_role(r#"{"v":1,"stream_ended":{"job_id":1,"reason":"completed"}}"#),
            SseFrameRole::Finished
        );
        assert_eq!(
            classify_sse_frame_role(r#"{"v":1,"error":"失败","code":"UPSTREAM"}"#),
            SseFrameRole::Error
        );
        // 无 `code` 的 error JSON 与模型正文示例同形，不算协议终止。
        assert_eq!(
            classify_sse_frame_role(r#"{"error":"division by zero"}"#),
            SseFrameRole::NonTerminal
        );
    }

    /// 正文里出现终态字面量、工具输出内嵌 `{"code":…}` 都不能被误判成终态。
    #[test]
    fn nested_and_quoted_lookalikes_are_not_terminal() {
        let delta = r#"{"type":"TEXT_MESSAGE_CONTENT","delta":"协议要求先发 RUN_FINISHED"}"#;
        assert_eq!(classify_sse_frame_role(delta), SseFrameRole::NonTerminal);
        let tool = r#"{"type":"TOOL_CALL_RESULT","content":"{\"code\":\"E1\"}"}"#;
        assert_eq!(classify_sse_frame_role(tool), SseFrameRole::NonTerminal);
    }

    /// 裸文本 delta（非 JSON）是业务帧。
    #[test]
    fn plain_text_delta_is_non_terminal() {
        assert_eq!(
            classify_sse_frame_role("正在读取文件…"),
            SseFrameRole::NonTerminal
        );
        assert_eq!(classify_sse_frame_role("[DONE]"), SseFrameRole::NonTerminal);
    }

    /// 一帧多行：任一行终态即整帧终态（`encode` 用 `\n` 拼接多个 AG-UI 事件）。
    #[test]
    fn multiline_payload_is_terminal_if_any_line_is() {
        let payload = "{\"type\":\"TEXT_MESSAGE_END\",\"messageId\":\"m\"}\n\
                       {\"type\":\"RUN_FINISHED\",\"threadId\":\"t\",\"runId\":\"r\"}";
        assert_eq!(classify_sse_frame_role(payload), SseFrameRole::Finished);
    }

    /// **回归**：取消 / 冲突 / 失败路径的 `RUN_ERROR` → `RUN_FINISHED` 是既有收尾形状，
    /// 不能算重复终态——否则每次非成功回合都会 +1，指标永远干净不了。
    #[test]
    fn error_then_finished_is_not_a_violation() {
        let mut watch = TerminalOrderWatch::default();
        let _ = watch.observe(r#"{"type":"RUN_ERROR","code":"STREAM_CANCELLED"}"#);
        let follow = watch.observe(r#"{"type":"RUN_FINISHED"}"#);
        assert!(follow.error_then_finished);
        assert!(!follow.repeated_terminal && !follow.event_after_terminal);
        let report = watch.report();
        assert_eq!(report.error_then_finished, 1);
        assert_eq!(report.repeated_terminal, 0);
        assert!(report.is_clean(), "既有收尾形状不应让指标变脏");
    }

    /// `Finished` 之后再来的任何终态才算真重复。
    #[test]
    fn finished_then_any_terminal_is_repeated() {
        for second in [
            r#"{"type":"RUN_FINISHED"}"#,
            r#"{"type":"RUN_ERROR","message":"迟到的错误"}"#,
        ] {
            let mut watch = TerminalOrderWatch::default();
            let _ = watch.observe(r#"{"type":"RUN_FINISHED"}"#);
            let dup = watch.observe(second);
            assert!(dup.repeated_terminal, "second={second}");
            assert!(!dup.event_after_terminal);
            assert_eq!(watch.report().repeated_terminal, 1);
            assert!(!watch.report().is_clean());
        }
    }

    /// `Error` 后又是 `Error` 仍是重复。
    #[test]
    fn error_then_error_is_repeated() {
        let mut watch = TerminalOrderWatch::default();
        let _ = watch.observe(r#"{"type":"RUN_ERROR","code":"A"}"#);
        let dup = watch.observe(r#"{"type":"RUN_ERROR","code":"B"}"#);
        assert!(dup.repeated_terminal);
        assert_eq!(watch.report().repeated_terminal, 1);
        assert_eq!(watch.report().error_then_finished, 0);
    }

    #[test]
    fn first_violation_only_flags_once() {
        let mut watch = TerminalOrderWatch::default();
        assert!(
            !watch
                .observe(r#"{"type":"RUN_FINISHED"}"#)
                .event_after_terminal
        );
        let first = watch.observe("仍在输出的正文");
        assert!(first.event_after_terminal && first.first_after_terminal);
        let second = watch.observe("又一帧");
        assert!(second.event_after_terminal && !second.first_after_terminal);
        assert_eq!(watch.report().event_after_terminal, 2);
    }

    #[test]
    fn frames_before_terminal_never_count() {
        let mut watch = TerminalOrderWatch::default();
        for frame in [
            r#"{"type":"RUN_STARTED"}"#,
            "delta",
            r#"{"type":"TOOL_CALL_START"}"#,
            r#"{"type":"RUN_FINISHED"}"#,
        ] {
            let _ = watch.observe(frame);
        }
        assert!(watch.report().is_clean());
    }
}
