//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_code_stats() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"可选：统计的子路径（相对工作区，默认 .）"},
            "format":{"type":"string","description":"输出格式：table（默认）/json","enum":["table","json"]}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_dependency_graph() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "format":{"type":"string","description":"输出格式：mermaid（默认）/dot/tree","enum":["mermaid","dot","tree"]},
            "depth":{"type":"integer","description":"依赖树深度（仅 Cargo），默认 1，上限 10","minimum":0,"maximum":10},
            "kind":{"type":"string","description":"项目类型：auto（默认，按标记文件自动检测）/rust/go/npm","enum":["auto","rust","cargo","go","npm","node"]}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_coverage_report() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"可选：覆盖率报告文件路径（相对工作区）。省略时自动检测 lcov.info / tarpaulin-report.json / cobertura.xml 等"},
            "format":{"type":"string","description":"报告格式：auto（默认，按文件后缀/内容自动检测）/lcov/tarpaulin/cobertura","enum":["auto","lcov","tarpaulin","tarpaulin_json","cobertura"]}
        },
        "required":[]
    })
}

// ── 文件工具增强 ────────────────────────────────────────────
