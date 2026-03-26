//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_frontend_lint() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "subdir": { "type": "string", "description": "可选：前端目录相对路径，默认 frontend" },
            "script": { "type": "string", "description": "可选：npm script 名称，默认 lint" }
        },
        "required": []
    })
}

#[allow(dead_code)] // 供后续注册独立 Python 工具或聚合参数时复用
pub(in crate::tools) fn params_ruff_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：相对工作区根的检查路径列表；默认 [\".\"]。禁止绝对路径与 .."
            }
        },
        "required": []
    })
}

#[allow(dead_code)]
pub(in crate::tools) fn params_pytest_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "test_path": { "type": "string", "description": "可选：相对工作区的测试文件或目录；空则整库" },
            "keyword": { "type": "string", "description": "可选：pytest -k 表达式（禁止 shell 元字符）" },
            "markers": { "type": "string", "description": "可选：pytest -m 标记表达式" },
            "quiet": { "type": "boolean", "description": "可选：是否加 -q，默认 true" },
            "maxfail": { "type": "integer", "description": "可选：--maxfail，默认不传", "minimum": 1 },
            "nocapture": { "type": "boolean", "description": "可选：是否 --capture=no，默认 false" }
        },
        "required": []
    })
}

#[allow(dead_code)]
pub(in crate::tools) fn params_mypy_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：相对工作区的检查路径，默认 [\".\"]"
            },
            "strict": { "type": "boolean", "description": "可选：是否传 --strict，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_python_install_editable() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "backend": {
                "type": "string",
                "description": "包管理后端：uv（uv pip install -e .）或 pip（python3 -m pip install -e .）",
                "enum": ["uv", "pip"]
            }
        },
        "required": ["backend"]
    })
}

pub(in crate::tools) fn params_uv_sync() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "frozen": { "type": "boolean", "description": "可选：是否传 --frozen（与 lock 严格一致），默认 false" },
            "no_dev": { "type": "boolean", "description": "可选：是否传 --no-dev，默认 false" },
            "all_packages": { "type": "boolean", "description": "可选：是否传 --all-packages（workspace），默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_uv_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给 `uv run` 的参数列表（必填、非空），如 [\"pytest\",\"-q\"]、[\"ruff\",\"check\",\".\"]。禁止空白与 shell 元字符，逐项不经 shell 解析"
            }
        },
        "required": ["args"]
    })
}
