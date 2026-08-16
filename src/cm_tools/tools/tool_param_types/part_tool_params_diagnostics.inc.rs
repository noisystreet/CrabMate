// `diagnostics` 工具参数（原 `tool_params/diagnostics.rs`）。

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ChangelogDraftArgs {
    pub since: Option<String>,
    pub until: Option<String>,
    #[schemars(range(min = 1, max = 2000))]
    pub max_commits: Option<u32>,
    pub group_by: Option<ChangelogGroupBy>,
    #[schemars(range(min = 1, max = 100))]
    pub max_tag_sections: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChangelogGroupBy {
    Date,
    Flat,
    #[serde(rename = "tag_ranges")]
    TagRanges,
    Tags,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct LicenseNoticeArgs {
    pub workspace_only: Option<bool>,
    #[schemars(range(min = 1, max = 3000))]
    pub max_crates: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClarifyQuestionnaireQuestion {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub required: Option<bool>,
    pub kind: Option<ClarifyQuestionKind>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ClarifyQuestionKind {
    Text,
    Choice,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresentClarificationQuestionnaireArgs {
    pub questionnaire_id: String,
    pub intro: String,
    pub questions: Vec<ClarifyQuestionnaireQuestion>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
pub struct DiagnosticSummaryArgs {
    pub include_toolchain: Option<bool>,
    pub include_workspace_paths: Option<bool>,
    pub include_env: Option<bool>,
    pub extra_env_vars: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct RepoOverviewSweepArgs {
    pub doc_paths: Option<Vec<String>>,
    pub source_roots: Option<Vec<String>>,
    pub build_globs: Option<Vec<String>>,
    #[schemars(range(min = 10, max = 500))]
    pub doc_preview_max_lines: Option<u32>,
    #[schemars(range(min = 1, max = 20))]
    pub list_tree_max_depth: Option<u32>,
    #[schemars(range(min = 50, max = 5000))]
    pub list_tree_max_entries: Option<u32>,
    pub list_tree_include_hidden: Option<bool>,
    #[schemars(range(min = 10, max = 2000))]
    pub build_glob_max_results: Option<u32>,
    #[schemars(range(min = 1, max = 100))]
    pub build_glob_max_depth: Option<u32>,
    pub include_project_profile: Option<bool>,
    #[schemars(range(min = 0, max = 50_000))]
    pub project_profile_max_chars: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CrateContractMapArgs {
    #[schemars(range(min = 5, max = 120))]
    pub head_lines_per_file: Option<u32>,
    #[schemars(range(min = 5, max = 200))]
    pub keyword_hits_per_file: Option<u32>,
    pub extra_paths: Option<Vec<String>>,
    #[schemars(range(min = 0, max = 40))]
    pub max_extra_paths: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorOutputPlaybookArgs {
    pub error_text: String,
    pub ecosystem: Option<String>,
    #[schemars(range(min = 1, max = 100_000))]
    pub max_chars: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LongTermRememberArgs {
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub ttl_secs: u64,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct LongTermForgetArgs {
    pub memory_id: Option<i64>,
    pub memory_text: Option<String>,
    pub explicit_only: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct LongTermMemoryListArgs {
    #[schemars(range(min = 1, max = 64))]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummarizeExperienceArgs {
    pub experience: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 过期秒数；0 表示永不过期（仍受 max_entries 淘汰）
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub ttl_secs: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaybookRunCommandsArgs {
    pub error_text: String,
    pub ecosystem: Option<String>,
    #[schemars(range(min = 1, max = 100_000))]
    pub max_chars: Option<u32>,
    #[schemars(range(min = 1, max = 3))]
    pub max_commands: Option<u32>,
}
