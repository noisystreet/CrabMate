//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_golangci_lint() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "fix":{"type":"boolean","description":"是否 --fix 自动修复，默认 false"},
            "fast":{"type":"boolean","description":"是否 --fast 快速模式，默认 false"}
        },
        "required":[]
    })
}

// ── 进程与端口管理 ──────────────────────────────────────────
