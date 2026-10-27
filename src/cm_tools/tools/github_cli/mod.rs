//! 内置 GitHub CLI（`gh`）封装：结构化参数；**退出码 0** 且 **stdout 整段为合法 JSON** 时附加格式化块（与是否传入 `--json` 字段无关）。
//!
//! 须 **`allowed_commands` 含 `gh`**（嵌入默认已含）。对外暴露 5 个工具：**`gh_pr`** / **`gh_run`**（按 `action` 分发的合并工具）、**`gh_issue_create`**、**`gh_release_create`**、**`gh_api`**。其中 **`gh_pr`** / **`gh_run`** / **`gh_issue_create`** / **`gh_release_create`** / **`gh_api`**（变更类 HTTP）整体列入写副作用工具集——`gh_pr` / `gh_run` 内部虽有只读动作，但工具粒度上仍按写效应处理。
//!
//! 五个工具自 C 档迁移起一律走 `Typed` runner（`*_try -> Result<String, ToolError>`），
//! 非零退出显式化为 `gh_{子命令}_failed`。

// `ToolError` 约 144 字节（含分类 + 消息 + 结构化正文），整个子模块都是
// `Result<_, ToolError>`；逐函数 `#[allow]` 只会淹没真正的告警，故在模块级别豁免。
#![allow(clippy::result_large_err)]

mod api;
mod common;
mod issue_create;
mod pr_body;
mod pr_mutate;
mod pr_workflow;
mod release_create;
mod run_ci;

pub mod web_api;

pub use common::GhCtx;
pub use common::{attach_json_if_exit_zero, gh_allowed, validate_api_path, validate_extra_args};

pub use api::gh_api_try;
pub use issue_create::gh_issue_create_try;
pub use pr_body::gh_pr_body_draft_try;
pub use pr_mutate::{gh_pr_comment_try, gh_pr_edit_try, gh_pr_merge_try, gh_pr_review_try};
pub use pr_workflow::{gh_pr_checks_try, gh_pr_create_try};
pub use release_create::gh_release_create_try;
pub use run_ci::{gh_run_failure_summary_try, gh_run_rerun_try};

// ── 动作分发工具 ────────────────────────────────────────────────
//
// 把 `gh_pr_*` / `gh_run_*` 两族合并成各一个工具：schema 是各动作字段的并集，
// 运行时按 `action` 分发到**未改动的**子实现。先用并集 struct 做一次严格反序列化
// （`deny_unknown_fields` + 枚举取值校验），再把原始 JSON 透传——子实现各自用
// `parse_args_json` 取自己关心的键，因此无需裁剪字段；校验失败时不会落到 `gh`。

use std::path::Path;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::parse_args_typed;
use crate::cm_tools::tools::tool_param_types::{
    GhPrAction, GhPrArgs, GhPrReviewEvent, GhRunAction, GhRunArgs,
};

/// 合并后 schema 里 `title` / `event` / `body` 都是 `Option`（否则无法与其它动作共用一份 schema），
/// 因此「某动作下必填」只能在分派层补校验，避免把明显缺参的调用放进子实现白跑一轮。
#[allow(clippy::result_large_err)]
fn validate_pr_action_fields(parsed: &GhPrArgs) -> Result<(), ToolError> {
    let is_blank = |s: &Option<String>| s.as_deref().unwrap_or("").trim().is_empty();
    match parsed.action {
        GhPrAction::Create => {
            if is_blank(&parsed.title) {
                return Err(ToolError::invalid_args(
                    "错误：action=create 需要非空 title".to_string(),
                ));
            }
        }
        GhPrAction::Review => match parsed.event {
            None => {
                return Err(ToolError::invalid_args(
                    "错误：action=review 需要 event（approve / request-changes / comment）".to_string(),
                ));
            }
            Some(GhPrReviewEvent::Comment) | Some(GhPrReviewEvent::RequestChanges) => {
                if is_blank(&parsed.body) {
                    return Err(ToolError::invalid_args(
                        "错误：action=review 且 event 为 comment / request-changes 时须提供 body"
                            .to_string(),
                    ));
                }
            }
            Some(GhPrReviewEvent::Approve) => {}
        },
        GhPrAction::Comment => {
            if is_blank(&parsed.body) {
                return Err(ToolError::invalid_args(
                    "错误：action=comment 需要非空 body".to_string(),
                ));
            }
        }
        // checks / merge / edit / body_draft 没有跨字段必填约束（merge 的 number 可省略＝当前分支）。
        GhPrAction::Checks | GhPrAction::Merge | GhPrAction::Edit | GhPrAction::BodyDraft => {}
    }
    Ok(())
}

/// `gh_pr`：按 `action` 分发到 `checks` / `create` / `merge` / `review` / `comment` / `edit` / `body_draft`。
#[allow(clippy::result_large_err)]
pub fn gh_pr_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    let parsed: GhPrArgs = parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    validate_pr_action_fields(&parsed)?;
    match parsed.action {
        GhPrAction::Checks => pr_workflow::gh_pr_checks_try(args_json, ctx),
        GhPrAction::Create => pr_workflow::gh_pr_create_try(args_json, ctx),
        GhPrAction::Merge => pr_mutate::gh_pr_merge_try(args_json, ctx),
        GhPrAction::Review => pr_mutate::gh_pr_review_try(args_json, ctx),
        GhPrAction::Comment => pr_mutate::gh_pr_comment_try(args_json, ctx),
        GhPrAction::Edit => pr_mutate::gh_pr_edit_try(args_json, ctx),
        GhPrAction::BodyDraft => pr_body::gh_pr_body_draft_try(args_json, ctx),
    }
}

/// `gh_run`：按 `action` 分发到 `rerun` / `failure_summary`。
#[allow(clippy::result_large_err)]
pub fn gh_run_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    let parsed: GhRunArgs = parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    match parsed.action {
        GhRunAction::Rerun => run_ci::gh_run_rerun_try(args_json, ctx),
        GhRunAction::FailureSummary => run_ci::gh_run_failure_summary_try(args_json, ctx),
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    /// 白名单不含 `gh`：子实现的第一步都会走 `gh_allowed` 并返回固定文案，
    /// 用它既能验证「已分派到正确子实现」，又不必真的执行 `gh`。
    fn allowlist_without_gh() -> Vec<String> {
        vec!["cargo".to_string()]
    }

    fn call_pr(args: &str) -> String {
        match gh_pr_try(
            args,
            &GhCtx::new(4096, &allowlist_without_gh(), Path::new(".")),
        ) {
            Ok(s) => s,
            Err(e) => e.message,
        }
    }

    fn call_run(args: &str) -> String {
        match gh_run_try(
            args,
            &GhCtx::new(4096, &allowlist_without_gh(), Path::new(".")),
        ) {
            Ok(s) => s,
            Err(e) => e.message,
        }
    }

    #[test]
    fn gh_pr_enforces_per_action_required_fields() {
        let cases = [
            (r#"{"action":"create"}"#, "title"),
            (r#"{"action":"create","title":"   "}"#, "title"),
            (r#"{"action":"review"}"#, "event"),
            (r#"{"action":"review","event":"approve"}"#, "allowed_commands"),
            (r#"{"action":"review","event":"request-changes"}"#, "body"),
            (r#"{"action":"review","event":"comment"}"#, "body"),
            (r#"{"action":"comment"}"#, "body"),
        ];
        for (args, needle) in cases {
            let out = call_pr(args);
            assert!(out.contains(needle), "{args} => {out}");
        }
    }

    #[test]
    fn gh_pr_rejects_unknown_action_and_unknown_field() {
        let bad_action = call_pr(r#"{"action":"nope"}"#);
        assert!(bad_action.contains("错误"), "{bad_action}");

        let unknown_field = call_pr(r#"{"action":"checks","bogus_key":1}"#);
        assert!(unknown_field.contains("bogus_key"), "{unknown_field}");
    }

    #[test]
    fn gh_pr_routes_to_matching_sub_implementation() {
        // `body_draft` 不需要 `gh` 命令（读模板 + git），故不在此断言路由。
        for args in [
            r#"{"action":"checks"}"#,
            r#"{"action":"create","title":"fix: x"}"#,
            r#"{"action":"merge"}"#,
            r#"{"action":"edit","title":"t"}"#,
            r#"{"action":"comment","body":"lgtm"}"#,
        ] {
            let out = call_pr(args);
            assert!(out.contains("allowed_commands"), "{args} => {out}");
        }
    }

    #[test]
    fn gh_run_routes_and_validates() {
        let missing_run_id = call_run(r#"{"action":"failure_summary"}"#);
        assert!(missing_run_id.contains("run_id"), "{missing_run_id}");

        let bad_action = call_run(r#"{"action":"nope","run_id":"1"}"#);
        assert!(bad_action.contains("错误"), "{bad_action}");

        for args in [
            r#"{"action":"rerun","run_id":"123"}"#,
            r#"{"action":"failure_summary","run_id":"123"}"#,
        ] {
            let out = call_run(args);
            assert!(out.contains("allowed_commands"), "{args} => {out}");
        }
    }
}
