//! 金样 fixture 的公共读取工具（`#[cfg(test)]`）。
//!
//! 8 个 golden 测试原先各自手抄同一段「读文件 → 逐行 `trim` → 跳过空行与 `#`
//! 注释 → `serde_json` 解析 → 带 `文件:行号` 的失败文案」，另有 3 份 `line_ctx`
//! 与 2 份 `repo_root` 副本。副本各自漂移的风险高于收益，故集中于此。语义与
//! 失败文案保持不变（唯一例外见 [`for_each_jsonl_record`]）。

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::cm_types::Message;

/// 仓库根。本 crate 就是 workspace 根，故 `CARGO_MANIFEST_DIR` 即所需路径。
#[must_use]
pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 金样断言的上下文串：`<path>:<1-based 行号> (<id>)`。
///
/// 断言失败时靠它定位「哪个 fixture 的第几行、哪条用例」，因此格式被各金样共用。
#[must_use]
pub(crate) fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

/// 逐条读取 JSONL 金样，对每条记录回调 `(path, 0-based line_no, record)`。
///
/// `rel_path` 相对仓库根（如 `fixtures/xxx.jsonl`）。空行（`trim` 后）与 `#`
/// 开头的行跳过；解析失败立即 panic，并打印 `path:line: invalid json` 与原始行，
/// 便于改 fixture 时定位。
///
/// 与旧副本的**唯一**差异：`turn_completion_golden` 过去用
/// `expect("parse golden line")`，现在统一成上面这条可诊断文案。
pub(crate) fn for_each_jsonl_record<T, F>(rel_path: &str, mut f: F)
where
    T: DeserializeOwned,
    F: FnMut(&Path, usize, T),
{
    let path = repo_root().join(rel_path);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let row: T = serde_json::from_str(trimmed).unwrap_or_else(|e| {
            panic!(
                "{}:{}: invalid json: {e}\n{trimmed}",
                path.display(),
                line_no + 1
            )
        });
        f(&path, line_no, row);
    }
}

/// 测试用最小 `Message`：`role` 由调用方给字面量，其余可选字段一律 `None`。
///
/// 注意与 [`Message::tool_result`] 的区别：本函数**不**设 `tool_call_id`，用于
/// 只关心 `role` + 正文的历史构造（会话导出、完成判定等金样）。
///
/// 此前 6 个文件各抄了一份同名同义的副本（`session_export` / `turn_completion` /
/// `turn_completion_golden` / `task_level_evidence::verify` / `cm_chat_export::{schema,markdown}`），
/// 任一字段改动都要改 6 处，故收敛于此。
pub(crate) fn msg(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: Some(text.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

/// 测试用工具结果消息：正文按 `tool_result` 信封 v1 编码，`tool_call_id` 保持 `None`。
///
/// `summary` 单独给，是因为完成判定要区分「摘要」与「原始输出」；此前 3 份副本里
/// 有一份写死 `summary = name`，调用方补同一个值即可。
pub(crate) fn tool_env(name: &str, summary: &str, output: &str) -> Message {
    let parsed = crate::cm_tools::tool_result::parse_tool_output(name, output);
    msg(
        "tool",
        &crate::cm_tools::tool_result::encode_tool_message_envelope_v1(
            name,
            summary.to_string(),
            &parsed,
            output,
            None,
        ),
    )
}
