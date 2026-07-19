//! 工作流 JSON 解析后的规格（节点、依赖、并行策略等）。

use super::node_tool_role::WorkflowNodeToolRole;

#[derive(Debug, Clone)]
pub struct WorkflowSpec {
    pub max_parallelism: usize,
    pub fail_fast: bool,
    pub compensate_on_failure: bool,
    pub output_inject_max_chars: usize,
    pub summary_preview_max_chars: usize,
    pub compensation_preview_max_chars: usize,
    pub nodes: Vec<WorkflowNodeSpec>,
    /// 解析时预计算的拓扑层数（Kahn 算法），避免 validate/execute 路径重复计算。
    pub cached_layer_count: usize,
    /// 运行时由 `from` 节点结果展开（`for_each` + `json_path`）。
    pub for_each_pending: Vec<ForEachPendingSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowBranch {
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub enum WorkflowRunIf {
    Branch {
        from: String,
        branch: WorkflowBranch,
    },
    Match {
        from: String,
        field: String,
        equals: Option<serde_json::Value>,
        in_list: Option<Vec<serde_json::Value>>,
    },
}

/// 待运行时展开的 `for_each`（`json_path` 路径；`static_items` 在编译期展开）。
#[derive(Debug, Clone)]
pub struct ForEachPendingSpec {
    pub base_id: String,
    pub from: String,
    pub json_path: Option<String>,
    pub static_items: Option<Vec<String>>,
    pub item_var: String,
    pub max_items: usize,
    pub parallel: bool,
    pub tool_name: String,
    pub tool_args_template: serde_json::Value,
    pub requires_approval: bool,
    pub timeout_secs: Option<u64>,
    pub compensate_with: Vec<String>,
    pub max_retries: u32,
    pub node_tool_role: Option<WorkflowNodeToolRole>,
    pub extra_deps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkflowNodeSpec {
    pub id: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub deps: Vec<String>,
    pub requires_approval: bool,
    pub timeout_secs: Option<u64>,
    pub compensate_with: Vec<String>,
    pub max_retries: u32,
    /// 可选：收窄该节点允许调用的工具类别；与分阶段 `executor_kind` 共用策略表。
    pub node_tool_role: Option<WorkflowNodeToolRole>,
    /// 条件执行（作者层 `when` 编译）；不满足时节点 `skipped`。
    pub run_if: Option<WorkflowRunIf>,
}
