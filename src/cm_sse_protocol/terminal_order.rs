//! 终态后业务帧检测（#1333 B4/P1）：帧角色分类 + 违规计数状态机。
//!
//! 权威语义见 `docs/SSE协议.md`「终态顺序」与 `docs/Turn布局设计.md` §16.6 / §16.7：
//! 终态（`RUN_FINISHED` / `RUN_ERROR`）**必须**是最后一条业务帧。本模块只做**观测**
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

use serde_json::Value;

/// 单帧（可能是多行 payload）在终态序里的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseFrameRole {
    /// `RUN_FINISHED` / `RUN_ERROR`：回合结束。
    Terminal,
    /// 其余一切（含正文 delta、工具帧、`conversation_saved`、`STATE_SNAPSHOT`）。
    ///
    /// **有意从宽**：§16.7 验收 1 要求终态是「最后一条业务事件」，`sse_capabilities` /
    /// `STATE_SNAPSHOT` 之类同样是业务帧。若将来确有终态后必须存在的帧，须先在
    /// `docs/SSE协议.md` 写明再改这里，否则门禁会失去意义。
    NonTerminal,
}

/// 一帧的观测结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalOrderObservation {
    /// 终态之后又来了业务帧（§16.7 验收 1 被破坏）。
    pub event_after_terminal: bool,
    /// 终态之后又来了终态（重复终态，单独计数，不与上项混算）。
    pub repeated_terminal: bool,
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
    /// 重复终态条数（§16.6 告警里的「`on_done` 重复」上溯信号）。
    pub repeated_terminal: u64,
}

impl TerminalOrderReport {
    /// 无违规（`event_after_terminal_count == 0 && repeated_terminal == 0`）。
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
        if line.starts_with('{') && line_is_terminal(line) {
            return SseFrameRole::Terminal;
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

/// 单行 JSON 是否终态。只看**顶层键**：正文 delta 里出现 `RUN_FINISHED` 字样、
/// 或工具输出里嵌 `{"code":…}` 都不算（它们不是顶层 `type` / `stream_ended` / `error`）。
fn line_is_terminal(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    let Some(obj) = v.as_object() else {
        return false;
    };
    if let Some(type_str) = obj.get("type").and_then(Value::as_str) {
        return matches!(type_str, "RUN_FINISHED" | "RUN_ERROR");
    }
    if obj.contains_key("stream_ended") {
        return true;
    }
    // V1 错误帧：`error` 与 `code` 均非空才算协议终止。仅 `{"error":…}` 与模型正文里的
    // 示例 JSON 同形，`sse::line` 的分类器同样不把它当协议错误，这里保持一致。
    non_empty_str(obj.get("error")) && non_empty_str(obj.get("code"))
}

fn non_empty_str(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// 单条流的终态序状态机：吃帧、报违规。纯逻辑，可直接被 golden 驱动。
#[derive(Debug, Default)]
pub struct TerminalOrderWatch {
    seen_terminal: bool,
    event_after_terminal: u64,
    repeated_terminal: u64,
}

impl TerminalOrderWatch {
    /// 观测一帧；返回本帧造成的违规增量。
    pub fn observe(&mut self, payload: &str) -> TerminalOrderObservation {
        self.observe_role(classify_sse_frame_role(payload))
    }

    /// 同 [`Self::observe`]，但由调用方已分类（省一次分类）。
    pub fn observe_role(&mut self, role: SseFrameRole) -> TerminalOrderObservation {
        let mut obs = TerminalOrderObservation::default();
        match (self.seen_terminal, role) {
            (false, SseFrameRole::Terminal) => self.seen_terminal = true,
            (false, SseFrameRole::NonTerminal) => {}
            (true, SseFrameRole::Terminal) => {
                self.repeated_terminal += 1;
                obs.repeated_terminal = true;
            }
            (true, SseFrameRole::NonTerminal) => {
                obs.first_after_terminal = self.event_after_terminal == 0;
                self.event_after_terminal += 1;
                obs.event_after_terminal = true;
            }
        }
        obs
    }

    /// 当前累计。
    #[must_use]
    pub fn report(&self) -> TerminalOrderReport {
        TerminalOrderReport {
            terminal_seen: self.seen_terminal,
            event_after_terminal: self.event_after_terminal,
            repeated_terminal: self.repeated_terminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SseFrameRole, TerminalOrderWatch, classify_sse_frame_role};
    use std::path::PathBuf;

    /// 反向门禁（#1333 P1）：`fixtures/sse_terminal_order_golden.jsonl` 给出**帧序列**与
    /// 期望违规数。与 `golden_ag_ui_classify_matches_expected` 互补——那个只看单帧分类，
    /// 这个看**顺序**。列：`描述 \t 帧 JSON 数组 \t 期望 event_after_terminal \t 期望 repeated_terminal`。
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
            let parts: Vec<&str> = t.splitn(4, '\t').collect();
            assert_eq!(
                parts.len(),
                4,
                "{}:{}: expected 4 tab columns, got {}",
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
            let want_after: u64 = parts[2].trim().parse().unwrap_or_else(|e| {
                panic!(
                    "{}:{}: bad expected count ({e})",
                    path.display(),
                    line_no + 1
                )
            });
            let want_repeated: u64 = parts[3].trim().parse().unwrap_or_else(|e| {
                panic!(
                    "{}:{}: bad expected count ({e})",
                    path.display(),
                    line_no + 1
                )
            });
            let mut watch = TerminalOrderWatch::default();
            for frame in &frames {
                let _ = watch.observe(frame);
            }
            let got = watch.report();
            assert_eq!(
                got.event_after_terminal,
                want_after,
                "{}:{}: event_after_terminal_count mismatch\n  desc: {}\n  frames: {frames:?}",
                path.display(),
                line_no + 1,
                parts[0],
            );
            assert_eq!(
                got.repeated_terminal,
                want_repeated,
                "{}:{}: repeated_terminal mismatch\n  desc: {}\n  frames: {frames:?}",
                path.display(),
                line_no + 1,
                parts[0],
            );
            checked += 1;
        }
        assert!(checked >= 8, "golden 至少要有 8 条用例，实际 {checked}");
    }

    #[test]
    fn ag_ui_terminal_roles() {
        assert_eq!(
            classify_sse_frame_role(r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r"}"#),
            SseFrameRole::Terminal
        );
        assert_eq!(
            classify_sse_frame_role(r#"{"type":"RUN_ERROR","message":"x"}"#),
            SseFrameRole::Terminal
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
            SseFrameRole::Terminal
        );
        assert_eq!(
            classify_sse_frame_role(r#"{"v":1,"error":"失败","code":"UPSTREAM"}"#),
            SseFrameRole::Terminal
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
        assert_eq!(classify_sse_frame_role(payload), SseFrameRole::Terminal);
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
    fn repeated_terminal_is_its_own_bucket() {
        let mut watch = TerminalOrderWatch::default();
        let _ = watch.observe(r#"{"type":"RUN_FINISHED"}"#);
        let dup = watch.observe(r#"{"type":"RUN_FINISHED"}"#);
        assert!(dup.repeated_terminal && !dup.event_after_terminal);
        let report = watch.report();
        assert_eq!(report.repeated_terminal, 1);
        assert_eq!(report.event_after_terminal, 0);
        assert!(!report.is_clean());
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
