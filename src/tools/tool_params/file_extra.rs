//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_delete_file() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"要删除的文件路径（相对工作区，必填）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_delete_dir() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"要删除的目录路径（相对工作区，必填）"},
            "recursive":{"type":"boolean","description":"是否递归删除（含子目录和文件），默认 false（仅删空目录）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_append_file() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"文件路径（相对工作区，必填）"},
            "content":{"type":"string","description":"要追加的内容"},
            "create_if_missing":{"type":"boolean","description":"文件不存在时是否创建，默认 false"}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_create_dir() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"要创建的目录路径（相对工作区，必填）"},
            "parents":{"type":"boolean","description":"是否递归创建父目录（类似 mkdir -p），默认 true"}
        },
        "required":["path"]
    })
}

pub(in crate::tools) fn params_search_replace() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"目标文件路径（相对工作区，必填）。**必须先用 read_dir 确认文件存在**，禁止直接假设某个文件存在。"},
            "search":{"type":"string","description":"要搜索的字符串或正则表达式（必填）"},
            "replace":{"type":"string","description":"替换为的字符串（默认空字符串，即删除匹配）"},
            "regex":{"type":"boolean","description":"是否将 search 作为正则表达式，默认 false（字面量匹配）"},
            "max_replacements":{"type":"integer","description":"最多替换次数（0=全部替换），默认 0","minimum":0},
            "dry_run":{"type":"boolean","description":"默认 true：仅预览替换结果，不修改文件"},
            "confirm":{"type":"boolean","description":"dry_run=false 时需要 confirm=true 才会实际写盘"}
        },
        "required":["path","search"]
    })
}

pub(in crate::tools) fn params_chmod_file() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"文件或目录路径（相对工作区，必填）"},
            "mode":{"type":"string","description":"八进制权限值（必填），如 \"755\"、\"644\"、\"700\""},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["path","mode"]
    })
}

pub(in crate::tools) fn params_symlink_info() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"要检查的路径（相对工作区，必填）"}
        },
        "required":["path"]
    })
}
