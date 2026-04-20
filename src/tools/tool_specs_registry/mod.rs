//! 工具规格表：按领域拆分为 `specs/*.inc.rs`（各文件为单个数组字面量），运行时在 `OnceLock` 中拼接为 `&'static [ToolSpec]`。

use std::sync::OnceLock;

use super::tool_params;
use super::tool_summary as ts;
use super::*;

static SPECS_ARCHIVE: &[ToolSpec] = &include!("specs/archive.inc.rs");
static SPECS_BASIC_NETWORK: &[ToolSpec] = &include!("specs/basic_network.inc.rs");
static SPECS_EXEC_PACKAGE: &[ToolSpec] = &include!("specs/exec_package.inc.rs");
static SPECS_CARGO_RUST: &[ToolSpec] = &include!("specs/cargo_rust.inc.rs");
static SPECS_FRONTEND_PYTHON: &[ToolSpec] = &include!("specs/frontend_python.inc.rs");
static SPECS_GO: &[ToolSpec] = &include!("specs/go.inc.rs");
static SPECS_JVM_CONTAINER: &[ToolSpec] = &include!("specs/jvm_container.inc.rs");
static SPECS_QUALITY_AST: &[ToolSpec] = &include!("specs/quality_ast.inc.rs");
static SPECS_FRONTEND_AUDIT_CI: &[ToolSpec] = &include!("specs/frontend_audit_ci.inc.rs");
static SPECS_DIAGNOSTICS_DOCS: &[ToolSpec] = &include!("specs/diagnostics_docs.inc.rs");
static SPECS_GITHUB_CLI: &[ToolSpec] = &include!("specs/github_cli.inc.rs");
static SPECS_GIT_READ: &[ToolSpec] = &include!("specs/git_read.inc.rs");
static SPECS_GIT_WRITE_CORE: &[ToolSpec] = &include!("specs/git_write_core.inc.rs");
static SPECS_FILE_CORE: &[ToolSpec] = &include!("specs/file_core.inc.rs");
static SPECS_MARKDOWN_STRUCTURED: &[ToolSpec] = &include!("specs/markdown_structured.inc.rs");
static SPECS_TEXT_CODE_NAV: &[ToolSpec] = &include!("specs/text_code_nav.inc.rs");
static SPECS_FORMAT_LINT: &[ToolSpec] = &include!("specs/format_lint.inc.rs");
static SPECS_SCHEDULE: &[ToolSpec] = &include!("specs/schedule.inc.rs");
static SPECS_GIT_WRITE_EXTRA: &[ToolSpec] = &include!("specs/git_write_extra.inc.rs");
static SPECS_NODEJS: &[ToolSpec] = &include!("specs/nodejs.inc.rs");
static SPECS_GOLANGCI: &[ToolSpec] = &include!("specs/golangci.inc.rs");
static SPECS_PROCESS: &[ToolSpec] = &include!("specs/process.inc.rs");
static SPECS_METRICS: &[ToolSpec] = &include!("specs/metrics.inc.rs");
static SPECS_FILE_EXTRA: &[ToolSpec] = &include!("specs/file_extra.inc.rs");
static SPECS_MISC_BASIC: &[ToolSpec] = &include!("specs/misc_basic.inc.rs");
static SPECS_SOURCE_ANALYSIS: &[ToolSpec] = &include!("specs/source_analysis.inc.rs");

static ALL_TOOL_SPECS: OnceLock<&'static [ToolSpec]> = OnceLock::new();

pub(super) fn tool_specs() -> &'static [ToolSpec] {
    ALL_TOOL_SPECS.get_or_init(|| {
        let cap = SPECS_ARCHIVE.len()
            + SPECS_BASIC_NETWORK.len()
            + SPECS_EXEC_PACKAGE.len()
            + SPECS_CARGO_RUST.len()
            + SPECS_FRONTEND_PYTHON.len()
            + SPECS_GO.len()
            + SPECS_JVM_CONTAINER.len()
            + SPECS_QUALITY_AST.len()
            + SPECS_FRONTEND_AUDIT_CI.len()
            + SPECS_DIAGNOSTICS_DOCS.len()
            + SPECS_GITHUB_CLI.len()
            + SPECS_GIT_READ.len()
            + SPECS_GIT_WRITE_CORE.len()
            + SPECS_FILE_CORE.len()
            + SPECS_MARKDOWN_STRUCTURED.len()
            + SPECS_TEXT_CODE_NAV.len()
            + SPECS_FORMAT_LINT.len()
            + SPECS_SCHEDULE.len()
            + SPECS_GIT_WRITE_EXTRA.len()
            + SPECS_NODEJS.len()
            + SPECS_GOLANGCI.len()
            + SPECS_PROCESS.len()
            + SPECS_METRICS.len()
            + SPECS_FILE_EXTRA.len()
            + SPECS_MISC_BASIC.len()
            + SPECS_SOURCE_ANALYSIS.len();
        let mut v = Vec::with_capacity(cap);
        v.extend_from_slice(SPECS_ARCHIVE);
        v.extend_from_slice(SPECS_BASIC_NETWORK);
        v.extend_from_slice(SPECS_EXEC_PACKAGE);
        v.extend_from_slice(SPECS_CARGO_RUST);
        v.extend_from_slice(SPECS_FRONTEND_PYTHON);
        v.extend_from_slice(SPECS_GO);
        v.extend_from_slice(SPECS_JVM_CONTAINER);
        v.extend_from_slice(SPECS_QUALITY_AST);
        v.extend_from_slice(SPECS_FRONTEND_AUDIT_CI);
        v.extend_from_slice(SPECS_DIAGNOSTICS_DOCS);
        v.extend_from_slice(SPECS_GITHUB_CLI);
        v.extend_from_slice(SPECS_GIT_READ);
        v.extend_from_slice(SPECS_GIT_WRITE_CORE);
        v.extend_from_slice(SPECS_FILE_CORE);
        v.extend_from_slice(SPECS_MARKDOWN_STRUCTURED);
        v.extend_from_slice(SPECS_TEXT_CODE_NAV);
        v.extend_from_slice(SPECS_FORMAT_LINT);
        v.extend_from_slice(SPECS_SCHEDULE);
        v.extend_from_slice(SPECS_GIT_WRITE_EXTRA);
        v.extend_from_slice(SPECS_NODEJS);
        v.extend_from_slice(SPECS_GOLANGCI);
        v.extend_from_slice(SPECS_PROCESS);
        v.extend_from_slice(SPECS_METRICS);
        v.extend_from_slice(SPECS_FILE_EXTRA);
        v.extend_from_slice(SPECS_MISC_BASIC);
        v.extend_from_slice(SPECS_SOURCE_ANALYSIS);
        Box::leak(v.into_boxed_slice())
    })
}
