//! 分阶段步骤与分层子目标共用的**验收规则 + 证据**内核。
//!
//! - **规则**：[`AcceptanceSpec`]（由 `PlanStepAcceptance` / `GoalAcceptance` 转换而来）。
//! - **证据**：[`AcceptanceEvidence`]（工具输出、解析后的 stdout/stderr、工作区路径策略等）。
//! - **判定**：[`verify_against_spec`] 产出 [`VerifyOutcome`]（与 `step_verifier::VerifyResult` 对齐）。

mod check;

pub use check::verify_against_spec;

/// 与历史 `step_verifier::VerifyResult` 一致，便于分阶段路径零改动引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Pass,
    Fail { reason: String },
}

impl VerifyOutcome {
    pub fn is_pass(&self) -> bool {
        matches!(self, VerifyOutcome::Pass)
    }
}

/// 工作区相对路径如何解析为绝对路径（分阶段与分层历史行为不同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileResolveKind {
    /// `crate::workspace::path::absolutize_relative_under_root`（分阶段 `PlanStepAcceptance`）。
    #[default]
    AbsolutizeRelative,
    /// `workspace_root.join(path)`（分层 `GoalAcceptance`）。
    WorkspaceJoin,
}

/// `expect_exit_code` 在缺少结构化退出码时的策略（分阶段默认按 0；分层历史上「抠不到则放过」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitCodePolicy {
    /// 与 `step_verifier` 一致：`tool_error` / fallback 均无则视为 **0**。
    #[default]
    DefaultZeroIfMissing,
    /// 与分层 `GoalVerifier::verify_exit_code` 一致：解析不到则**不**因退出码失败。
    LenientIfUnparsed,
}

/// 归一化验收条件（不含 serde；由两侧 acceptance 结构转换）。
#[derive(Debug, Clone, Default)]
pub struct AcceptanceSpec {
    pub expect_exit_code: Option<i32>,
    pub exit_code_policy: ExitCodePolicy,
    /// 分阶段：区分 stdout / stderr，大小写敏感。
    pub expect_stdout_contains: Option<String>,
    pub expect_stderr_contains: Option<String>,
    /// 分层：多条子串须同时出现在「合并输出」中（见 [`AcceptanceEvidence`]）。
    pub expect_combined_output_contains: Vec<String>,
    pub combined_match_case_insensitive: bool,
    pub expect_file_exists: Vec<String>,
    pub expect_json_path_equals: Option<crate::agent::plan_artifact::JsonPathEqualsRule>,
    pub expect_http_status: Option<u16>,
    pub file_resolve: FileResolveKind,
}

impl AcceptanceSpec {
    /// 空规范：不施加任何约束（调用方仍可短路，内核视为 Pass）。
    pub fn is_empty(&self) -> bool {
        self.expect_exit_code.is_none()
            && self.expect_stdout_contains.is_none()
            && self.expect_stderr_contains.is_none()
            && self.expect_combined_output_contains.is_empty()
            && self.expect_file_exists.is_empty()
            && self.expect_json_path_equals.is_none()
            && self.expect_http_status.is_none()
    }
}

impl From<&crate::agent::plan_artifact::PlanStepAcceptance> for AcceptanceSpec {
    fn from(a: &crate::agent::plan_artifact::PlanStepAcceptance) -> Self {
        let mut files = Vec::new();
        if let Some(ref p) = a.expect_file_exists
            && !p.trim().is_empty()
        {
            files.push(p.clone());
        }
        Self {
            expect_exit_code: a.expect_exit_code,
            exit_code_policy: ExitCodePolicy::DefaultZeroIfMissing,
            expect_stdout_contains: a.expect_stdout_contains.clone(),
            expect_stderr_contains: a.expect_stderr_contains.clone(),
            expect_combined_output_contains: Vec::new(),
            combined_match_case_insensitive: false,
            expect_file_exists: files,
            expect_json_path_equals: a.expect_json_path_equals.clone(),
            expect_http_status: a.expect_http_status,
            file_resolve: FileResolveKind::AbsolutizeRelative,
        }
    }
}

/// 单次工具执行或子目标摘要对应的证据切片。
#[derive(Debug, Clone, Copy)]
pub struct AcceptanceEvidence<'a> {
    pub tool_name: &'a str,
    pub tool_output: &'a str,
    pub stdout: &'a str,
    pub stderr: &'a str,
    pub tool_error: Option<&'a crate::tool_result::ToolError>,
    /// 无 `tool_error` 时（如分层）从合并文本解析的退出码。
    pub fallback_exit_code: Option<i32>,
    pub workspace_root: &'a std::path::Path,
    pub file_resolve: FileResolveKind,
    /// 若置位，则 `expect_combined_output_contains` 只扫此串，而不拼 `stdout`+`stderr`（分层子目标验收）。
    pub combined_text_override: Option<&'a str>,
}
