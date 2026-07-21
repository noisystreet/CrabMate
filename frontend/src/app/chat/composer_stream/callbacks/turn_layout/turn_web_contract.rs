//! Web 块布局契约：`project_turn_web` → `BubbleOutputQueue` flush → `StoredMessage` 顺序。

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crabmate_turn_layout::{SegmentKind, TurnEvent, project_turn_web};
    use serde::Deserialize;

    use super::super::super::super::turn_canonical::TurnCanonicalState;
    use crate::sse_dispatch::TurnSegmentStartInfo;
    use crate::storage::StoredMessage;
    use crate::storage::StoredMessageState;

    use super::super::bubble_queue::{
        BATCH_NARRATION_ROW_ID, BubbleOutputQueue, FINAL_ANSWER_ROW_ID,
    };

    #[derive(Debug, Deserialize)]
    struct WebGoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect: Vec<crabmate_turn_layout::ProjectedRow>,
        #[serde(default)]
        expect_open_preview: Option<String>,
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/turn_project_web_golden.jsonl")
    }

    fn load_cases() -> Vec<(usize, WebGoldenCase)> {
        let path = fixture_path();
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        raw.lines()
            .enumerate()
            .filter_map(|(line_no, line)| {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    return None;
                }
                let case: WebGoldenCase = serde_json::from_str(t).unwrap_or_else(|e| {
                    panic!(
                        "{}:{}: invalid golden json: {e}\n{t}",
                        path.display(),
                        line_no + 1
                    );
                });
                Some((line_no + 1, case))
            })
            .collect()
    }

    fn apply_event(turn: &mut TurnCanonicalState, ev: TurnEvent) {
        match ev {
            TurnEvent::TimelineAssistant { text } => {
                turn.ingest_pre_tool_commentary(text.as_str());
            }
            TurnEvent::SegmentStart {
                segment_id,
                kind,
                before_tool_call_id,
            } => {
                turn.on_segment_start(TurnSegmentStartInfo {
                    segment_id,
                    kind: match kind {
                        SegmentKind::Commentary => "commentary".to_string(),
                        SegmentKind::Answer => "answer".to_string(),
                    },
                    before_tool_call_id,
                });
            }
            TurnEvent::SegmentDelta {
                segment_id: _,
                delta,
            } => {
                let _ = turn.try_apply_commentary_delta(delta.as_str());
            }
            TurnEvent::SegmentEnd { segment_id } => {
                turn.on_segment_end(segment_id);
            }
            TurnEvent::ToolCall {
                tool_call_id,
                name,
                summary,
            } => {
                turn.on_tool_call(tool_call_id.as_str(), name.as_str(), summary.as_str());
            }
            TurnEvent::ToolPhaseEnd => {
                turn.on_tool_phase_end();
            }
        }
    }

    fn tool_messages_from_projection(turn: &TurnCanonicalState) -> Vec<StoredMessage> {
        project_turn_web(turn.turn_ref())
            .into_iter()
            .filter(|r| r.kind == "tool")
            .map(|r| StoredMessage {
                id: format!("tool-{}", r.tool_call_id.clone().unwrap_or_default()),
                role: "system".into(),
                text: r.text,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: r.tool_call_id,
                tool_name: r.tool_name,
                created_at: 0,
            })
            .collect()
    }

    fn batch_row_index(messages: &[StoredMessage]) -> Option<usize> {
        messages.iter().position(|m| m.id == BATCH_NARRATION_ROW_ID)
    }

    #[test]
    fn golden_turn_web_stored_sync() {
        let path = fixture_path();
        for (line_no, case) in load_cases() {
            let mut turn = TurnCanonicalState::new();
            for ev in case.events {
                apply_event(&mut turn, ev);
            }
            assert_eq!(
                project_turn_web(turn.turn_ref()),
                case.expect,
                "projection drift in case {} at {}:{}",
                case.id,
                path.display(),
                line_no
            );

            let mut messages = tool_messages_from_projection(&turn);
            BubbleOutputQueue.sync_web_projection(&mut messages, &turn, None, None);

            let batch = crabmate_turn_layout::batch_narration_row(turn.turn_ref())
                .expect("case must define batch row when tools exist");
            let batch_idx = batch_row_index(&messages).unwrap_or_else(|| {
                panic!(
                    "case {} at {}:{}: missing turn-batch-narration row",
                    case.id,
                    path.display(),
                    line_no
                )
            });
            assert_eq!(
                messages[batch_idx].text, batch.text,
                "case {} batch text",
                case.id
            );

            if let Some(ref anchor) = batch.tool_call_id {
                let tool_idx = messages
                    .iter()
                    .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(anchor.as_str()))
                    .unwrap_or_else(|| panic!("case {} missing anchor tool {anchor}", case.id));
                assert!(
                    batch_idx < tool_idx,
                    "case {}: batch row must precede anchor tool",
                    case.id
                );
            }

            if let Some(ref preview) = case.expect_open_preview {
                assert_eq!(
                    BubbleOutputQueue::loading_preview_text(&turn, None),
                    preview.as_str(),
                    "case {} open preview",
                    case.id
                );
            }
        }
    }

    /// 真实路径形态 B：无 `turn_segment_*`、仅 plain delta + tool_call；stored 须拆成 batch + tools + final。
    #[test]
    fn real_morph_b_bulk_deltas_stored_block_layout() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_commentary_delta("好的，先看 HPCG 安装说明。"));
        turn.on_tool_call("tc_unpack", "unpack", "unpack");
        assert!(turn.try_apply_commentary_delta("读取 INSTALL 与 Makefile。"));
        turn.on_tool_call("tc_read", "read_file", "read INSTALL");
        turn.on_tool_call("tc_make", "run_command", "make");
        turn.on_tool_phase_end();
        // `try_apply_answer_state_transition` 仅状态转换，终答在 overlay。
        assert!(turn.try_apply_answer_state_transition("HPCG 编译完成。"));

        let batch = crabmate_turn_layout::batch_narration_row(turn.turn_ref()).expect("batch");
        assert!(
            batch.text.contains("好的，先看 HPCG") && batch.text.contains("读取 INSTALL"),
            "batch={}",
            batch.text
        );

        let mut messages = tool_messages_from_projection(&turn);
        let queue = BubbleOutputQueue;
        queue.sync_web_projection(&mut messages, &turn, None, Some("HPCG 编译完成。"));

        let batch_idx = batch_row_index(&messages).expect("batch row");
        let final_idx = messages
            .iter()
            .position(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("final row");
        let first_tool = messages.iter().position(|m| m.is_tool).expect("tool row");
        assert!(
            batch_idx < first_tool,
            "batch must precede tools: idx batch={batch_idx} tool={first_tool}"
        );
        assert!(
            final_idx > batch_idx,
            "final must follow batch: batch={batch_idx} final={final_idx}"
        );
        assert_eq!(messages[final_idx].text, "HPCG 编译完成。");
        assert!(
            messages
                .iter()
                .filter(|m| m.role == "assistant" && !m.is_tool)
                .count()
                >= 2,
            "expected separate batch + final assistant rows"
        );
    }

    /// open 旁注段撑到 `tool_phase_end` 仍无 `segment_end`：须关段落盘 batch，勿留 overlay 巨泡。
    #[test]
    fn open_commentary_through_tool_phase_end_syncs_batch_before_tools() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_commentary_delta("先看 HPCG 安装说明。"));
        turn.on_tool_call("tc_unpack", "unpack", "unpack");
        assert!(turn.try_apply_commentary_delta("读取 INSTALL 与 Makefile。"));
        turn.on_tool_call("tc_read", "read_file", "read INSTALL");
        turn.on_tool_phase_end();
        assert!(
            crabmate_turn_layout::streaming_commentary_block_text(turn.turn_ref()).is_none(),
            "open preview must be closed after tool_phase_end"
        );

        let batch = crabmate_turn_layout::batch_narration_row(turn.turn_ref()).expect("batch");
        assert!(
            batch.text.contains("先看 HPCG") && batch.text.contains("读取 INSTALL"),
            "batch={}",
            batch.text
        );

        let mut messages = tool_messages_from_projection(&turn);
        let queue = BubbleOutputQueue;
        queue.sync_web_projection(&mut messages, &turn, None, None);

        let batch_idx = batch_row_index(&messages).expect("batch row");
        let first_tool = messages.iter().position(|m| m.is_tool).expect("tool row");
        assert!(
            batch_idx < first_tool,
            "batch must precede tools: batch={batch_idx} tool={first_tool}"
        );
        assert!(
            messages
                .iter()
                .find(|m| m.id == BATCH_NARRATION_ROW_ID)
                .is_some_and(|m| m.text.len() > 20),
            "batch row must hold merged narration, not empty shell"
        );
    }

    /// 真实 LLM 形态 B（`chat_export_20260704_160510` 编译 hpcg 轮）：plain delta + 多工具 + 无 `turn_segment_*`。
    #[test]
    fn real_hpcg_morph_b_export_section_order() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_unpack", "unpack", "unpack hpcg");
        assert!(turn.try_apply_commentary_delta("好的，先解压 HPCG 看看结构。"));
        turn.on_tool_call("tc_list", "list_tree", "list tree");
        assert!(turn.try_apply_commentary_delta("HPCG 源码已解压。"));
        turn.on_tool_call("tc_read", "read_file", "read INSTALL");
        assert!(turn.try_apply_commentary_delta("读取 INSTALL 与 Makefile。"));
        turn.on_tool_call("tc_make", "run_command", "make arch=Linux_Serial");
        assert!(turn.try_apply_commentary_delta("开始编译。"));
        turn.on_tool_phase_end();
        // `try_apply_answer_state_transition` 仅状态转换，终答在 overlay。
        assert!(turn.try_apply_answer_state_transition("HPCG 编译完成。"));

        let mut messages = tool_messages_from_projection(&turn);
        BubbleOutputQueue.sync_web_projection(&mut messages, &turn, None, Some("HPCG 编译完成。"));

        let batch_idx = batch_row_index(&messages).expect("batch row");
        let final_idx = messages
            .iter()
            .position(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("final row");
        let first_tool = messages.iter().position(|m| m.is_tool).expect("tool row");
        let last_tool = messages.iter().rposition(|m| m.is_tool).expect("last tool");
        assert!(batch_idx < first_tool, "batch before tools");
        assert!(final_idx > last_tool, "final after tools");

        let assistant_rows: Vec<_> = messages
            .iter()
            .filter(|m| m.role == "assistant" && !m.is_tool)
            .collect();
        assert_eq!(
            assistant_rows.len(),
            2,
            "export must be batch + final, not mega bubble: {:?}",
            assistant_rows
                .iter()
                .map(|m| (m.id.as_str(), m.text.len()))
                .collect::<Vec<_>>()
        );
        assert!(
            messages[batch_idx].text.contains("先解压 HPCG")
                && messages[batch_idx].text.contains("开始编译"),
            "batch={}",
            messages[batch_idx].text
        );
        assert_eq!(messages[final_idx].text, "HPCG 编译完成。");
    }

    /// 单工具 + 晚于 `tool_call` 的旁注（分析目录轮）：batch 仍须在工具前。
    #[test]
    fn morph_b_late_narration_after_first_tool_batch_before_tool() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_list", "list_tree", "list tree");
        assert!(turn.try_apply_commentary_delta("好的，我来看看当前工作区的情况。"));
        turn.on_tool_phase_end();
        // `try_apply_answer_state_transition` 仅状态转换，终答在 overlay。
        assert!(turn.try_apply_answer_state_transition("当前工作区是一个空目录。"));

        let mut messages = tool_messages_from_projection(&turn);
        BubbleOutputQueue.sync_web_projection(
            &mut messages,
            &turn,
            None,
            Some("当前工作区是一个空目录。"),
        );

        let batch_idx = batch_row_index(&messages).expect("batch");
        let tool_idx = messages.iter().position(|m| m.is_tool).expect("tool");
        assert!(batch_idx < tool_idx);
        assert_eq!(messages[batch_idx].text, "好的，我来看看当前工作区的情况。");
    }

    /// 零工具轮次：流式 delta 累积 → sync_web_projection → FINAL_ANSWER_ROW。
    /// 验证终答完整无重复，且不产生 batch 或 tool 行。
    #[test]
    fn zero_tool_answer_bubble_layout() {
        let mut turn = TurnCanonicalState::new();
        let full_answer = "我具备以下技能，按类别整理如下：\n\n---\n\n### 📁 文件与目录操作\n- `read_file`、`create_file`\n\n---\n\n需要我帮你做什么？可以直接说任务。";
        // `try_apply_answer_state_transition` 仅状态转换，终答在 overlay。
        let chars: Vec<char> = full_answer.chars().collect();
        let third = chars.len() / 3;
        for chunk in [
            &chars[..third],
            &chars[third..2 * third],
            &chars[2 * third..],
        ] {
            let delta: String = chunk.iter().collect();
            assert!(turn.try_apply_answer_state_transition(delta.as_str()));
        }

        let mut messages = tool_messages_from_projection(&turn);
        BubbleOutputQueue.sync_web_projection(&mut messages, &turn, None, Some(full_answer));

        // 不应有 tool 行
        assert!(
            !messages.iter().any(|m| m.is_tool),
            "zero-tool turn must not produce tool rows"
        );
        // 不应有 batch 行
        assert!(
            !messages.iter().any(|m| m.id == BATCH_NARRATION_ROW_ID),
            "zero-tool turn must not produce batch row"
        );
        // 必须有 FINAL_ANSWER_ROW 且内容完整
        let final_idx = messages
            .iter()
            .position(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("zero-tool turn must produce FINAL_ANSWER_ROW");
        let text = &messages[final_idx].text;
        assert!(
            text.contains("文件与目录操作"),
            "FINAL_ANSWER_ROW must contain core content"
        );
        assert!(
            text.contains("需要我帮你做什么？"),
            "FINAL_ANSWER_ROW must contain closing sentence"
        );
        // 验证无加倍：长度不超过原文 1.1 倍
        assert!(
            text.len() <= (full_answer.len() as f64 * 1.1) as usize + 5,
            "FINAL_ANSWER_ROW must not be doubled: len={}, expected≤{}",
            text.len(),
            (full_answer.len() as f64 * 1.1) as usize
        );
    }

    /// 回归测试：post-tool `sync_web_projection` 将完整 overlay answer 写入 FINAL_ANSWER_ROW。
    ///
    /// 若 `drain` 先于 `sync` 调用导致 overlay 被清空，则 `flush_final_answer_row` 读不到
    /// 终答正文，FINAL_ANSWER_ROW 缺失或内容为流式期间的最后一个增量片段。
    #[test]
    fn post_tool_final_answer_row_receives_full_overlay_answer() {
        let mut turn = TurnCanonicalState::new();
        // 模拟工具阶段
        turn.on_tool_call("tc_read", "read_file", "read file");
        assert!(turn.try_apply_commentary_delta("先看看文件内容。"));
        turn.on_tool_phase_end();
        turn.on_tool_phase_end(); // finalize_inner 会再调一次

        // 完整终答正文（模拟 1664 字节的中文回答）
        let full_answer = "这是一个小型 C++ CMake 项目，已构建完成且全部测试通过。项目包含主程序 hello.cpp、测试文件 test_hello.cpp 和 README 文档，使用 CMake FetchContent 自动拉取 Google Test。\n\n## 核心功能\n\n`hello` 命令行问候工具支持 --name、--count、--help 三个选项，测试用例 6/6 全部通过（22 ms）。\n\n有什么需要我帮忙的吗？比如添加新功能、重构、或分析代码质量。";
        // `try_apply_answer_state_transition` 仅状态转换，终答在 overlay。
        assert!(turn.try_apply_answer_state_transition(full_answer));

        let mut messages = tool_messages_from_projection(&turn);
        BubbleOutputQueue.sync_web_projection(&mut messages, &turn, None, Some(full_answer));

        let final_text = messages
            .iter()
            .find(|m| m.id == FINAL_ANSWER_ROW_ID)
            .map(|m| m.text.as_str())
            .unwrap_or("<missing>");
        assert_eq!(
            final_text, full_answer,
            "FINAL_ANSWER_ROW must contain complete overlay answer, not partial deltas"
        );
    }

    /// 回归测试：overlay 被消费后（`overlay_answer = None`），`flush_final_answer_row` 不产生
    /// FINAL_ANSWER_ROW——防止用空的 canonical 或旧增量片段覆盖已有完整行。
    #[test]
    fn flush_final_answer_row_skips_when_overlay_empty() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_read", "read_file", "read file");
        assert!(turn.try_apply_commentary_delta("先看看文件内容。"));
        turn.on_tool_phase_end();
        turn.on_tool_phase_end();
        assert!(turn.try_apply_answer_state_transition("完整终答。"));

        // 先正常投影，创建完整的 FINAL_ANSWER_ROW
        let mut messages = tool_messages_from_projection(&turn);
        let full_answer = "完整终答。";
        BubbleOutputQueue.sync_web_projection(&mut messages, &turn, None, Some(full_answer));
        assert!(
            messages
                .iter()
                .any(|m| m.id == FINAL_ANSWER_ROW_ID && m.text == full_answer),
            "first sync must create FINAL_ANSWER_ROW"
        );

        // 模拟 drain 后 overlay 已空：第二次 sync 不应覆盖已有完整行
        let final_before = messages
            .iter()
            .find(|m| m.id == FINAL_ANSWER_ROW_ID)
            .unwrap()
            .text
            .clone();
        BubbleOutputQueue.sync_web_projection(
            &mut messages,
            &turn,
            None,
            None, // overlay 已被 drain 消费
        );
        let final_after = messages
            .iter()
            .find(|m| m.id == FINAL_ANSWER_ROW_ID)
            .map(|m| m.text.as_str())
            .unwrap_or("<missing>");
        assert_eq!(
            final_after, final_before,
            "FINAL_ANSWER_ROW must be preserved when overlay is empty (already drained)"
        );
    }

    /// 回归测试：零工具场景 overlay 已被清空（如 `already_visible=true` 跳过 final_response），
    /// `drain` 后 loading 尾泡仍有正文时，`ensure_final_answer_row_from_text` 应补建
    /// FINAL_ANSWER_ROW。
    #[test]
    fn zero_tool_ensure_final_answer_row_from_loading_tail_text() {
        let mut turn = TurnCanonicalState::new();
        // 零工具：无 tool_call，只有 answer 文本（由流式 delta 写入 loading 尾泡）。
        assert!(turn.try_apply_answer_state_transition("终答正文。"));

        let mut messages = tool_messages_from_projection(&turn);
        // 模拟 stream 中 loading 尾泡已有正文（overlay 已清空但 loading text 保留）
        messages.push(StoredMessage {
            id: "loading-tail".into(),
            role: "assistant".into(),
            text: "终答正文。".to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        });

        // FINAL_ANSWER_ROW 尚不存在
        assert!(
            !messages.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
            "FINAL_ANSWER_ROW must not exist before ensure"
        );

        BubbleOutputQueue::ensure_final_answer_row_from_text(
            &mut messages,
            "终答正文。",
            Some("loading-tail"),
        );

        let final_row = messages
            .iter()
            .find(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("FINAL_ANSWER_ROW must be created");
        assert_eq!(final_row.text, "终答正文。");

        // 再次调用不重复创建
        let count_before = messages
            .iter()
            .filter(|m| m.id == FINAL_ANSWER_ROW_ID)
            .count();
        BubbleOutputQueue::ensure_final_answer_row_from_text(
            &mut messages,
            "终答正文。",
            Some("loading-tail"),
        );
        let count_after = messages
            .iter()
            .filter(|m| m.id == FINAL_ANSWER_ROW_ID)
            .count();
        assert_eq!(
            count_after, count_before,
            "ensure_final_answer_row_from_text must be idempotent"
        );
    }

    /// 回归测试：零工具 `ensure_final_answer_row_from_text` 在 loading 文本为空时
    /// 不应创建空的 FINAL_ANSWER_ROW。
    #[test]
    fn zero_tool_ensure_skips_on_empty_text() {
        let mut messages: Vec<StoredMessage> =
            tool_messages_from_projection(&TurnCanonicalState::new());
        BubbleOutputQueue::ensure_final_answer_row_from_text(
            &mut messages,
            "",
            Some("loading-tail"),
        );
        assert!(
            !messages.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
            "empty text must not create FINAL_ANSWER_ROW"
        );
    }
}
