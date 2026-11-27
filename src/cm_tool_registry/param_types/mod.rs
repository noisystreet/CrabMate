//! 内置工具入参的 **serde + schemars** 真源：与 [`super::tool_json_schema`] 生成的 `parameters` 及
//! 各 `runner_*` 反序列化形状一致。
//!
//! 按工具族拆成子模块（#1353：此前是 8 个 `part_*.inc.rs` 靠 `include!` 拼进本模块的同一作用域，
//! 跨片引用只能靠裸名共享）。现在跨模块只共享 [`defaults`] 的 serde 默认值辅助，公开路径经下面的
//! 重导出保持不变。

mod basic;
mod cargo_gh_py;
mod defaults;
mod delegation;
mod diagnostics;
mod exec_file;
mod git;
mod schedule;
mod structured;

pub use self::basic::{
    BackgroundJobAction, BackgroundJobArgs, BackgroundJobCancelArgs, BackgroundJobWaitArgs,
    BackgroundJobWaitUntil, CodeStatsArgs, CodeStatsFormat, CoverageReportArgs,
    CoverageReportFormat, DateCalcArgs, DateCalcMode, DependencyGraphArgs, DependencyGraphFormat,
    DependencyGraphKind, EnvVarCheckArgs, GetWeatherArgs, JsonFormatArgs, JsonFormatMode,
    MarkdownCheckLinksArgs, MarkdownCheckLinksOutputFormat, RegexTestArgs, WebSearchArgs,
};
pub use self::delegation::DelegateTaskArgs;

pub use self::cargo_gh_py::{
    CargoCommonCliArgs, CargoRunArgs, CargoTestArgs, GhApiArgs, GhIssueCreateArgs, GhPrAction,
    GhPrArgs, GhPrReviewEvent, GhReleaseCreateArgs, GhRunAction, GhRunArgs, PytestRunArgs,
    PythonSnippetRunArgs, RustAnalyzerArgs,
};

pub use self::diagnostics::{
    CrateContractMapArgs, DiagnosticSummaryArgs, LongTermForgetArgs, LongTermMemoryListArgs,
    LongTermRememberArgs, PresentClarificationQuestionnaireArgs, RepoOverviewSweepArgs,
    SelfConfigInfoArgs, SkillManageAction, SkillManageArgs, SkillManageLayer, SkillReadArgs,
    SummarizeExperienceArgs,
};

pub use self::exec_file::{
    ApplyPatchArgs, CodebaseSemanticSearchArgs, FileFromToOverwriteArgs, FileWriteArgs,
    GlobFilesArgs, ListTreeArgs, ModifyFileArgs, ReadBinaryMetaArgs, ReadDirEnhancedArgs,
    ReadFileArgs, RunCommandArgs, SearchInFilesEnhancedArgs, TerminalSessionArgs,
};

pub use self::git::{GitAction, GitArgs};

pub use self::schedule::{
    AddEventArgs, AddReminderArgs, CallGraphSketchArgs, CodeNavAction, CodeNavArgs, EventAction,
    EventArgs, FindReferencesArgs, FindSymbolArgs, IdOnlyArgs, ListEventsArgs, ListRemindersArgs,
    ReminderAction, ReminderArgs, UpdateEventArgs, UpdateReminderArgs,
};

#[cfg(feature = "archive-tools")]
pub use self::schedule::{ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs};

pub use self::structured::{
    AppendFileArgs, CodespellCheckArgs, CreateDirArgs, DeleteDirArgs, DeleteFilesArgs,
    DocsHealthSweepArgs, SEARCH_REPLACE_NEW_TEXT_ALIASES, SEARCH_REPLACE_NEW_TEXT_KEY,
    SEARCH_REPLACE_OLD_TEXT_ALIASES, SEARCH_REPLACE_OLD_TEXT_KEY, SearchReplaceArgs,
    StructuredAction, StructuredArgs, StructuredDataFormat, StructuredDiffArgs,
    StructuredPatchAction, StructuredPatchArgs, StructuredQueryArgs, StructuredValidateArgs,
    TableTextAction, TableTextAggregateOp, TableTextArgs, TableTextDelimiter, TextDiffArgs,
    TyposCheckArgs, WorkflowExecuteArgs,
};
