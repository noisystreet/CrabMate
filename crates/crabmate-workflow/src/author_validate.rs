//! `workflow_author` v2：版本号与 steps / nodes 两种模式的 JSON Schema 校验。

use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

/// 当前支持的作者层规范版本（根对象 `version` 字段）。
pub const WORKFLOW_AUTHOR_SPEC_VERSION: u64 = 2;

static STEPS_SCHEMA_VALIDATOR: OnceLock<Validator> = OnceLock::new();
static NODES_SCHEMA_VALIDATOR: OnceLock<Validator> = OnceLock::new();

/// 作者层 YAML/JSON 的两种互斥形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorDocumentMode {
    /// `steps[]`（可含 `when` / `for_each` / `repeat` / `choice`），编译为 `nodes`。
    Steps,
    /// 直接 `workflow.nodes` 或仅 `workflow.workflow_template`。
    Nodes,
}

impl AuthorDocumentMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Nodes => "nodes",
        }
    }
}

fn steps_schema_validator() -> &'static Validator {
    STEPS_SCHEMA_VALIDATOR.get_or_init(|| {
        let raw =
            include_str!("../../../fixtures/workflows/schema/workflow_author_v2_steps.schema.json");
        let schema: Value =
            serde_json::from_str(raw).expect("workflow_author_v2_steps.schema.json 须为合法 JSON");
        jsonschema::validator_for(&schema)
            .expect("workflow_author_v2_steps schema 须可编译为验证器")
    })
}

fn nodes_schema_validator() -> &'static Validator {
    NODES_SCHEMA_VALIDATOR.get_or_init(|| {
        let raw =
            include_str!("../../../fixtures/workflows/schema/workflow_author_v2_nodes.schema.json");
        let schema: Value =
            serde_json::from_str(raw).expect("workflow_author_v2_nodes.schema.json 须为合法 JSON");
        jsonschema::validator_for(&schema)
            .expect("workflow_author_v2_nodes schema 须可编译为验证器")
    })
}

fn format_schema_errors(
    validator: &Validator,
    instance: &Value,
    mode: AuthorDocumentMode,
) -> String {
    let mut iter = validator.iter_errors(instance);
    let Some(e1) = iter.next() else {
        return format!(
            "workflow_author（{} 模式）与 JSON Schema 不一致。",
            mode.as_str()
        );
    };
    let mut s = if e1.instance_path().as_str().is_empty() {
        e1.to_string()
    } else {
        format!("路径 {}: {}", e1.instance_path(), e1)
    };
    for (i, e) in iter.enumerate() {
        if i >= 2 {
            s.push_str(" …");
            break;
        }
        s.push('；');
        s.push_str(&e.to_string());
    }
    format!(
        "workflow_author（{} 模式）与 schema 不一致：{s}（见 fixtures/workflows/schema/）",
        mode.as_str()
    )
}

fn validate_version_field(root: &Value) -> Result<(), String> {
    let Some(v) = root.get("version") else {
        return Err(format!(
            "workflow_author 缺少 version 字段（当前仅支持 version: {WORKFLOW_AUTHOR_SPEC_VERSION}）"
        ));
    };
    let version = v
        .as_u64()
        .or_else(|| v.as_i64().filter(|&n| n >= 0).map(|n| n as u64))
        .ok_or_else(|| "version 须为非负整数".to_string())?;
    if version != WORKFLOW_AUTHOR_SPEC_VERSION {
        return Err(format!(
            "workflow_author 不支持 version: {version}（当前仅支持 version: {WORKFLOW_AUTHOR_SPEC_VERSION}）"
        ));
    }
    Ok(())
}

fn workflow_object(root: &Value) -> Option<&serde_json::Map<String, Value>> {
    root.get("workflow").and_then(|w| w.as_object())
}

/// 判定作者层模式；**不**接受同时含 `steps` 与 `nodes`。
pub fn detect_author_document_mode(root: &Value) -> Result<AuthorDocumentMode, String> {
    validate_version_field(root)?;

    let has_root_steps = root.get("steps").is_some();
    let wf = workflow_object(root);
    let has_wf_steps = wf.is_some_and(|o| o.contains_key("steps"));
    let has_wf_nodes = wf.is_some_and(|o| o.contains_key("nodes"));
    let has_template = wf.is_some_and(|o| {
        o.get("workflow_template")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    });

    if (has_root_steps || has_wf_steps) && (has_wf_nodes || root.get("nodes").is_some()) {
        return Err(
            "workflow_author 不能同时包含 steps 与 nodes（请二选一，见 docs/工作流Markdown作者层设计.md）"
                .to_string(),
        );
    }

    if has_root_steps || has_wf_steps {
        return Ok(AuthorDocumentMode::Steps);
    }

    if has_wf_nodes || root.get("nodes").is_some() || has_template {
        return Ok(AuthorDocumentMode::Nodes);
    }

    Err(
        "workflow_author 须包含 steps，或 workflow.nodes / workflow.workflow_template 之一"
            .to_string(),
    )
}

fn validate_with_schema(mode: AuthorDocumentMode, root: &Value) -> Result<(), String> {
    let validator = match mode {
        AuthorDocumentMode::Steps => steps_schema_validator(),
        AuthorDocumentMode::Nodes => nodes_schema_validator(),
    };
    if validator.is_valid(root) {
        return Ok(());
    }
    Err(format_schema_errors(validator, root, mode))
}

/// 是否应对该 JSON 做作者层 schema 校验（含 `version` 或 `steps` 时启用）。
pub fn should_validate_author_spec(root: &Value) -> bool {
    if root.get("version").is_some() {
        return true;
    }
    root.get("steps").is_some() || workflow_object(root).is_some_and(|o| o.contains_key("steps"))
}

/// 版本 + 模式判定 + JSON Schema；在 `compile_workflow_author_yaml` 之前调用。
pub fn validate_workflow_author_document(root: &Value) -> Result<AuthorDocumentMode, String> {
    let mode = detect_author_document_mode(root)?;
    validate_with_schema(mode, root)?;
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_missing_version() {
        let root = json!({ "workflow": { "fail_fast": true }, "steps": [] });
        let err = validate_workflow_author_document(&root).unwrap_err();
        assert!(err.contains("version"));
    }

    #[test]
    fn rejects_unsupported_version() {
        let root =
            json!({ "version": 1, "workflow": {}, "steps": [{"id":"a","tool":"calc","args":{}}] });
        let err = validate_workflow_author_document(&root).unwrap_err();
        assert!(err.contains("不支持 version"));
    }

    #[test]
    fn rejects_steps_and_nodes_together() {
        let root = json!({
            "version": 2,
            "workflow": { "nodes": [] },
            "steps": [{"id":"a","tool":"calc","args":{}}]
        });
        let err = detect_author_document_mode(&root).unwrap_err();
        assert!(err.contains("不能同时"));
    }
}
