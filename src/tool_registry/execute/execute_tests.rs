//! `execute` 模块单元测试（拆出以降低 `execute.rs` 物理行数棘轮）。

use super::super::meta::HandlerLookupTable;
use super::*;
use crate::types::{FunctionCall, ToolCall};

fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "tc_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn read_dir_path_is_external_detects_absolute_and_parent_ref() {
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"/tmp"}"#),
        Some("/tmp".to_string())
    );
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"../secrets"}"#),
        Some("../secrets".to_string())
    );
    assert_eq!(read_dir_path_is_external(r#"{"path":"src"}"#), None);
}

#[tokio::test]
async fn prefetch_parallel_syncdefault_approvals_blocks_external_read_dir_without_channel() {
    let calls = vec![tool_call("read_dir", r#"{"path":"/tmp"}"#)];
    let failures = prefetch_parallel_syncdefault_approvals(
        &calls,
        None,
        None,
        &HandlerLookupTable::default_dispatch(),
    )
    .await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("read_dir".to_string(), r#"{"path":"/tmp"}"#.to_string()))
        .expect("missing failure for external read_dir");
    assert!(msg.contains("需要审批通道"));
}
