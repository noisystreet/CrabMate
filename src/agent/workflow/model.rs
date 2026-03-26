//! 工作流 JSON 解析后的规格（节点、依赖、并行策略等）。

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
}
