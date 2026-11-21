//! 加载期校验的**唯一编排**：工具名 → DAG → 拓扑分层。
//!
//! 这套编排此前有三份：工具面 `validate_only`（`run.rs`）、`crabmate workflow validate`
//! （`runtime/cli_workflow.rs`）、`crabmate doctor` 的工作流块（`runtime/cli_doctor.rs`）。
//! 底层三个函数一直是共用的，重复的是**顺序与错误措辞**——那种偏差不会报错，只会让同一个
//! YAML 在不同入口得到不同结论（#1383）。现在顺序与阶段名只有这一份，调用方只负责渲染：
//! 工具面拼成 `human_summary` 信封，CLI 直接打印 `detail`（保持既有文案与退出码）。

use super::dag::{topo_layers, validate_dag};
use super::model::WorkflowSpec;
use super::tool_name_check::validate_tool_names;

/// 失败发生在哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateStage {
    /// 节点声明的 `tool_name` 不在工具登记表里。
    ToolNames,
    /// DAG 结构问题（未知依赖 / 环 / 自环）。
    Dag,
    /// 拓扑分层计算失败。
    Topo,
}

impl ValidateStage {
    /// 阶段名。工具信封与日志都用这一串，避免「一边写 DAG 校验失败、一边写 dag invalid」。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ToolNames => "工具名校验失败",
            Self::Dag => "DAG 校验失败",
            Self::Topo => "层级计算失败",
        }
    }
}

/// 校验失败：阶段 + 底层细节（`detail` 不含阶段名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateFailure {
    pub stage: ValidateStage,
    pub detail: String,
}

impl ValidateFailure {
    /// `阶段：细节`。
    #[must_use]
    pub fn message(&self) -> String {
        format!("{}：{}", self.stage.label(), self.detail)
    }
}

/// 工具名 → DAG → 拓扑分层；成功时返回分层表（即执行顺序）。
///
/// 三段的先后不能换：工具名先查，才能让「拼错 `tool_name`」在 DAG 报「未知依赖」之前就暴露。
pub fn validate_workflow_spec(spec: &WorkflowSpec) -> Result<Vec<Vec<String>>, ValidateFailure> {
    validate_tool_names(spec).map_err(|detail| ValidateFailure {
        stage: ValidateStage::ToolNames,
        detail,
    })?;
    validate_dag(&spec.nodes).map_err(|detail| ValidateFailure {
        stage: ValidateStage::Dag,
        detail,
    })?;
    topo_layers(&spec.nodes).map_err(|detail| ValidateFailure {
        stage: ValidateStage::Topo,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::{ValidateStage, validate_workflow_spec};
    use crate::cm_workflow::model::WorkflowSpec;
    use crate::cm_workflow::parse::parse_workflow_spec;

    fn spec(json: &str) -> WorkflowSpec {
        parse_workflow_spec(json).expect("parse workflow spec")
    }

    /// 合法 DAG：返回分层表——`workflow validate --json` 与 `validate_only` 用的是同一份。
    #[test]
    fn valid_spec_returns_layers() {
        let s = spec(
            r#"{"workflow":{"nodes":[
                {"id":"a","tool_name":"json_format","tool_args":{},"deps":[]},
                {"id":"b","tool_name":"json_format","tool_args":{},"deps":["a"]}
            ]}}"#,
        );
        let layers = validate_workflow_spec(&s).expect("should validate");
        assert_eq!(layers, vec![vec!["a".to_string()], vec!["b".to_string()]]);
    }

    /// 拼错 `tool_name` 必须落在 `ToolNames` 阶段——不能漏到 DAG 阶段被报成「未知依赖」。
    #[test]
    fn unknown_tool_name_is_tool_names_stage() {
        let s = spec(
            r#"{"workflow":{"nodes":[
                {"id":"a","tool_name":"no_such_tool","tool_args":{},"deps":[]}
            ]}}"#,
        );
        let fail = validate_workflow_spec(&s).expect_err("unknown tool must fail");
        assert_eq!(fail.stage, ValidateStage::ToolNames);
        assert!(
            fail.message().starts_with("工具名校验失败："),
            "`message()` 必须是「阶段：细节」，实际 {}",
            fail.message()
        );
    }

    /// 未知依赖落在 `Dag` 阶段；措辞同样带阶段前缀（工具信封与日志都靠它）。
    #[test]
    fn unknown_dependency_is_dag_stage() {
        let s = spec(
            r#"{"workflow":{"nodes":[
                {"id":"a","tool_name":"json_format","tool_args":{},"deps":["ghost"]}
            ]}}"#,
        );
        let fail = validate_workflow_spec(&s).expect_err("unknown dep must fail");
        assert_eq!(fail.stage, ValidateStage::Dag);
        assert!(
            fail.message().starts_with("DAG 校验失败："),
            "{}",
            fail.message()
        );
    }
}
