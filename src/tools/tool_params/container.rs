//! 容器工具 JSON Schema。

pub(in crate::tools) fn params_docker_build() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "context": { "type": "string", "description": "构建上下文相对路径，默认 \".\"；禁止 .. 与绝对路径" },
            "tag": { "type": "string", "description": "镜像 tag，默认 crabmate-local:latest" },
            "dockerfile": { "type": "string", "description": "可选：-f Dockerfile 相对路径" },
            "no_cache": { "type": "boolean", "description": "可选：是否 --no-cache，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_docker_compose_ps() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "project": { "type": "string", "description": "可选：-p 项目名，仅字母数字与 _-" },
            "compose_files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：多个 -f compose 文件相对路径"
            }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_podman_images() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "reference": { "type": "string", "description": "可选：过滤镜像引用（repository:tag 等），空则列出全部" }
        },
        "required": []
    })
}
