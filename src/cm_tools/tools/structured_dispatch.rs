//! `structured` 单入口的 `action` 分发（`validate` / `query` / `diff` / `patch`）。
//!
//! 四个子动作各自保有自己的 `deny_unknown_fields` 入参结构，所以本模块负责三件事：
//!
//! 1. 用并集结构 [`StructuredArgs`] 解析一次（`action` 必填、未知键拒绝）；
//! 2. 按 `action` 校验必填项，给出与合并前一致的错误文案；
//! 3. 只把该 `action` 认识的键转发给既有子实现——因此「另一个 action 的字段」不会被
//!    子实现当成未知字段拒绝（并集 schema 对模型才有意义）。
//!
//! 子实现（`*_try`）与成功正文全部保持不变，合并只发生在注册表与分发层。

use std::path::Path;

use serde_json::{Map, Value};

use super::structured_data::{
    structured_diff_try, structured_patch_try, structured_query_try, structured_validate_try,
};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::{
    StructuredAction, StructuredArgs, StructuredDataFormat, StructuredPatchAction,
};
use crate::cm_tools::tools::{ToolContext, parse_args_typed};

/// `structured` 的唯一执行入口。
#[allow(clippy::result_large_err)]
pub fn structured_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let args: StructuredArgs = parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    require_for_action(&args)?;
    let sub_json = Value::Object(action_args(&args)?).to_string();
    match args.action {
        StructuredAction::Validate => structured_validate_try(&sub_json, working_dir),
        StructuredAction::Query => structured_query_try(&sub_json, working_dir),
        StructuredAction::Diff => structured_diff_try(&sub_json, working_dir),
        StructuredAction::Patch => structured_patch_try(&sub_json, working_dir, ctx),
    }
}

/// 按 `action` 校验必填项。`value` 的必填性仍由 patch 子实现判定（`mutation=set` 时）。
#[allow(clippy::result_large_err)]
fn require_for_action(args: &StructuredArgs) -> Result<(), ToolError> {
    match args.action {
        StructuredAction::Validate => require_path(args),
        StructuredAction::Query | StructuredAction::Patch => {
            require_path(args)?;
            require_query(args)
        }
        StructuredAction::Diff => {
            require_named(args.path_a.as_deref(), "path_a")?;
            require_named(args.path_b.as_deref(), "path_b")
        }
    }
}

fn nonempty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

#[allow(clippy::result_large_err)]
fn require_path(args: &StructuredArgs) -> Result<(), ToolError> {
    require_named(args.path.as_deref(), "path")
}

#[allow(clippy::result_large_err)]
fn require_query(args: &StructuredArgs) -> Result<(), ToolError> {
    match nonempty(args.query.as_deref()) {
        Some(_) => Ok(()),
        None => Err(ToolError::invalid_args(
            "错误：缺少 query（JSON Pointer 如 /a/b 或点号路径如 a.b）".to_string(),
        )),
    }
}

#[allow(clippy::result_large_err)]
fn require_named(v: Option<&str>, field: &str) -> Result<(), ToolError> {
    match nonempty(v) {
        Some(_) => Ok(()),
        None => Err(ToolError::invalid_args(format!("错误：缺少 {field}"))),
    }
}

/// 组装转发给子实现的入参：只含本 `action` 认识的键，省略未提供的可选字段，
/// 让子结构自身的 `serde` 默认值继续生效（`has_header` / `summarize` / `dry_run`
/// / `create_missing` / `max_diff_lines` 的默认值因此一个都不必在本层重复）。
#[allow(clippy::result_large_err)]
fn action_args(args: &StructuredArgs) -> Result<Map<String, Value>, ToolError> {
    let mut m = Map::new();
    match args.action {
        StructuredAction::Validate => validate_args(&mut m, args),
        StructuredAction::Query => query_args(&mut m, args),
        StructuredAction::Diff => diff_args(&mut m, args),
        StructuredAction::Patch => patch_args(&mut m, args)?,
    }
    Ok(m)
}

fn validate_args(m: &mut Map<String, Value>, args: &StructuredArgs) {
    put_str(m, "path", args.path.as_deref());
    put_format(m, args.format);
    put_bool(m, "has_header", args.has_header);
    put_bool(m, "summarize", args.summarize);
}

fn query_args(m: &mut Map<String, Value>, args: &StructuredArgs) {
    put_str(m, "path", args.path.as_deref());
    put_str(m, "query", args.query.as_deref());
    put_format(m, args.format);
    put_bool(m, "has_header", args.has_header);
}

fn diff_args(m: &mut Map<String, Value>, args: &StructuredArgs) {
    put_str(m, "path_a", args.path_a.as_deref());
    put_str(m, "path_b", args.path_b.as_deref());
    put_format(m, args.format);
    put_bool(m, "has_header", args.has_header);
    if let Some(n) = args.max_diff_lines {
        m.insert("max_diff_lines".to_string(), Value::from(n));
    }
}

#[allow(clippy::result_large_err)]
fn patch_args(m: &mut Map<String, Value>, args: &StructuredArgs) -> Result<(), ToolError> {
    if matches!(
        args.format,
        Some(StructuredDataFormat::Csv | StructuredDataFormat::Tsv)
    ) {
        return Err(ToolError::invalid_args(
            "错误：structured patch 不支持 csv/tsv，请改用 table_text 或直接编辑".to_string(),
        ));
    }
    put_str(m, "path", args.path.as_deref());
    put_str(m, "query", args.query.as_deref());
    // 补丁的 `set`/`remove` 在子结构里仍叫 `action`；顶层 `action` 已被子动作名占用，
    // 故对外字段名是 `mutation`，这里是唯一的改名映射点。
    let mutation = args.mutation.unwrap_or(StructuredPatchAction::Set);
    let token = match mutation {
        StructuredPatchAction::Set => "set",
        StructuredPatchAction::Remove => "remove",
    };
    m.insert("action".to_string(), Value::String(token.to_string()));
    if let Some(v) = &args.value {
        m.insert("value".to_string(), v.clone());
    }
    put_format(m, args.format);
    put_bool(m, "create_missing", args.create_missing);
    put_bool(m, "dry_run", args.dry_run);
    put_bool(m, "confirm", args.confirm);
    put_str(m, "base_revision", args.base_revision.as_deref());
    Ok(())
}

fn put_str(m: &mut Map<String, Value>, key: &str, v: Option<&str>) {
    if let Some(s) = v {
        m.insert(key.to_string(), Value::String(s.to_string()));
    }
}

fn put_bool(m: &mut Map<String, Value>, key: &str, v: Option<bool>) {
    if let Some(b) = v {
        m.insert(key.to_string(), Value::Bool(b));
    }
}

fn put_format(m: &mut Map<String, Value>, f: Option<StructuredDataFormat>) {
    if let Some(f) = f {
        m.insert(
            "format".to_string(),
            Value::String(f.as_detect_token().to_string()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_tools::tools::testing::test_ctx;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "crabmate_structured_dispatch_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn missing_action_is_rejected() {
        let dir = tmp_dir("missing_action");
        let err = structured_try("{}", &dir, &test_ctx(&dir)).expect_err("action 必填");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.contains("action"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_action_is_rejected_by_schema() {
        let dir = tmp_dir("unknown_action");
        let err = structured_try(r#"{"action":"nope","path":"a.json"}"#, &dir, &test_ctx(&dir))
            .expect_err("未知 action 须失败");
        assert_eq!(err.code, "invalid_args");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_keys_still_fail() {
        let dir = tmp_dir("unknown_keys");
        let err = structured_try(
            r#"{"action":"validate","path":"a.json","bogus":1}"#,
            &dir,
            &test_ctx(&dir),
        )
        .expect_err("未知键须失败");
        assert_eq!(err.code, "invalid_args");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn per_action_required_fields_are_enforced() {
        let dir = tmp_dir("required");
        let ctx = test_ctx(&dir);
        for args in [
            r#"{"action":"validate"}"#,
            r#"{"action":"query","path":"a.json"}"#,
            r#"{"action":"diff","path_a":"a.json"}"#,
            r#"{"action":"patch","path":"a.json"}"#,
        ] {
            let err = structured_try(args, &dir, &ctx).expect_err("必填项缺失须失败");
            assert_eq!(err.code, "invalid_args", "{args}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fields_of_other_actions_are_ignored() {
        let dir = tmp_dir("ignore");
        std::fs::write(dir.join("a.json"), r#"{"k":1}"#).expect("write fixture");
        // `validate` 请求里带上 query / value / dry_run：都是别的 action 的键，须被忽略。
        let out = structured_try(
            r#"{"action":"validate","path":"a.json","query":"/k","value":9,"dry_run":false}"#,
            &dir,
            &test_ctx(&dir),
        )
        .expect("validate 应成功");
        assert!(out.contains("校验通过"), "{out}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.json")).expect("read fixture"),
            r#"{"k":1}"#,
            "忽略别的 action 的字段，且不得写盘"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patch_dry_run_previews_and_mutation_maps_to_action() {
        let dir = tmp_dir("patch_preview");
        std::fs::write(dir.join("a.json"), "{\"k\":1}\n").expect("write fixture");
        let out = structured_try(
            r#"{"action":"patch","path":"a.json","query":"/k","value":2}"#,
            &dir,
            &test_ctx(&dir),
        )
        .expect("预览应成功");
        assert!(out.contains("预览成功"), "{out}");
        assert!(out.contains("action=set"), "{out}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.json")).expect("read fixture"),
            "{\"k\":1}\n",
            "默认 dry_run=true 不得写盘"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patch_remove_mutation_writes_when_dry_run_false() {
        let dir = tmp_dir("patch_write");
        std::fs::write(dir.join("a.json"), "{\"k\":1,\"z\":2}\n").expect("write fixture");
        let out = structured_try(
            r#"{"action":"patch","path":"a.json","query":"/z","mutation":"remove","dry_run":false}"#,
            &dir,
            &test_ctx(&dir),
        )
        .expect("写盘应成功");
        assert!(out.contains("已写入"), "{out}");
        assert!(out.contains("action=remove"), "{out}");
        let written = std::fs::read_to_string(dir.join("a.json")).expect("read fixture");
        assert!(!written.contains("\"z\""), "{written}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patch_rejects_tabular_format_before_forwarding() {
        let dir = tmp_dir("patch_csv");
        std::fs::write(dir.join("a.csv"), "a,b\n1,2\n").expect("write fixture");
        let err = structured_try(
            r#"{"action":"patch","path":"a.csv","query":"/0/a","value":3,"format":"csv"}"#,
            &dir,
            &test_ctx(&dir),
        )
        .expect_err("patch 不支持 csv");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.contains("csv/tsv"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn action_names_match_the_documented_list() {
        let names: Vec<String> = [
            StructuredAction::Validate,
            StructuredAction::Query,
            StructuredAction::Diff,
            StructuredAction::Patch,
        ]
        .iter()
        .map(|a| serde_json::to_value(a).expect("serialize action"))
        .map(|v| v.as_str().expect("action is string").to_string())
        .collect();
        assert_eq!(names, ["validate", "query", "diff", "patch"]);
    }
}
