//! 工具迁移测试共用的脚手架（仅 `#[cfg(test)]`）。
//!
//! 目标：让 Legacy 面（`String`）与 Typed 面（`Result<String, ToolError>`）的契约断言
//! 只写一次——正文逐字节一致、失败码 / 分类 / 可重试一致、成功正文含失败字样仍判成功。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::ToolContext;
use crate::cm_config::WebSearchProvider;
use crate::cm_tools::tool_result::{
    ToolError, ToolFailureCategory, parse_legacy_output, ToolResult,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// 进程内唯一的临时工作区目录（调用方负责 `remove_dir_all`）。
pub(crate) fn make_test_dir() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "crabmate_tool_test_{}_{}_{}",
        std::process::id(),
        ts,
        seq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 最小 `ToolContext`：固定工作区、无外部服务、无命令白名单。
pub(crate) fn test_ctx(working_dir: &Path) -> ToolContext<'_> {
    ToolContext {
        cfg: None,
        codebase_semantic_host: None,
        command_max_output_len: 1 << 20,
        weather_timeout_secs: 5,
        allowed_commands: &[],
        working_dir,
        path_scope: crate::cm_types::PathScope::SessionRoot,
        web_search_timeout_secs: 5,
        web_search_provider: WebSearchProvider::Worbrow,
        web_search_api_key: "",
        web_search_max_results: 3,
        web_search_proxy: "",
        http_fetch_allowed_prefixes: &[],
        http_fetch_timeout_secs: 5,
        http_fetch_max_response_bytes: 1024,
        command_timeout_secs: 5,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 0,
        long_term_memory_host: None,
        tool_jobs_host: None,
    }
}

/// 在工作区内写文件（自动创建父目录）。
pub(crate) fn write(ws: &Path, rel: &str, content: &str) {
    let p = ws.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// 断言失败对象的 `code` / `category` / `retryable` 三元组。
pub(crate) fn assert_error(
    err: &ToolError,
    code: &str,
    category: ToolFailureCategory,
    retryable: bool,
) {
    assert_eq!(err.code, code, "message: {}", err.message);
    assert_eq!(err.category, category, "message: {}", err.message);
    assert_eq!(err.retryable, retryable, "message: {}", err.message);
}

/// 参数类失败的固定契约（`invalid_args` / `InvalidInput` / 不可重试）。
pub(crate) fn assert_invalid_input(err: &ToolError) {
    assert_error(err, "invalid_args", ToolFailureCategory::InvalidInput, false);
}

/// 成功路径双目录对比：输入须由调用方在两目录中准备一致。
///
/// String 面与 Typed 面正文必须逐字节一致；Typed 面须返回 `Ok`。返回 Typed 正文。
pub(crate) fn assert_success_parity(
    dir_legacy: &Path,
    dir_typed: &Path,
    string_face: impl FnOnce() -> String,
    typed_face: impl FnOnce() -> Result<String, ToolError>,
) -> String {
    let legacy = string_face();
    let typed = typed_face().unwrap_or_else(|e| {
        panic!(
            "Typed 面应成功，实际失败 [{}/{}]: {}",
            e.code,
            e.category.as_str(),
            e.message
        )
    });
    assert_eq!(
        legacy, typed,
        "成功正文必须与 String 面逐字节一致（legacy={dir_legacy:?}, typed={dir_typed:?}）"
    );
    typed
}

/// 失败路径经主 `run_tool` 往返：Legacy 解析出的错误码须与 Typed 显式码一致。
///
/// 用于验证 `crabmate_tool_output` 头确实覆盖了正文启发式（如正文不含失败词却被判失败）。
pub(crate) fn assert_run_tool_error_code(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
    expected_code: &str,
) -> String {
    let output = super::run_tool(name, args_json, ctx);
    let parsed = parse_legacy_output(name, &output);
    assert!(!parsed.ok, "应判失败: {output}");
    assert_eq!(
        parsed.error_code.as_deref(),
        Some(expected_code),
        "Legacy 解析错误码不一致: {output}"
    );
    output
}

/// 成功正文含「失败」等字样时，经主 `run_tool` 仍须判成功（Typed 收尾注入显式头）。
pub(crate) fn assert_success_with_failure_wording(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> String {
    let output = super::run_tool(name, args_json, ctx);
    let parsed = parse_legacy_output(name, &output);
    assert!(
        parsed.ok,
        "成功正文含失败字样不得被判失败: error_code={:?} {output}",
        parsed.error_code
    );
    assert!(parsed.error_code.is_none(), "{output}");
    output
}

/// 经 [`super::run_tool_result`] 断言成功（附带 `message` 以便失败时定位）。
pub(crate) fn assert_run_tool_result_ok(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> ToolResult {
    let result = super::run_tool_result(name, args_json, ctx);
    assert!(
        result.ok,
        "错误码 {:?} 正文 {}",
        result.error_code, result.message
    );
    assert_eq!(result.error_code, None, "{}", result.message);
    result
}
