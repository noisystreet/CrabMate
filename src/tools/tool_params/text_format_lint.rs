//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_table_text() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "description": "preview：抽样预览；validate：检查每行列数是否一致；select_columns：按 0 起列下标抽取为 TSV；filter_rows：按列 equals/contains 筛选行；aggregate：对列做 sum/mean/min/max/count 等",
                "enum": ["preview", "validate", "select_columns", "filter_rows", "aggregate"]
            },
            "path": { "type": "string", "description": "相对工作区的表格文件（与 text 二选一）；单文件上限 4MiB" },
            "text": { "type": "string", "description": "内联 CSV/TSV 文本（上限 256KiB）；与 path 二选一" },
            "delimiter": {
                "type": "string",
                "description": "auto（默认：.tsv→tab、.csv→comma，否则按首行 sniff）、comma/csv、tab/tsv、semicolon、pipe",
                "enum": ["auto", "comma", "csv", "tab", "tsv", "semicolon", "pipe"]
            },
            "has_header": { "type": "boolean", "description": "首行是否为表头；aggregate/filter/select 在 true 时会跳过首行数据，默认 true" },
            "preview_rows": { "type": "integer", "description": "preview：预览数据行数，默认 20，上限 200", "minimum": 1, "maximum": 200 },
            "max_rows_scan": { "type": "integer", "description": "validate/aggregate：最多扫描的数据行，默认 200000，上限 200000" },
            "columns": {
                "type": "array",
                "items": { "type": "integer", "minimum": 0 },
                "description": "select_columns：要保留的列下标（从 0 开始）"
            },
            "column": { "type": "integer", "description": "filter_rows/aggregate：列下标（从 0 开始）", "minimum": 0 },
            "equals": { "type": "string", "description": "filter_rows：该列全等匹配" },
            "contains": { "type": "string", "description": "filter_rows：该列子串匹配（与 equals 二选一）" },
            "op": {
                "type": "string",
                "description": "aggregate：count_non_empty、count_numeric、sum、mean、min、max",
                "enum": ["count", "count_non_empty", "count_numeric", "sum", "mean", "avg", "min", "max"]
            },
            "max_output_rows": { "type": "integer", "description": "select_columns/filter_rows：最多输出行数，默认 500，上限 10000", "minimum": 1, "maximum": 10000 }
        },
        "required": ["action"]
    })
}

pub(in crate::tools) fn params_text_diff() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "description": "inline：比较 left 与 right 字符串；paths：比较工作区内 left_path 与 right_path 两个 UTF-8 文件",
                "enum": ["inline", "paths"]
            },
            "left": { "type": "string", "description": "mode=inline 时左侧文本（单侧最多 256KiB）" },
            "right": { "type": "string", "description": "mode=inline 时右侧文本" },
            "left_path": { "type": "string", "description": "mode=paths 时相对工作区的左文件路径" },
            "right_path": { "type": "string", "description": "mode=paths 时相对工作区的右文件路径" },
            "context_lines": {
                "type": "integer",
                "description": "unified diff 上下文行数，默认 3，上限 20；可为 0",
                "minimum": 0,
                "maximum": 20
            },
            "max_output_bytes": {
                "type": "integer",
                "description": "diff 正文最大字节，默认 50000，上限 500000",
                "minimum": 1,
                "maximum": 500000
            }
        },
        "required": []
    })
}
