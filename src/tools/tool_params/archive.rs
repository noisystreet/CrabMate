//! 归档工具 JSON 参数 schema

pub(in crate::tools) fn params_archive_pack() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "output": {
                "type": "string",
                "description": "输出归档文件路径（相对路径）。根据扩展名自动选择格式：.zip、.tar、.tar.gz/.tgz、.tar.bz2/.tbz2、.tar.xz/.txz"
            },
            "sources": {
                "type": "array",
                "items": { "type": "string" },
                "description": "要打包的文件或目录路径列表（相对路径）"
            },
            "exclude": {
                "type": "array",
                "items": { "type": "string" },
                "description": "排除模式列表（可选），如 [\"*.tmp\", \".git\"]"
            },
            "format": {
                "type": "string",
                "enum": ["auto", "tar", "zip", "tar.gz", "tar.bz2", "tar.xz"],
                "description": "归档格式（可选，默认 auto 根据扩展名自动检测）"
            }
        },
        "required": ["output", "sources"]
    })
}

pub(in crate::tools) fn params_archive_unpack() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "archive": {
                "type": "string",
                "description": "归档文件路径（相对路径）。支持格式：.zip、.tar、.tar.gz/.tgz、.tar.bz2/.tbz2、.tar.xz/.txz、.7z、.rar"
            },
            "output_dir": {
                "type": "string",
                "description": "解压输出目录（可选，默认为当前目录）"
            },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "只解压指定文件（可选，默认为全部）"
            },
            "strip_components": {
                "type": "integer",
                "description": "解压时去掉前 N 层目录（可选，类似 tar --strip-components）"
            }
        },
        "required": ["archive"]
    })
}

pub(in crate::tools) fn params_archive_list() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "archive": {
                "type": "string",
                "description": "归档文件路径（相对路径）"
            },
            "verbose": {
                "type": "boolean",
                "description": "显示详细信息（大小、修改时间、权限）"
            }
        },
        "required": ["archive"]
    })
}
