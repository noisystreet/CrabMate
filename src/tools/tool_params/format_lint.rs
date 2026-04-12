//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_format_check_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区根目录的文件路径；支持 .rs、.py（ruff format --check）、ts/tsx/js/jsx/json（prettier --check）"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_quality_workspace() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "run_cargo_fmt_check": { "type":"boolean", "description":"可选：cargo fmt --check，默认 true" },
            "run_cargo_check": { "type":"boolean", "description":"可选：cargo check --all-targets，默认 false（与 clippy 互补；显式开启可更快暴露纯编译错误）" },
            "run_cargo_clippy": { "type":"boolean", "description":"可选：cargo clippy --all-targets，默认 true" },
            "run_cargo_test": { "type":"boolean", "description":"可选：cargo test，默认 false（较慢）" },
            "run_frontend_lint": { "type":"boolean", "description":"可选：npm run lint（未指定 subdir 时优先 frontend / frontend-leptos 中含 package.json 的目录），默认 false" },
            "run_frontend_build": { "type":"boolean", "description":"可选：npm run build（同上目录启发式），默认 false" },
            "run_frontend_prettier_check": { "type":"boolean", "description":"可选：npx prettier --check .（同上），默认 false" },
            "run_ruff_check": { "type":"boolean", "description":"可选：ruff check，默认 false（无 Python 项目时跳过）" },
            "run_pytest": { "type":"boolean", "description":"可选：python3 -m pytest，默认 false" },
            "run_mypy": { "type":"boolean", "description":"可选：mypy，默认 false" },
            "run_maven_compile": { "type":"boolean", "description":"可选：mvn -q compile（须 pom.xml），默认 false" },
            "run_maven_test": { "type":"boolean", "description":"可选：mvn -q test，默认 false" },
            "run_gradle_compile": { "type":"boolean", "description":"可选：gradle -q classes（或 tasks），默认 false" },
            "run_gradle_test": { "type":"boolean", "description":"可选：gradle -q test，默认 false" },
            "run_docker_compose_ps": { "type":"boolean", "description":"可选：docker compose ps，默认 false" },
            "run_podman_images": { "type":"boolean", "description":"可选：podman images，默认 false" },
            "fail_fast": { "type":"boolean", "description":"可选：遇首个失败即停止后续步骤，默认 true" },
            "summary_only": { "type":"boolean", "description":"可选：仅输出各步骤 passed/failed 汇总，默认 false" }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_format_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区根目录的文件路径，如 src/main.rs、frontend-leptos/src/lib.rs、src/pkg/__init__.py、src/foo.cpp（.py 使用 ruff format；.c/.h/.cpp 等使用 clang-format）"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_run_lints() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "run_cargo": {
                "type": "boolean",
                "description": "是否运行 Rust 侧检查（cargo check + cargo clippy），默认为 true"
            },
            "run_cargo_check": {
                "type": "boolean",
                "description": "在 clippy 之前是否先运行 cargo check --all-targets；run_cargo 为 true 时默认 true"
            },
            "run_frontend": {
                "type": "boolean",
                "description": "是否运行 npm run lint（未传 subdir 时按 frontend / frontend-leptos 启发式选目录），默认为 true"
            },
            "run_frontend_build": {
                "type": "boolean",
                "description": "是否额外运行 npm run build，默认为 false"
            },
            "run_python_ruff": {
                "type": "boolean",
                "description": "是否运行 ruff check（有 Python 项目标记时），默认为 true"
            }
        },
        "required": []
    })
}
