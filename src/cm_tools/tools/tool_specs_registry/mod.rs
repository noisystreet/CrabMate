//! 工具规格表：按领域拆分为 `specs/*.inc.rs`（各文件为单个数组字面量），运行时在 `OnceLock` 中拼接为 `&'static [ToolSpec]`。

use std::sync::OnceLock;

use crate::cm_tools::tool_names;
use crate::cm_tools::tools::http_fetch::{HttpFetchArgs, HttpRequestArgs};
use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value as schema_of;
use crate::cm_tools::tools::tool_param_types as args;

use super::tool_summary as ts;
use super::*;

#[cfg(feature = "archive-tools")]
static SPECS_ARCHIVE: &[ToolSpec] = &include!("specs/archive.inc.rs");
#[cfg(not(feature = "archive-tools"))]
static SPECS_ARCHIVE: &[ToolSpec] = &[];
static SPECS_BASIC_NETWORK: &[ToolSpec] = &include!("specs/basic_network.inc.rs");
static SPECS_EXEC_PACKAGE: &[ToolSpec] = &include!("specs/exec_package.inc.rs");
static SPECS_CARGO_RUST: &[ToolSpec] = &include!("specs/cargo_rust.inc.rs");
static SPECS_FRONTEND_PYTHON: &[ToolSpec] = &include!("specs/frontend_python.inc.rs");
static SPECS_DOCS_HEALTH: &[ToolSpec] = &include!("specs/docs_health.inc.rs");
static SPECS_FRONTEND_AUDIT_CI: &[ToolSpec] = &include!("specs/frontend_audit_ci.inc.rs");
static SPECS_DIAGNOSTICS_DOCS: &[ToolSpec] = &include!("specs/diagnostics_docs.inc.rs");
static SPECS_GITHUB_CLI: &[ToolSpec] = &include!("specs/github_cli.inc.rs");
static SPECS_GIT_READ: &[ToolSpec] = &include!("specs/git_read.inc.rs");
static SPECS_FILE_CORE: &[ToolSpec] = &include!("specs/file_core.inc.rs");
static SPECS_MARKDOWN_STRUCTURED: &[ToolSpec] = &include!("specs/markdown_structured.inc.rs");
static SPECS_TEXT_CODE_NAV: &[ToolSpec] = &include!("specs/text_code_nav.inc.rs");
static SPECS_SCHEDULE: &[ToolSpec] = &include!("specs/schedule.inc.rs");
static SPECS_PROCESS: &[ToolSpec] = &include!("specs/process.inc.rs");
static SPECS_METRICS: &[ToolSpec] = &include!("specs/metrics.inc.rs");
static SPECS_FILE_EXTRA: &[ToolSpec] = &include!("specs/file_extra.inc.rs");
static SPECS_MISC_BASIC: &[ToolSpec] = &include!("specs/misc_basic.inc.rs");
static SPECS_SELF_CONFIG: &[ToolSpec] = &include!("specs/self_config_info.inc.rs");

static ALL_TOOL_SPECS: OnceLock<&'static [ToolSpec]> = OnceLock::new();

pub(super) fn tool_specs() -> &'static [ToolSpec] {
    ALL_TOOL_SPECS.get_or_init(|| {
        let cap = SPECS_ARCHIVE.len()
            + SPECS_BASIC_NETWORK.len()
            + SPECS_EXEC_PACKAGE.len()
            + SPECS_CARGO_RUST.len()
            + SPECS_FRONTEND_PYTHON.len()
            + SPECS_DOCS_HEALTH.len()
            + SPECS_FRONTEND_AUDIT_CI.len()
            + SPECS_DIAGNOSTICS_DOCS.len()
            + SPECS_GITHUB_CLI.len()
            + SPECS_GIT_READ.len()
            + SPECS_FILE_CORE.len()
            + SPECS_MARKDOWN_STRUCTURED.len()
            + SPECS_TEXT_CODE_NAV.len()
            + SPECS_SCHEDULE.len()
            + SPECS_PROCESS.len()
            + SPECS_METRICS.len()
            + SPECS_FILE_EXTRA.len()
            + SPECS_MISC_BASIC.len()
            + SPECS_SELF_CONFIG.len();
        let mut v = Vec::with_capacity(cap);
        v.extend_from_slice(SPECS_ARCHIVE);
        v.extend_from_slice(SPECS_BASIC_NETWORK);
        v.extend_from_slice(SPECS_EXEC_PACKAGE);
        v.extend_from_slice(SPECS_CARGO_RUST);
        v.extend_from_slice(SPECS_FRONTEND_PYTHON);
        v.extend_from_slice(SPECS_DOCS_HEALTH);
        v.extend_from_slice(SPECS_FRONTEND_AUDIT_CI);
        v.extend_from_slice(SPECS_DIAGNOSTICS_DOCS);
        v.extend_from_slice(SPECS_GITHUB_CLI);
        v.extend_from_slice(SPECS_GIT_READ);
        v.extend_from_slice(SPECS_FILE_CORE);
        v.extend_from_slice(SPECS_MARKDOWN_STRUCTURED);
        v.extend_from_slice(SPECS_TEXT_CODE_NAV);
        v.extend_from_slice(SPECS_SCHEDULE);
        v.extend_from_slice(SPECS_PROCESS);
        v.extend_from_slice(SPECS_METRICS);
        v.extend_from_slice(SPECS_FILE_EXTRA);
        v.extend_from_slice(SPECS_MISC_BASIC);
        v.extend_from_slice(SPECS_SELF_CONFIG);
        Box::leak(v.into_boxed_slice())
    })
}

#[cfg(test)]
mod name_consistency_tests {
    use super::*;
    use crate::cm_tools::tool_names::ALL;
    use std::collections::HashSet;

    /// `tool_names::ALL` 与注册表必须**双向一致**。
    ///
    /// 只改 `specs` 不改 `ALL`（或反之）会让所有以 `ALL` 驱动的遍历静默漏项，
    /// 且不会有任何编译错误——这正是把工具名收敛到单一真源要防的那类缺陷。
    #[test]
    fn all_tool_names_match_specs() {
        let registered: HashSet<&str> = tool_specs().iter().map(|s| s.name).collect();
        let declared: HashSet<&str> = ALL.iter().copied().collect();

        let missing_in_all: Vec<&str> = registered.difference(&declared).copied().collect();
        let missing_in_specs: Vec<&str> = declared.difference(&registered).copied().collect();

        assert!(
            missing_in_all.is_empty(),
            "以下工具已在 specs 注册，但未在 tool_names::ALL 声明：{missing_in_all:?}"
        );
        assert!(
            missing_in_specs.is_empty(),
            "以下工具已在 tool_names::ALL 声明，但未在 specs 注册：{missing_in_specs:?}"
        );
    }

    /// 注册表内部不得有重名（重名会让 `tools_for_name` 类查找取到任意一个）。
    #[test]
    fn spec_names_are_unique() {
        let mut seen = HashSet::new();
        for spec in tool_specs() {
            assert!(seen.insert(spec.name), "specs 中工具名重复：{}", spec.name);
        }
    }
}
