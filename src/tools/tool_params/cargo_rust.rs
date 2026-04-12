//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_cargo_common() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "release": { "type": "boolean", "description": "可选：是否使用 --release，默认 false" },
            "all_targets": { "type": "boolean", "description": "可选：是否使用 --all-targets（check/clippy 生效）" },
            "package": { "type": "string", "description": "可选：指定 package 名（--package）" },
            "bin": { "type": "string", "description": "可选：指定 bin 目标（--bin）" },
            "features": { "type": "string", "description": "可选：特性列表（--features），如 \"feat1,feat2\"" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "release": { "type": "boolean", "description": "可选：是否使用 --release，默认 false" },
            "package": { "type": "string", "description": "可选：指定 package 名（--package）" },
            "bin": { "type": "string", "description": "可选：指定 bin 目标（--bin）" },
            "features": { "type": "string", "description": "可选：特性列表（--features）" },
            "test_filter": { "type": "string", "description": "可选：测试名过滤（紧跟 cargo test 后）" },
            "nocapture": { "type": "boolean", "description": "可选：是否传递 -- --nocapture" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "release": { "type": "boolean", "description": "可选：是否使用 --release，默认 false" },
            "package": { "type": "string", "description": "可选：指定 package 名（--package）" },
            "bin": { "type": "string", "description": "可选：指定 bin 目标（--bin）" },
            "features": { "type": "string", "description": "可选：特性列表（--features）" },
            "args": { "type": "array", "items": { "type": "string" }, "description": "可选：传给可执行程序的参数列表" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_rust_test_one() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "test_name": { "type": "string", "description": "要运行的测试名/过滤串（必填）" },
            "package": { "type": "string", "description": "可选：指定 package 名（--package）" },
            "bin": { "type": "string", "description": "可选：指定 bin 目标（--bin）" },
            "features": { "type": "string", "description": "可选：特性列表（--features）" },
            "nocapture": { "type": "boolean", "description": "可选：是否传递 -- --nocapture" }
        },
        "required": ["test_name"]
    })
}

pub(in crate::tools) fn params_cargo_metadata() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "no_deps": { "type": "boolean", "description": "可选：是否添加 --no-deps，默认 true" },
            "format_version": { "type": "integer", "description": "可选：metadata 格式版本，默认 1", "minimum": 1 }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_tree() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：指定 package（--package）" },
            "invert": { "type": "string", "description": "可选：反向依赖某个 crate（--invert）" },
            "depth": { "type": "integer", "description": "可选：依赖树深度（--depth）", "minimum": 0 },
            "edges": { "type": "string", "description": "可选：依赖边类型（--edges），如 normal,build,dev" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_clean() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：指定 package（--package）" },
            "release": { "type": "boolean", "description": "可选：仅清理 release 产物（--release）" },
            "doc": { "type": "boolean", "description": "可选：仅清理文档产物（--doc）" },
            "dry_run": { "type": "boolean", "description": "可选：只预览将删除内容（--dry-run），默认 true" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_doc() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：指定 package（--package）" },
            "no_deps": { "type": "boolean", "description": "可选：是否使用 --no-deps，默认 true" },
            "open": { "type": "boolean", "description": "可选：是否执行 --open（需图形环境）" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_nextest() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：指定 package（--package）" },
            "profile": { "type": "string", "description": "可选：nextest profile（--profile）" },
            "test_filter": { "type": "string", "description": "可选：测试过滤串" },
            "nocapture": { "type": "boolean", "description": "可选：透传 -- --nocapture" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_fmt_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}

pub(in crate::tools) fn params_cargo_outdated() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "workspace": { "type":"boolean", "description":"可选：是否传 --workspace（检查整个 workspace）" },
            "depth": { "type":"integer", "description":"可选：依赖树深度（--depth）", "minimum":0 }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_cargo_machete() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "with_metadata": { "type":"boolean", "description":"可选：传 --with-metadata（调用 cargo metadata，更准但更慢，可能改动 Cargo.lock）" },
            "path": { "type":"string", "description":"可选：相对工作区的子目录，传给 cargo machete <path>；不可含 .." }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_cargo_udeps() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "nightly": { "type":"boolean", "description":"可选：为 true 时执行 cargo +nightly udeps（cargo-udeps 通常需要 nightly）" }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_cargo_publish_dry_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：workspace 中指定包名（--package）" },
            "allow_dirty": { "type": "boolean", "description": "可选：--allow-dirty，默认 false" },
            "no_verify": { "type": "boolean", "description": "可选：--no-verify，默认 false（跳过发布前的构建验证）" },
            "features": { "type": "string", "description": "可选：--features" },
            "all_features": { "type": "boolean", "description": "可选：--all-features，默认 false" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_rust_rustc() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给 rustc 的参数（不含程序名）；在工作区根目录执行。与 run_command 相同：每项不得含 .. 或以 / 开头。例：[\"--explain\",\"E0382\"]、[\"-vV\"]、[\"--print\",\"cfg\"]"
            }
        },
        "required": ["args"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_rust_compiler_json() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "all_targets": { "type": "boolean", "description": "可选：--all-targets，默认 false" },
            "package": { "type": "string", "description": "可选：--package" },
            "features": { "type": "string", "description": "可选：--features" },
            "all_features": { "type": "boolean", "description": "可选：--all-features，默认 false" },
            "max_diagnostics": { "type": "integer", "description": "可选：最多汇总多少条 compiler-message，默认 120，上限 500", "minimum": 1 },
            "message_format": {
                "type": "string",
                "description": "传给 cargo 的 --message-format，默认 json；也可用 json-diagnostic-short 等"
            }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_rust_analyzer_position() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的 .rs 源文件路径" },
            "line": { "type": "integer", "description": "光标行号，0-based（与 LSP 一致；第 10 行填 9）", "minimum": 0 },
            "character": { "type": "integer", "description": "可选：UTF-16 偏移列号，0-based，默认 0", "minimum": 0 },
            "server_path": { "type": "string", "description": "可选：rust-analyzer 可执行文件路径，默认在 PATH 中查找 rust-analyzer" },
            "wait_after_open_ms": { "type": "integer", "description": "可选：didOpen 后等待索引的毫秒数，默认 500，上限 5000", "minimum": 0 }
        },
        "required": ["path", "line"]
    })
}

pub(in crate::tools) fn params_rust_analyzer_references() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的 .rs 源文件路径" },
            "line": { "type": "integer", "description": "0-based 行号", "minimum": 0 },
            "character": { "type": "integer", "description": "可选：0-based 列号，默认 0", "minimum": 0 },
            "include_declaration": { "type": "boolean", "description": "可选：references 是否包含定义处，默认 true" },
            "server_path": { "type": "string", "description": "可选：rust-analyzer 可执行路径" },
            "wait_after_open_ms": { "type": "integer", "description": "可选：didOpen 后等待毫秒，默认 500", "minimum": 0 }
        },
        "required": ["path", "line"]
    })
}

pub(in crate::tools) fn params_rust_analyzer_hover() -> serde_json::Value {
    params_rust_analyzer_position()
}

pub(in crate::tools) fn params_rust_analyzer_document_symbol() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的 .rs 源文件路径" },
            "max_symbols": { "type": "integer", "description": "可选：最多输出多少条符号（树或扁平列表遍历计数），默认 500，范围 1–5000", "minimum": 1, "maximum": 5000 },
            "server_path": { "type": "string", "description": "可选：rust-analyzer 可执行路径" },
            "wait_after_open_ms": { "type": "integer", "description": "可选：didOpen 后等待毫秒，默认 500，上限 5000", "minimum": 0 }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_cargo_fix() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "confirm": { "type":"boolean", "description":"是否真正应用修复（必须 true）" },
            "broken_code": { "type":"boolean", "description":"可选：即使仍有编译错误也应用修复（--broken-code）" },
            "all_targets": { "type":"boolean", "description":"可选：固定传 --all-targets（--all-targets）" },
            "package": { "type":"string", "description":"可选：仅修复指定 package（--package）" },
            "features": { "type":"string", "description":"可选：--features 传入特性列表（逗号或空格分隔由 cargo 接受）" },
            "all_features": { "type":"boolean", "description":"可选：--all-features" },
            "edition": { "type":"string", "description":"可选：应用到某个 edition（--edition）" },
            "edition_idioms": { "type":"boolean", "description":"可选：--edition-idioms" },
            "allow_dirty": { "type":"boolean", "description":"可选：允许工作区有改动时也修复（--allow-dirty）" },
            "allow_staged": { "type":"boolean", "description":"可选：允许有暂存改动（--allow-staged）" },
            "allow_no_vcs": { "type":"boolean", "description":"可选：即使未检测到 VCS 也允许（--allow-no-vcs）" }
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_cargo_audit() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "deny_warnings": { "type": "boolean", "description": "可选：是否添加 --deny warnings" },
            "json": { "type": "boolean", "description": "可选：是否使用 --json 输出" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_cargo_deny() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "checks": { "type": "string", "description": "可选：检查项，默认 \"advisories licenses bans sources\"" },
            "all_features": { "type": "boolean", "description": "可选：是否启用 --all-features" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_rust_file_outline() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"相对工作区的 .rs 文件路径（必填）" },
            "include_use": { "type":"boolean", "description":"可选：是否列出 use 行（默认 false，减少噪音）" },
            "max_items": { "type":"integer", "description":"可选：最多列出条目数，默认 200，上限 500", "minimum":1 }
        },
        "required":["path"],
        "additionalProperties":false
    })
}
