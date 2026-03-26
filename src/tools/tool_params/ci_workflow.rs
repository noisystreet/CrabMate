//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_backtrace_analyze() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "backtrace": { "type": "string", "description": "panic/backtrace 原文（必填）" },
            "crate_hint": { "type": "string", "description": "可选：业务 crate 名提示，用于过滤调用栈" }
        },
        "required": ["backtrace"]
    })
}

pub(in crate::tools) fn params_ci_pipeline_local() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "run_fmt": { "type": "boolean", "description": "是否运行 cargo fmt --check，默认 true" },
            "run_clippy": { "type": "boolean", "description": "是否运行 cargo clippy，默认 true" },
            "run_test": { "type": "boolean", "description": "是否运行 cargo test，默认 true" },
            "run_frontend_lint": { "type": "boolean", "description": "是否运行 frontend lint，默认 true" },
            "run_ruff_check": { "type": "boolean", "description": "是否运行 ruff check（无 Python 项目标记时跳过），默认 true" },
            "run_pytest": { "type": "boolean", "description": "是否运行 python3 -m pytest（较慢，默认 false）" },
            "run_mypy": { "type": "boolean", "description": "是否运行 mypy（默认 false）" },
            "fail_fast": { "type": "boolean", "description": "是否在首个失败步骤后立即停止，默认 false" },
            "summary_only": { "type": "boolean", "description": "是否仅输出步骤通过/失败/跳过统计，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_release_ready_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "run_ci": { "type": "boolean", "description": "是否运行 ci_pipeline_local（默认 true）" },
            "run_audit": { "type": "boolean", "description": "是否运行 cargo_audit（默认 true）" },
            "run_deny": { "type": "boolean", "description": "是否运行 cargo_deny（默认 true）" },
            "require_clean_worktree": { "type": "boolean", "description": "是否要求 Git 工作区干净（默认 true）" },
            "fail_fast": { "type": "boolean", "description": "失败后是否立即停止（默认 false）" },
            "summary_only": { "type": "boolean", "description": "仅输出汇总（默认 true）" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_workflow_execute() -> serde_json::Value {
    // schema 保持宽松：workflow 内部 nodes/dag 结构由运行时解析并做 DAG 校验。
    serde_json::json!({
        "type": "object",
        "properties": {
            "workflow": { "type": "object", "description": "DAG 工作流定义：max_parallelism/fail_fast/compensate_on_failure + nodes" }
        },
        "required": ["workflow"],
        "additionalProperties": false
    })
}
