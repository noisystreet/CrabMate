//! 工具错误正文里**供分类器反查**的前缀 / 标记文案。
//!
//! 这些串是「正文首行 → `error_code`」的**兜底**路径（[`super::classify_error_code`]）。
//! 单独提出来是因为分类器按**子串**匹配：文案改了而分类器没改不会编译失败，只会让该类
//! 错误的 `error_code` 静默退化成 `{tool}_failed`——下游的重试判定与 `failure_category`
//! 随之走错分支，且没有任何报错提示。
//!
//! **新代码请在产生处直接 `ToolError::*_code`，不要往本表加条目**：能从产生处带上码的，
//! 就不该再靠正文反查。本表只覆盖历史遗留的纯 `String` 输出面。
//!
//! ## 绑定强度（不要高估）
//!
//! - **双向绑定**：产生处直接引用同名常量，或由单测断言 `Display` 前缀——改文案必然编译
//!   失败或测试变红。已覆盖 `parse_args`、`write_precheck`、`run_command` 的两条短路说明、
//!   [`super::ToolError::unknown_tool`]、`RunCommandError`。
//! - **仅单向匹配**：[`ERROR_PREFIX`] 这类泛用词散落在几十个产生点，无法逐处替换成常量；
//!   它仍是「约定」，改这个词时要人工扫一遍 [`super::classify_error_code`]。
//!
//! `command_error.rs` 的 `thiserror` `#[error("…")]` 属性只接受字面量格式串，无法直接
//! 引用常量，故那里改用单测把文案与常量绑死（改文案而漏改常量会让测试变红）。

/// 未知工具名（[`super::ToolError::unknown_tool`] 与 `tool_registry` 的派发失败）。
pub(crate) const UNKNOWN_TOOL_PREFIX: &str = "未知工具";

/// 入参 JSON 无法解析（`tools::parse_args::parse_args_json`）。
pub(crate) const INVALID_ARGS_JSON_PREFIX: &str = "参数 JSON 无效:";

/// 入参类型与工具参数结构不匹配（`tools::parse_args::parse_args_from_value`）。
///
/// 注意 `RunCommandError::JsonParse` 用的是全角冒号变体，但两者都以本串开头，
/// `contains` 反查不受冒号形态影响。
pub(crate) const INVALID_ARGS_PREFIX: &str = "参数解析错误";

/// 参数与工具 JSON Schema 不一致（`tool_args_validate`）。
pub(crate) const ARGS_SCHEMA_MISMATCH_PREFIX: &str = "参数与工具 JSON Schema";

/// 写盘前语法校验失败，后接 `（CODE）`（`tools::file::write_precheck`）。
pub(crate) const WRITE_PRECHECK_FAILED_PREFIX: &str = "校验失败（";

/// 命令不在白名单（`RunCommandError::DisallowedCommand`）。
pub(crate) const COMMAND_NOT_ALLOWED_PREFIX: &str = "不允许的命令";

/// 未绑定工作区。
///
/// 工作流侧已带显式 `workspace_not_set` 码（不依赖反查）；本常量用于识别其它仍以纯正文
/// 返回的调用方。
pub(crate) const WORKSPACE_NOT_SET_PREFIX: &str = "未设置工作区";

/// 同命令重复失败的短路说明（`agent_turn` 的 `run_command` 短路）。
pub(crate) const REPEATED_SAME_COMMAND_PREFIX: &str = "检测到同命令重复失败";

/// 同类失败已发生的短路说明（`agent_turn` 的 `run_command` 家族短路）。
pub(crate) const REPEATED_FAILURE_FAMILY_PREFIX: &str = "检测到同类失败已发生";

/// 「超时」字样。
///
/// **只服务 [`super::classify_error_code`] 的「错误码推断」**（把带超时的历史正文映射成
/// `timeout`），**不参与成败判定**：`looks_like_failure` 只认前缀（#1440 K8）。
pub(crate) const TIMEOUT_MARKER: &str = "超时";

/// 工具正文的通用错误前缀。
pub(crate) const ERROR_PREFIX: &str = "错误";

/// 执行失败（子进程 / 外部命令类工具）。
pub(crate) const EXECUTE_FAILED_PREFIX: &str = "执行失败";

/// `apply_patch` 部分应用失败时，body 里**失败区块的行首**（输出用 `成功：` / `失败：`
/// 两段列出文件）。
///
/// **故意不进 [`KNOWN_ERROR_PREFIXES`]**：那张表服务「错误正文**首行**」判定
/// （④ 级与 `ToolError::build` 补前缀），而这里是 body 中间的一行。只由
/// `body_after_header_looks_like_failure` 的逐行扫描使用。
pub(crate) const PATCH_PARTIAL_FAILURE_PREFIX: &str = "失败：";

/// `delete_files` 部分失败时，body 里的失败区块行首（形如 `删除失败（2 个）：…`）。
///
/// 同 [`PATCH_PARTIAL_FAILURE_PREFIX`]：只用于 body 逐行扫描，不进首行前缀表。
pub(crate) const DELETE_FAILED_PREFIX: &str = "删除失败（";

/// `apply_patch` 整体失败时的区块行首（形如 `补丁应用失败：…`）。
///
/// 同 [`PATCH_PARTIAL_FAILURE_PREFIX`]：只用于 body 逐行扫描，不进首行前缀表。
pub(crate) const PATCH_FAILED_PREFIX: &str = "补丁应用失败";

/// 工具在错误正文里**显式嵌入错误码**的标记前缀，形如 `错误[http_timeout]：…`。
///
/// 这是服务端保留协议（见 [`super::extract_error_code_marker`]）：其它工具的错误正文
/// 请勿以 `错误[小写码]` 开头，以免被误提取成 `error_code`。
pub(crate) const ERROR_CODE_MARKER_PREFIX: &str = "错误[";

/// 错误正文首行可能带的**已知前缀**集合（`looks_like_failure` 与 `classify_error_code` 认得）。
///
/// **#1430**：`ToolError` 构造时用它判断「这条正文下游能不能认出是错误」——
/// 认不出就补 [`ERROR_PREFIX`]，让 ④ 级判定只需 `starts_with`，不必依赖
/// `contains("失败")` 的宽口径（那条会把「正文提到失败」的成功输出误判为失败）。
pub(crate) const KNOWN_ERROR_PREFIXES: &[&str] = &[
    ERROR_PREFIX,
    UNKNOWN_TOOL_PREFIX,
    INVALID_ARGS_JSON_PREFIX,
    INVALID_ARGS_PREFIX,
    ARGS_SCHEMA_MISMATCH_PREFIX,
    WRITE_PRECHECK_FAILED_PREFIX,
    COMMAND_NOT_ALLOWED_PREFIX,
    WORKSPACE_NOT_SET_PREFIX,
    REPEATED_SAME_COMMAND_PREFIX,
    REPEATED_FAILURE_FAMILY_PREFIX,
    EXECUTE_FAILED_PREFIX,
];

/// 首行是否已带 [`KNOWN_ERROR_PREFIXES`] 中的任一个（忽略前导空白）。
pub(crate) fn has_known_error_prefix(first_line: &str) -> bool {
    let head = first_line.trim_start();
    KNOWN_ERROR_PREFIXES.iter().any(|p| head.starts_with(p))
}

#[cfg(test)]
mod tests {
    use crate::cm_tool_result::classify_error_code_fixture;

    /// **字面量快照**：每条历史文案都必须反查到具体错误码。
    ///
    /// 输入刻意写成字面量，而**不是** `format!("{CONST}…")`。两边同源时，常量怎么改
    /// 测试都绿 —— 那就完全抓不到「改文案忘改分类器」这件事，而这正是本模块存在的理由。
    /// 用字面量后：常量一改，分类器跟着用新常量去匹配这些旧文案就匹配不上，退化成
    /// `{tool}_failed` 兜底，测试立刻红。
    ///
    /// 改动文案的人只有两种选择：同步改这里的快照（说明他看到了分类器），或者看着测试红。
    #[test]
    fn historical_error_text_reverse_maps_to_its_code() {
        let cases: &[(&str, &str)] = &[
            ("未知工具：nope", "unknown_tool"),
            ("参数 JSON 无效: expected value", "invalid_args"),
            ("参数解析错误：missing field `path`", "invalid_args"),
            ("参数与工具 JSON Schema 不一致：x", "invalid_args"),
            (
                "校验失败（PY_SYNTAX_ERROR）：app.py:2: bad",
                "write_precheck_failed",
            ),
            ("不允许的命令：rm。允许的命令：ls", "command_not_allowed"),
            ("错误：未设置工作区，禁止执行", "workspace_not_set"),
            (
                "错误：检测到同命令重复失败，已短路本次调用（error=x）",
                "repeated_tool_failure_short_circuit",
            ),
            (
                "错误：检测到同类失败已发生（family=x, prev_error=y）",
                "repeated_tool_family_failure_short_circuit",
            ),
            ("错误：命令执行超时（5 秒）", "timeout"),
        ];
        for (text, expected) in cases {
            assert_eq!(
                classify_error_code_fixture(text, "some_tool"),
                *expected,
                "文案未反查到期望码：{text}"
            );
        }
    }

    /// 认不出来的正文仍要退回 `{tool}_failed`（不能为了「都有码」而乱猜）。
    #[test]
    fn unrecognized_body_falls_back_to_tool_failed() {
        assert_eq!(
            classify_error_code_fixture("一段没有任何已知前缀的正文", "my_tool"),
            "my_tool_failed"
        );
    }
}
