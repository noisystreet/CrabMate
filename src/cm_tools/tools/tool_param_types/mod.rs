//! 内置工具入参的 **serde + schemars** 真源：与 [`super::tool_json_schema`] 生成的 `parameters` 及
//! 各 `runner_*` 反序列化形状一致。
//!
//! 按工具族拆成子模块（#1353：此前是 8 个 `part_*.inc.rs` 靠 `include!` 拼进本模块的同一作用域，
//! 跨片引用只能靠裸名共享）。现在跨模块只共享 [`defaults`] 的 serde 默认值辅助，公开路径经下面的
//! 重导出保持不变。

mod basic;
mod cargo_gh_py;
mod defaults;
mod diagnostics;
mod exec_file;
mod git;
mod schedule;
mod structured;

pub use self::basic::{
    GetWeatherArgs, WebSearchArgs, RegexTestArgs, DateCalcMode,
    DateCalcArgs, JsonFormatMode, JsonFormatArgs, EnvVarCheckArgs,
    BackgroundJobAction, BackgroundJobArgs, BackgroundJobCancelArgs, BackgroundJobWaitUntil,
    BackgroundJobWaitArgs, MarkdownCheckLinksOutputFormat, MarkdownCheckLinksArgs, CodeStatsFormat,
    CodeStatsArgs, DependencyGraphFormat, DependencyGraphKind, DependencyGraphArgs,
    CoverageReportFormat, CoverageReportArgs,
};

pub use self::cargo_gh_py::{
    CargoCommonCliArgs, CargoTestArgs, CargoRunArgs, RustAnalyzerArgs,
    GhPrReviewEvent, GhIssueCreateArgs, GhReleaseCreateArgs, GhApiArgs,
    PytestRunArgs, PythonSnippetRunArgs, GhPrAction, GhPrArgs,
    GhRunAction, GhRunArgs,
};

pub use self::diagnostics::{
    PresentClarificationQuestionnaireArgs, DiagnosticSummaryArgs, SelfConfigInfoArgs, SkillManageAction,
    SkillManageLayer, SkillManageArgs, SkillReadArgs, RepoOverviewSweepArgs,
    CrateContractMapArgs, LongTermRememberArgs, LongTermForgetArgs, LongTermMemoryListArgs,
    SummarizeExperienceArgs,
};

pub use self::exec_file::{
    TerminalSessionArgs, RunCommandArgs, FileWriteArgs, ModifyFileArgs,
    FileFromToOverwriteArgs, ReadFileArgs, GlobFilesArgs, ListTreeArgs,
    ReadBinaryMetaArgs, ApplyPatchArgs, CodebaseSemanticSearchArgs, SearchInFilesEnhancedArgs,
    ReadDirEnhancedArgs,
};

pub use self::git::{
    GitArgs, GitAction,
};

pub use self::schedule::{
    AddReminderArgs, ListRemindersArgs, UpdateReminderArgs, IdOnlyArgs,
    AddEventArgs, ListEventsArgs, UpdateEventArgs, ReminderAction,
    ReminderArgs, EventAction, EventArgs, FindSymbolArgs,
    FindReferencesArgs, CallGraphSketchArgs, CodeNavAction, CodeNavArgs,
};

#[cfg(feature = "archive-tools")]
pub use self::schedule::{ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs};

pub use self::structured::{
    StructuredDataFormat, StructuredPatchAction, StructuredValidateArgs, StructuredQueryArgs,
    StructuredDiffArgs, StructuredPatchArgs, StructuredAction, StructuredArgs,
    WorkflowExecuteArgs, TyposCheckArgs, CodespellCheckArgs, DocsHealthSweepArgs,
    TableTextAction, TableTextDelimiter, TableTextAggregateOp, TableTextArgs,
    TextDiffArgs, DeleteFilesArgs, DeleteDirArgs, AppendFileArgs,
    CreateDirArgs, SearchReplaceArgs,
};
