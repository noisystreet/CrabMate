pub(super) fn params_get_current_time() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "description": "输出模式：time（仅时间）、calendar（仅日历）、both（时间+日历）。默认 time。",
                "enum": ["time", "calendar", "both"]
            },
            "year": {
                "type": "integer",
                "description": "可选：日历年份（仅在 mode=calendar/both 时生效）"
            },
            "month": {
                "type": "integer",
                "description": "可选：日历月份 1-12（仅在 mode=calendar/both 时生效）",
                "minimum": 1,
                "maximum": 12
            }
        },
        "required": []
    })
}

pub(super) fn params_calc() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "数学表达式，如 1+2*3、2^10、sqrt(2)、s(pi/2)、math::log10(100)"
            }
        },
        "required": ["expression"]
    })
}

pub(super) fn params_convert_units() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "description": "物理量类别：length（长度）、mass（质量）、temperature（温度）、data（信息量：bit/byte/KB/MB…与 KiB/MiB…）、time（时间）、area（面积）、pressure（压强）、speed（速度）。可用英文或中文别名（如 温度、数据量）。",
                "enum": [
                    "length", "mass", "temperature", "data", "time", "area", "pressure", "speed",
                    "距离", "长度", "质量", "重量", "温度", "存储", "数据量", "时间", "时长", "面积", "压强", "压力", "速度"
                ]
            },
            "value": {
                "type": "number",
                "description": "待换算的数值（有限浮点数）"
            },
            "from": {
                "type": "string",
                "description": "源单位符号或别名。示例：长度 km、m、mile、英尺；温度 C、F、K；数据 GiB、MB、byte；时间 h、min、s；速度 m/s、km/h、mph；压强 Pa、bar、atm。"
            },
            "to": {
                "type": "string",
                "description": "目标单位，与 from 同类别。"
            }
        },
        "required": ["category", "value", "from", "to"],
        "additionalProperties": false
    })
}

pub(super) fn params_weather() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "city": {
                "type": "string",
                "description": "城市或地区名，如北京、上海、Tokyo"
            },
            "location": {
                "type": "string",
                "description": "与 city 同义，城市或地区名"
            }
        },
        "required": []
    })
}

pub(super) fn params_web_search() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "搜索关键词或问句（联网检索网页摘要）"
            },
            "max_results": {
                "type": "integer",
                "description": "返回条数上限，1～20，默认取配置 web_search_max_results",
                "minimum": 1,
                "maximum": 20
            }
        },
        "required": ["query"]
    })
}

pub(super) fn params_http_fetch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "完整 http(s) URL。Web 仅允许匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）；TUI 未匹配时可人工审批（与 run_command 相同）。"
            },
            "method": {
                "type": "string",
                "description": "HTTP 方法：GET（默认，返回正文截断）或 HEAD（仅状态码、Content-Type、Content-Length、重定向链，不下载 body）",
                "enum": ["GET", "HEAD", "get", "head"]
            }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

pub(super) fn params_http_request() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "完整 http(s) URL。仅允许匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。"
            },
            "method": {
                "type": "string",
                "description": "HTTP 方法：POST / PUT / PATCH / DELETE（大小写均可）。",
                "enum": ["POST", "PUT", "PATCH", "DELETE", "post", "put", "patch", "delete"]
            },
            "json_body": {
                "description": "可选：JSON 请求体（任意合法 JSON 值）。序列化后上限 256KiB。",
                "oneOf": [
                    {"type":"object"},
                    {"type":"array"},
                    {"type":"string"},
                    {"type":"number"},
                    {"type":"integer"},
                    {"type":"boolean"},
                    {"type":"null"}
                ]
            }
        },
        "required": ["url", "method"],
        "additionalProperties": false
    })
}

pub(super) fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "命令名（小写），须为配置中 allowed_commands 白名单之一（如 ls、gcc、cmake、make、file 等）。**不要**用本工具运行工作区内的可执行文件（例如 ./main、./a.out、./build/app）；此类请改用 **run_executable**，参数 path 填相对工作目录的路径。"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给白名单命令的参数（可选）。**不要**用 args 拼出「执行当前目录下程序」——应使用 run_executable。"
            }
        },
        "required": ["command"]
    })
}

pub(super) fn params_run_executable() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的可执行文件路径（如 ./main、./a.out、./build/app）。编译或构建得到的程序应**优先用本工具运行**，不要用 run_command。"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "传给程序的参数（可选），如 [\"--help\"], [\"arg1\", \"arg2\"]"
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_package_query() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": {
                "type": "string",
                "description": "要查询的包名（如 bash、curl、openssl、libc6:amd64）。仅支持字母、数字及 . + - _ : @。"
            },
            "manager": {
                "type": "string",
                "description": "包管理器：auto（默认，优先 apt 后 rpm）、apt、rpm。",
                "enum": ["auto", "apt", "rpm"]
            }
        },
        "required": ["package"],
        "additionalProperties": false
    })
}

pub(super) fn params_cargo_common() -> serde_json::Value {
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

pub(super) fn params_cargo_test() -> serde_json::Value {
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

pub(super) fn params_cargo_run() -> serde_json::Value {
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

pub(super) fn params_rust_test_one() -> serde_json::Value {
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

pub(super) fn params_cargo_metadata() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "no_deps": { "type": "boolean", "description": "可选：是否添加 --no-deps，默认 true" },
            "format_version": { "type": "integer", "description": "可选：metadata 格式版本，默认 1", "minimum": 1 }
        },
        "required": []
    })
}

pub(super) fn params_cargo_tree() -> serde_json::Value {
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

pub(super) fn params_cargo_clean() -> serde_json::Value {
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

pub(super) fn params_cargo_doc() -> serde_json::Value {
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

pub(super) fn params_cargo_nextest() -> serde_json::Value {
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

pub(super) fn params_cargo_fmt_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}

pub(super) fn params_cargo_outdated() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "workspace": { "type":"boolean", "description":"可选：是否传 --workspace（检查整个 workspace）" },
            "depth": { "type":"integer", "description":"可选：依赖树深度（--depth）", "minimum":0 }
        },
        "required":[]
    })
}

pub(super) fn params_cargo_machete() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "with_metadata": { "type":"boolean", "description":"可选：传 --with-metadata（调用 cargo metadata，更准但更慢，可能改动 Cargo.lock）" },
            "path": { "type":"string", "description":"可选：相对工作区的子目录，传给 cargo machete <path>；不可含 .." }
        },
        "required":[]
    })
}

pub(super) fn params_cargo_udeps() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "nightly": { "type":"boolean", "description":"可选：为 true 时执行 cargo +nightly udeps（cargo-udeps 通常需要 nightly）" }
        },
        "required":[]
    })
}

pub(super) fn params_cargo_publish_dry_run() -> serde_json::Value {
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

pub(super) fn params_rust_compiler_json() -> serde_json::Value {
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

pub(super) fn params_rust_analyzer_position() -> serde_json::Value {
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

pub(super) fn params_rust_analyzer_references() -> serde_json::Value {
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

pub(super) fn params_cargo_fix() -> serde_json::Value {
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

pub(super) fn params_frontend_lint() -> serde_json::Value {
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
pub(super) fn params_ruff_check() -> serde_json::Value {
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
pub(super) fn params_pytest_run() -> serde_json::Value {
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
pub(super) fn params_mypy_check() -> serde_json::Value {
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

pub(super) fn params_python_install_editable() -> serde_json::Value {
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

pub(super) fn params_uv_sync() -> serde_json::Value {
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

pub(super) fn params_uv_run() -> serde_json::Value {
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

pub(super) fn params_go_build() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：go build 包路径或模式，默认 ./...；禁止 .. 与绝对路径" },
            "output": { "type": "string", "description": "可选：-o 输出可执行文件相对路径；禁止 .. 与绝对路径" },
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" }
        },
        "required": []
    })
}

pub(super) fn params_go_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：测试包路径，默认 ./...；禁止 .. 与绝对路径" },
            "run": { "type": "string", "description": "可选：-run 测试名过滤（保守字符集）" },
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" },
            "short": { "type": "boolean", "description": "可选：是否 -short，默认 false" },
            "count": { "type": "integer", "description": "可选：-count，须为正整数", "minimum": 1 },
            "timeout": { "type": "string", "description": "可选：-timeout，如 30s（短字符串、无空白）" }
        },
        "required": []
    })
}

pub(super) fn params_go_vet() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "package": { "type": "string", "description": "可选：go vet 包路径，默认 ./...；禁止 .. 与绝对路径" }
        },
        "required": []
    })
}

pub(super) fn params_go_mod_tidy() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "verbose": { "type": "boolean", "description": "可选：是否 -v，默认 false" },
            "confirm": { "type": "boolean", "description": "须为 true 才会执行（写回 go.mod/go.sum）" }
        },
        "required": []
    })
}

pub(super) fn params_go_fmt_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：传给 gofmt -l 的相对路径列表，默认 [\".\"]；禁止 .. 与绝对路径"
            }
        },
        "required": []
    })
}

pub(super) fn params_pre_commit_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "hook": { "type": "string", "description": "可选：仅运行指定 hook id（字母数字与 ._-）" },
            "all_files": { "type": "boolean", "description": "可选：是否 --all-files（与 files 同时存在时以 files 为准），默认 false" },
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：--files 相对路径列表；非空时不加 --all-files"
            },
            "verbose": { "type": "boolean", "description": "可选：是否 --verbose，默认 false" }
        },
        "required": []
    })
}

pub(super) fn params_typos_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：相对工作区根的待检查路径（文件或目录），默认 [\"README.md\",\"docs\"]；仅当路径存在时才会传入 typos，最多 24 项。禁止 .. 与绝对路径。"
            },
            "config_path": {
                "type": "string",
                "description": "可选：typos 配置文件相对路径（如 .typos.toml / typos.toml），可在配置中维护项目词典。"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(super) fn params_codespell_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：同 typos_check，默认 README.md 与 docs；只读检查，不会写回（封装层不传 -w）。"
            },
            "skip": {
                "type": "string",
                "description": "可选：传给 codespell 的 --skip（如 \"*.svg,*.lock\"），不含换行，最长 512 字符"
            },
            "dictionary_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：项目词典文件列表（相对路径，逐个传给 codespell -I，最多 8 项）。"
            },
            "ignore_words_list": {
                "type": "string",
                "description": "可选：传给 codespell -L 的逗号分隔忽略词列表（不含换行，最长 512 字符）。"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(super) fn params_ast_grep_run() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "ast-grep 模式串（如 Rust：`fn $NAME($$$) { $$$ }`）。单行建议；最长 4096 字符。"
            },
            "lang": {
                "type": "string",
                "description": "语言：rust、c/cpp、python、javascript、typescript、tsx、jsx、go、java、kotlin、bash、html、css（可用别名如 rs、py、ts）"
            },
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：搜索根路径（相对工作区），默认 [\"src\"]；仅存在路径会参与；最多 8 项。内置排除 target/node_modules/.git/vendor/dist/build。"
            },
            "globs": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：额外 --globs（如 \"!**/generated/**\"），最多 10 项；禁止 .. 与反引号等。"
            }
        },
        "required": ["pattern", "lang"],
        "additionalProperties": false
    })
}

pub(super) fn params_ast_grep_rewrite() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "ast-grep 模式串，最长 4096 字符"
            },
            "rewrite": {
                "type": "string",
                "description": "替换模板（ast-grep rewrite 模板），最长 4096 字符"
            },
            "lang": {
                "type": "string",
                "description": "语言：rust、c/cpp、python、javascript、typescript、tsx、jsx、go、java、kotlin、bash、html、css"
            },
            "paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：搜索根路径（相对工作区），默认 [\"src\"]；最多 8 项。"
            },
            "globs": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：额外 --globs，最多 10 项（禁止 .. 等非法字符）。"
            },
            "dry_run": {
                "type": "boolean",
                "description": "是否仅预览（默认 true）。false 时会写入文件。"
            },
            "confirm": {
                "type": "boolean",
                "description": "当 dry_run=false 时必须为 true，防止误改。"
            }
        },
        "required": ["pattern", "rewrite", "lang"],
        "additionalProperties": false
    })
}

pub(super) fn params_cargo_audit() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "deny_warnings": { "type": "boolean", "description": "可选：是否添加 --deny warnings" },
            "json": { "type": "boolean", "description": "可选：是否使用 --json 输出" }
        },
        "required": []
    })
}

pub(super) fn params_cargo_deny() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "checks": { "type": "string", "description": "可选：检查项，默认 \"advisories licenses bans sources\"" },
            "all_features": { "type": "boolean", "description": "可选：是否启用 --all-features" }
        },
        "required": []
    })
}

pub(super) fn params_backtrace_analyze() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "backtrace": { "type": "string", "description": "panic/backtrace 原文（必填）" },
            "crate_hint": { "type": "string", "description": "可选：业务 crate 名提示，用于过滤调用栈" }
        },
        "required": ["backtrace"]
    })
}

pub(super) fn params_ci_pipeline_local() -> serde_json::Value {
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

pub(super) fn params_release_ready_check() -> serde_json::Value {
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

pub(super) fn params_workflow_execute() -> serde_json::Value {
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

pub(super) fn params_git_status() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "porcelain": {
                "type": "boolean",
                "description": "可选：是否使用机器可读的 --porcelain 输出，默认 false"
            },
            "include_untracked": {
                "type": "boolean",
                "description": "可选：是否显示未跟踪文件，默认 true"
            },
            "branch": {
                "type": "boolean",
                "description": "可选：是否显示分支信息，默认 true"
            }
        },
        "required": []
    })
}

pub(super) fn params_git_clean_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

pub(super) fn params_git_diff() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "description": "diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum": ["working", "staged", "all"]
            },
            "path": {
                "type": "string",
                "description": "可选：仅查看某个相对路径（文件或目录）的 diff，如 src/main.rs"
            },
            "context_lines": {
                "type": "integer",
                "description": "可选：每处变更展示上下文行数（-U），默认 3",
                "minimum": 0
            }
        },
        "required": []
    })
}

pub(super) fn params_git_diff_stat() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{
                "type":"string",
                "description":"diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum":["working","staged","all"]
            },
            "path":{
                "type":"string",
                "description":"可选：仅查看某个相对路径（文件或目录）的 diff（例如 src/main.rs）"
            }
        },
        "required":[]
    })
}

pub(super) fn params_git_diff_names() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{
                "type":"string",
                "description":"diff 模式：working（未暂存）、staged（已暂存）、all（两者都看）。默认 working。",
                "enum":["working","staged","all"]
            },
            "path":{
                "type":"string",
                "description":"可选：仅查看某个相对路径（文件或目录）的变更文件名（例如 src/main.rs）"
            }
        },
        "required":[]
    })
}

pub(super) fn params_git_log() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 20","minimum":1},
            "oneline":{"type":"boolean","description":"可选：是否使用单行展示，默认 true"}
        },
        "required":[]
    })
}

pub(super) fn params_changelog_draft() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "since": {
                "type": "string",
                "description": "可选：范围起点（tag/提交/分支）；与 until 组成 since..until"
            },
            "until": {
                "type": "string",
                "description": "可选：范围终点；默认与 HEAD 组合见 since；都空则从 HEAD 回溯"
            },
            "max_commits": {
                "type": "integer",
                "description": "最多纳入多少条提交，默认 500，上限 2000",
                "minimum": 1,
                "maximum": 2000
            },
            "group_by": {
                "type": "string",
                "description": "聚合方式：date=按提交日；flat=平铺列表；tag_ranges 或 tags=按相邻 tag 区间（semver 降序，需至少 2 个 tag）",
                "enum": ["date", "flat", "tag_ranges", "tags"]
            },
            "max_tag_sections": {
                "type": "integer",
                "description": "tag_ranges 时最多几段区间（每段一对相邻 tag），默认 25，上限 100",
                "minimum": 1,
                "maximum": 100
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(super) fn params_license_notice() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "workspace_only": {
                "type": "boolean",
                "description": "仅列出工作区成员包（默认 false：含解析图中的传递依赖）"
            },
            "max_crates": {
                "type": "integer",
                "description": "表格最多多少行（按 crate 名去重后），默认 500，上限 3000",
                "minimum": 1,
                "maximum": 3000
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(super) fn params_git_show() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "rev":{"type":"string","description":"可选：提交号/引用，默认 HEAD"}
        },
        "required":[]
    })
}

pub(super) fn params_git_diff_base() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "base":{"type":"string","description":"可选：基准分支，默认 main（对比 base...HEAD）"},
            "context_lines":{"type":"integer","description":"可选：上下文行数，默认 3","minimum":0}
        },
        "required":[]
    })
}

pub(super) fn params_git_blame() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"相对路径（必填）"},
            "start_line":{"type":"integer","description":"可选：起始行（需和 end_line 一起使用）","minimum":1},
            "end_line":{"type":"integer","description":"可选：结束行（需和 start_line 一起使用）","minimum":1}
        },
        "required":["path"]
    })
}

pub(super) fn params_git_file_history() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"相对路径（必填）"},
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 30","minimum":1}
        },
        "required":["path"]
    })
}

pub(super) fn params_git_branch_list() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "include_remote":{"type":"boolean","description":"可选：是否包含远程分支，默认 true"}
        },
        "required":[]
    })
}

pub(super) fn params_git_stage_files() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "paths":{"type":"array","items":{"type":"string"},"description":"要暂存的相对路径列表（必填）"}
        },
        "required":["paths"]
    })
}

pub(super) fn params_git_commit() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "message":{"type":"string","description":"提交信息（必填）"},
            "stage_all":{"type":"boolean","description":"可选：提交前是否执行 git add -A，默认 false"},
            "confirm":{"type":"boolean","description":"安全确认；仅当 true 时才会执行 commit"}
        },
        "required":["message"]
    })
}

pub(super) fn params_git_fetch() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "remote":{"type":"string","description":"可选：远程名，如 origin"},
            "branch":{"type":"string","description":"可选：分支名（与 remote 一起使用）"},
            "prune":{"type":"boolean","description":"可选：是否 --prune，默认 false"}
        },
        "required":[]
    })
}

pub(super) fn params_git_remote_set_url() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"远程名（必填）"},
            "url":{"type":"string","description":"远程 URL（必填）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["name","url"]
    })
}

pub(super) fn params_git_apply() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "patch_path":{"type":"string","description":"补丁文件相对路径（必填）"},
            "check_only":{"type":"boolean","description":"是否仅检查可应用性，默认 true"}
        },
        "required":["patch_path"]
    })
}

pub(super) fn params_git_clone() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "repo_url":{"type":"string","description":"仓库 URL（必填）"},
            "target_dir":{"type":"string","description":"工作区内目标相对目录（必填）"},
            "depth":{"type":"integer","description":"可选：浅克隆深度（--depth）","minimum":1},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["repo_url","target_dir"]
    })
}

pub(super) fn params_empty_object() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}

pub(super) fn params_diagnostic_summary() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "include_toolchain": {
                "type": "boolean",
                "description": "是否输出 rustc/cargo/rustup/bc 与 OS 架构，默认 true"
            },
            "include_workspace_paths": {
                "type": "boolean",
                "description": "是否检查工作区 target/、Cargo.toml、frontend 等路径，默认 true"
            },
            "include_env": {
                "type": "boolean",
                "description": "是否列出关键环境变量仅状态（永不输出取值），默认 true"
            },
            "extra_env_vars": {
                "type": "array",
                "items": { "type": "string" },
                "description": "额外变量名，须为大写 [A-Z0-9_]+（如 CI）；与内置列表合并且去重"
            }
        },
        "required": []
    })
}

pub(super) fn params_file_write() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径，如 subdir/name.txt"
            },
            "content": {
                "type": "string",
                "description": "要写入的文件内容"
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_modify_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径"
            },
            "mode": {
                "type": "string",
                "description": "可选：full（默认）整文件覆盖；replace_lines 按行区间替换，流式读写适合大文件",
                "enum": ["full", "replace_lines"]
            },
            "content": {
                "type": "string",
                "description": "full 时为新的全文；replace_lines 时替换区间的新内容（可为空以删除这些行）"
            },
            "start_line": {
                "type": "integer",
                "description": "replace_lines 必填：起始行（1-based，含）",
                "minimum": 1
            },
            "end_line": {
                "type": "integer",
                "description": "replace_lines 必填：结束行（1-based，含）",
                "minimum": 1
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_file_from_to_overwrite() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "from": {
                "type": "string",
                "description": "源文件路径（相对工作目录）"
            },
            "to": {
                "type": "string",
                "description": "目标文件路径（相对工作目录）；父目录不存在时会创建"
            },
            "overwrite": {
                "type": "boolean",
                "description": "目标已存在且为文件时是否覆盖；默认 false"
            }
        },
        "required": ["from", "to"],
        "additionalProperties": false
    })
}

pub(super) fn params_read_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径，如 src/main.rs"
            },
            "start_line": {
                "type": "integer",
                "description": "可选：起始行（1-based，包含该行）；省略时默认为 1",
                "minimum": 1
            },
            "end_line": {
                "type": "integer",
                "description": "可选：结束行（1-based，包含该行）；省略时按 max_lines 自动截断到 start_line+max_lines-1 或 EOF",
                "minimum": 1
            },
            "max_lines": {
                "type": "integer",
                "description": "单次最多返回行数，默认 500，上限 8000；防止大文件一次读爆上下文",
                "minimum": 1,
                "maximum": 8000
            },
            "count_total_lines": {
                "type": "boolean",
                "description": "可选：是否额外扫描全文件统计总行数（大文件会多一次 I/O，默认 false）"
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_read_dir() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"可选：相对工作目录的目录路径（默认 .）" },
            "max_entries": { "type":"integer", "description":"可选：最多返回多少条目录项（默认 200）", "minimum":1 },
            "include_hidden": { "type":"boolean", "description":"可选：是否包含隐藏文件/目录（以 . 开头），默认 false" }
        },
        "required":[]
    })
}

pub(super) fn params_glob_files() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "glob 模式（相对起始目录），如 **/*.rs、*.toml、src/**/*.ts；禁止 .. 与绝对路径"
            },
            "path": {
                "type": "string",
                "description": "可选：起始子目录（相对工作区，默认 .）"
            },
            "max_depth": {
                "type": "integer",
                "description": "可选：相对起始目录最大路径层数（默认 20，上限 100）；0 表示仅起始目录下的一层，不进入子目录",
                "minimum": 0,
                "maximum": 100
            },
            "max_results": {
                "type": "integer",
                "description": "可选：最多返回多少条匹配路径（默认 200，上限 5000）",
                "minimum": 1,
                "maximum": 5000
            },
            "include_hidden": {
                "type": "boolean",
                "description": "可选：是否进入/匹配以 . 开头的文件与目录，默认 false"
            }
        },
        "required": ["pattern"],
        "additionalProperties": false
    })
}

pub(super) fn params_list_tree() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "可选：起始目录（相对工作区，默认 .）"
            },
            "max_depth": {
                "type": "integer",
                "description": "可选：相对起始目录最大路径层数（默认 8，上限 60）；控制向下递归层数",
                "minimum": 0,
                "maximum": 60
            },
            "max_entries": {
                "type": "integer",
                "description": "可选：最多列出多少条路径（含起点 .，默认 500，上限 10000）",
                "minimum": 1,
                "maximum": 10000
            },
            "include_hidden": {
                "type": "boolean",
                "description": "可选：是否列出以 . 开头的项，默认 false"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(super) fn params_file_exists() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"相对工作目录的路径（文件或目录，必填）" },
            "kind": { "type":"string", "description":"可选：匹配类型 file|dir|any，默认 any", "enum":["file","dir","any"] }
        },
        "required":["path"],
        "additionalProperties":false
    })
}

pub(super) fn params_read_binary_meta() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的文件路径（必填）" },
            "prefix_hash_bytes": {
                "type": "integer",
                "description": "参与 SHA256 的文件头字节数：默认 8192；0 表示不计算哈希；最大 262144",
                "minimum": 0,
                "maximum": 262144
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub(super) fn params_hash_file() -> serde_json::Value {
    let max_prefix: i64 = (4u64 * 1024 * 1024 * 1024).min(i64::MAX as u64) as i64;
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的文件路径（必填）" },
            "algorithm": {
                "type": "string",
                "description": "哈希算法：sha256（默认）、sha512、blake3",
                "enum": ["sha256", "sha-256", "sha512", "sha-512", "blake3"]
            },
            "max_bytes": {
                "type": "integer",
                "description": "可选：仅哈希文件前若干字节（与整文件校验不同）；省略则整文件流式哈希。最小 1，上限 4GiB",
                "minimum": 1,
                "maximum": max_prefix
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub(super) fn params_extract_in_file() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"相对工作目录的文件路径（必填）" },
            "pattern": { "type":"string", "description":"要匹配的正则表达式（必填）" },
            "start_line": { "type":"integer", "description":"可选：起始行（1-based，包含该行）", "minimum":1 },
            "end_line": { "type":"integer", "description":"可选：结束行（1-based，包含该行）", "minimum":1 },
            "max_matches": { "type":"integer", "description":"可选：最多返回匹配条数（默认 50）", "minimum":1 },
            "case_insensitive": { "type":"boolean", "description":"可选：是否忽略大小写（默认 true）" },
            "max_snippet_chars": { "type":"integer", "description":"可选：每条匹配行最多截断字符数（默认 400）", "minimum":1 },
            "mode": { "type":"string", "description":"可选：提取模式（lines 或 rust_fn_block，默认 lines）", "enum":["lines","rust_fn_block"] },
            "max_block_chars": { "type":"integer", "description":"可选：rust_fn_block 模式下每个块最多截断字符数（默认 8000）", "minimum":1 },
            "max_block_lines": { "type":"integer", "description":"可选：rust_fn_block 模式下每个块最多扫描/输出的行数（默认 500）", "minimum":1 }
        },
        "required":["path","pattern"],
        "additionalProperties":false
    })
}

pub(super) fn params_find_symbol() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "symbol": { "type":"string", "description":"要定位的符号名（必填）" },
            "path": { "type":"string", "description":"可选：搜索起点子路径（相对工作区根目录，默认 .）" },
            "kind": { "type":"string", "description":"可选：符号类型（fn|struct|enum|trait|const|static|type|mod|any，默认 any）" },
            "max_results": { "type":"integer", "description":"可选：最多返回结果条数（默认 30）", "minimum":1 },
            "context_lines": { "type":"integer", "description":"可选：每条结果输出的上下文行数（默认 2）", "minimum":0 },
            "case_insensitive": { "type":"boolean", "description":"可选：是否忽略大小写（默认 true）" },
            "include_hidden": { "type":"boolean", "description":"可选：是否包含隐藏文件（以 . 开头），默认 false" }
        },
        "required":["symbol"],
        "additionalProperties":false
    })
}

pub(super) fn params_find_references() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "symbol": { "type":"string", "description":"要查找引用的标识符名（必填）" },
            "path": { "type":"string", "description":"可选：仅在某子路径下搜索（相对工作区）" },
            "max_results": { "type":"integer", "description":"可选：最多返回条数，默认 80，上限 300", "minimum":1 },
            "case_sensitive": { "type":"boolean", "description":"可选：是否大小写敏感（默认 false，即忽略大小写）" },
            "exclude_definitions": { "type":"boolean", "description":"可选：是否跳过疑似定义行（默认 true）" },
            "include_hidden": { "type":"boolean", "description":"可选：是否遍历隐藏目录（默认 false）" }
        },
        "required":["symbol"],
        "additionalProperties":false
    })
}

pub(super) fn params_rust_file_outline() -> serde_json::Value {
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

pub(super) fn params_format_check_file() -> serde_json::Value {
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

pub(super) fn params_quality_workspace() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "run_cargo_fmt_check": { "type":"boolean", "description":"可选：cargo fmt --check，默认 true" },
            "run_cargo_clippy": { "type":"boolean", "description":"可选：cargo clippy --all-targets，默认 true" },
            "run_cargo_test": { "type":"boolean", "description":"可选：cargo test，默认 false（较慢）" },
            "run_frontend_lint": { "type":"boolean", "description":"可选：frontend 下 npm run lint，默认 false" },
            "run_frontend_prettier_check": { "type":"boolean", "description":"可选：frontend 下 npx prettier --check .，默认 false" },
            "run_ruff_check": { "type":"boolean", "description":"可选：ruff check，默认 false（无 Python 项目时跳过）" },
            "run_pytest": { "type":"boolean", "description":"可选：python3 -m pytest，默认 false" },
            "run_mypy": { "type":"boolean", "description":"可选：mypy，默认 false" },
            "fail_fast": { "type":"boolean", "description":"可选：遇首个失败即停止后续步骤，默认 true" },
            "summary_only": { "type":"boolean", "description":"可选：仅输出各步骤 passed/failed 汇总，默认 false" }
        },
        "required":[]
    })
}

pub(super) fn params_apply_patch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "patch": {
                "type": "string",
                "description": "unified diff（同 git diff）：含 ---/+++ 与 @@；变更上下各保留 2～3 行上下文（空格行），禁止零上下文；小步单主题便于回滚。路径：要么 --- src/x.rs（strip 默认 0），要么 --- a/src/x.rs 且 strip=1。可 patch -R 或 git checkout 撤销。"
            },
            "strip": {
                "type": "integer",
                "description": "patch -p：无 a/ 前缀时用 0；--- a/... 的 Git 风格 diff 须用 1",
                "minimum": 0
            }
        },
        "required": ["patch"]
    })
}

pub(super) fn params_search_in_files() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "要搜索的正则或纯文本关键字（使用 Rust 正则语法）"
            },
            "path": {
                "type": "string",
                "description": "可选的子目录或文件相对路径，仅在该路径下搜索（相对于工作区根目录）"
            },
            "max_results": {
                "type": "integer",
                "description": "最多返回多少条匹配结果，默认 200 条",
                "minimum": 1
            },
            "case_insensitive": {
                "type": "boolean",
                "description": "是否大小写不敏感匹配，默认 true"
            },
            "ignore_hidden": {
                "type": "boolean",
                "description": "是否忽略隐藏文件和目录（以点开头），默认 true"
            }
        },
        "required": ["pattern"]
    })
}

pub(super) fn params_markdown_check_links() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "roots": {
                "type": "array",
                "items": { "type": "string" },
                "description": "要扫描的相对路径（文件须为 .md，目录则递归收集 .md）。默认 [\"README.md\",\"docs\"]"
            },
            "max_files": {
                "type": "integer",
                "description": "最多处理多少个 Markdown 文件，默认 300，上限 3000",
                "minimum": 1,
                "maximum": 3000
            },
            "max_depth": {
                "type": "integer",
                "description": "目录递归深度上限，默认 24，上限 80",
                "minimum": 1,
                "maximum": 80
            },
            "allowed_external_prefixes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：仅对这些前缀匹配的 http(s) 或 // 外链发起 HEAD 探测；为空则所有外链仅计数、不联网"
            },
            "external_timeout_secs": {
                "type": "integer",
                "description": "外链探测超时（秒），默认 10，上限 60",
                "minimum": 1,
                "maximum": 60
            },
            "check_fragments": {
                "type": "boolean",
                "description": "是否校验 Markdown 锚点（#fragment），默认 true。"
            },
            "output_format": {
                "type": "string",
                "description": "输出格式：text（默认）/ json / sarif",
                "enum": ["text", "json", "sarif"]
            }
        },
        "required": []
    })
}

pub(super) fn params_structured_validate() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区的 JSON / YAML / TOML / CSV / TSV 文件路径（如 package.json、data.csv）"
            },
            "format": {
                "type": "string",
                "description": "可选：auto（按扩展名推断）或 json / yaml|yml / toml / csv / tsv",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；true 时解析为对象数组，false 时为字符串数组的数组；JSON/YAML/TOML 忽略。默认 true"
            },
            "summarize": {
                "type": "boolean",
                "description": "可选：校验通过后是否输出顶层结构摘要，默认 true"
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_structured_query() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的数据文件路径" },
            "query": {
                "type": "string",
                "description": "路径：以 / 开头为 JSON Pointer（RFC 6901，如 /dependencies/serde）；否则为点号路径（如 dependencies.serde；纯数字段作数组下标）"
            },
            "format": {
                "type": "string",
                "description": "可选：auto / json / yaml|yml / toml / csv / tsv",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；默认 true"
            }
        },
        "required": ["path", "query"]
    })
}

pub(super) fn params_structured_diff() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path_a": { "type": "string", "description": "相对工作区的第一份文件（如 openapi.old.json）" },
            "path_b": { "type": "string", "description": "相对工作区的第二份文件（如 openapi.new.json）" },
            "format": {
                "type": "string",
                "description": "可选：对两边使用同一格式；auto 时按各自扩展名分别推断",
                "enum": ["auto", "json", "yaml", "yml", "toml", "csv", "tsv"]
            },
            "has_header": {
                "type": "boolean",
                "description": "仅 CSV/TSV：首行是否为列名；对 path_a 与 path_b 使用同一语义；默认 true"
            },
            "max_diff_lines": {
                "type": "integer",
                "description": "最多输出多少条差异路径，默认 200，上限 2000",
                "minimum": 1,
                "maximum": 2000
            }
        },
        "required": ["path_a", "path_b"]
    })
}

pub(super) fn params_structured_patch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的数据文件路径（json/yaml/yml/toml）" },
            "query": {
                "type": "string",
                "description": "目标路径：JSON Pointer（/a/b）或点号路径（a.b.0）"
            },
            "action": {
                "type": "string",
                "enum": ["set", "remove"],
                "description": "补丁动作，默认 set"
            },
            "value": {
                "description": "仅 action=set 需要：写入值（任意 JSON 值）",
                "oneOf": [
                    {"type":"object"},
                    {"type":"array"},
                    {"type":"string"},
                    {"type":"number"},
                    {"type":"integer"},
                    {"type":"boolean"},
                    {"type":"null"}
                ]
            },
            "format": {
                "type": "string",
                "description": "可选：auto / json / yaml|yml / toml",
                "enum": ["auto", "json", "yaml", "yml", "toml"]
            },
            "create_missing": {
                "type": "boolean",
                "description": "action=set 时中间路径缺失是否自动创建，默认 true"
            },
            "dry_run": {
                "type": "boolean",
                "description": "默认 true：仅预览；false 将实际写入"
            },
            "confirm": {
                "type": "boolean",
                "description": "当 dry_run=false 时必须 true"
            }
        },
        "required": ["path", "query"],
        "additionalProperties": false
    })
}

pub(super) fn params_text_transform() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "op": {
                "type": "string",
                "description": "base64_encode | base64_decode | url_encode | url_decode | hash_short | lines_join | lines_split",
                "enum": [
                    "base64_encode",
                    "base64_decode",
                    "url_encode",
                    "url_decode",
                    "hash_short",
                    "lines_join",
                    "lines_split"
                ]
            },
            "text": {
                "type": "string",
                "description": "输入文本；单次上限 256KiB。lines_split 时按 delimiter 切分；lines_join 时按行拆开再用 delimiter 连接。"
            },
            "delimiter": {
                "type": "string",
                "description": "lines_join 默认空格；lines_split 必填非空；最大 256 字节"
            },
            "hash_algo": {
                "type": "string",
                "description": "仅 hash_short：sha256（默认）或 blake3；输出 16 位十六进制前缀",
                "enum": ["sha256", "blake3"]
            }
        },
        "required": ["op", "text"]
    })
}

pub(super) fn params_table_text() -> serde_json::Value {
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

pub(super) fn params_text_diff() -> serde_json::Value {
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

pub(super) fn params_format_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区根目录的文件路径，如 src/main.rs、frontend/src/App.tsx、src/pkg/__init__.py、src/foo.cpp（.py 使用 ruff format；.c/.h/.cpp 等使用 clang-format）"
            }
        },
        "required": ["path"]
    })
}

pub(super) fn params_run_lints() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "run_cargo": {
                "type": "boolean",
                "description": "是否运行 cargo clippy，默认为 true"
            },
            "run_frontend": {
                "type": "boolean",
                "description": "是否在 frontend 目录下运行 npm run lint（若存在），默认为 true"
            },
            "run_python_ruff": {
                "type": "boolean",
                "description": "是否运行 ruff check（有 Python 项目标记时），默认为 true"
            }
        },
        "required": []
    })
}

pub(super) fn params_add_reminder() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "提醒内容" },
            "due_at": { "type": "string", "description": "可选：到期时间（支持 RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD）" }
        },
        "required": ["title"]
    })
}

pub(super) fn params_list_reminders() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "include_done": { "type": "boolean", "description": "是否包含已完成提醒，默认 false" },
            "future_days": { "type": "integer", "description": "可选：仅显示未来 N 天内到期的提醒（只筛选有 due_at 的提醒）", "minimum": 0 }
        },
        "required": []
    })
}

pub(super) fn params_update_reminder() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "提醒 id" },
            "title": { "type": "string", "description": "可选：更新标题" },
            "due_at": { "type": "string", "description": "可选：更新到期时间（空字符串表示清空）" },
            "done": { "type": "boolean", "description": "可选：更新完成状态" }
        },
        "required": ["id"]
    })
}

pub(super) fn params_id_only() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "id": { "type": "string", "description": "条目 id" } },
        "required": ["id"]
    })
}

pub(super) fn params_add_event() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "日程标题" },
            "start_at": { "type": "string", "description": "开始时间（RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD）" },
            "end_at": { "type": "string", "description": "可选：结束时间（同 start_at 格式）" },
            "location": { "type": "string", "description": "可选：地点" },
            "notes": { "type": "string", "description": "可选：备注" }
        },
        "required": ["title", "start_at"]
    })
}

pub(super) fn params_list_events() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "year": { "type": "integer", "description": "可选：按年份过滤（如 2026）" },
            "month": { "type": "integer", "description": "可选：按月份过滤（1-12，通常与 year 一起用）", "minimum": 1, "maximum": 12 },
            "future_days": { "type": "integer", "description": "可选：仅显示未来 N 天内开始的日程（按 start_at 过滤）", "minimum": 0 }
        },
        "required": []
    })
}

pub(super) fn params_update_event() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "日程 id" },
            "title": { "type": "string", "description": "可选：更新标题" },
            "start_at": { "type": "string", "description": "可选：更新开始时间" },
            "end_at": { "type": "string", "description": "可选：更新结束时间（空字符串表示清空）" },
            "location": { "type": "string", "description": "可选：更新地点（空字符串表示清空）" },
            "notes": { "type": "string", "description": "可选：更新备注（空字符串表示清空）" }
        },
        "required": ["id"]
    })
}

// ── Git 写操作补全 ──────────────────────────────────────────

pub(super) fn params_git_checkout() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "target":{"type":"string","description":"分支名、标签名或 commit SHA（必填）"},
            "create":{"type":"boolean","description":"是否以 -b 创建新分支，默认 false"}
        },
        "required":["target"]
    })
}

pub(super) fn params_git_branch_create() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"新分支名（必填）"},
            "start_point":{"type":"string","description":"可选：起始点（分支/tag/SHA），默认 HEAD"}
        },
        "required":["name"]
    })
}

pub(super) fn params_git_branch_delete() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"要删除的分支名（必填）"},
            "force":{"type":"boolean","description":"是否强制删除（-D），默认 false（-d，需已合并）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["name"]
    })
}

pub(super) fn params_git_push() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "remote":{"type":"string","description":"远程名，默认 origin"},
            "branch":{"type":"string","description":"可选：推送的分支/refspec"},
            "set_upstream":{"type":"boolean","description":"是否 -u 设置上游，默认 false"},
            "force_with_lease":{"type":"boolean","description":"是否 --force-with-lease，默认 false"},
            "tags":{"type":"boolean","description":"是否 --tags 推送标签，默认 false"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(super) fn params_git_merge() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "branch":{"type":"string","description":"要合并的分支名（必填）"},
            "no_ff":{"type":"boolean","description":"是否 --no-ff 强制合并提交，默认 false"},
            "squash":{"type":"boolean","description":"是否 --squash 压缩合并，默认 false"},
            "message":{"type":"string","description":"可选：合并消息"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["branch"]
    })
}

pub(super) fn params_git_rebase() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "onto":{"type":"string","description":"变基目标（分支/SHA）"},
            "abort":{"type":"boolean","description":"是否 --abort 取消变基"},
            "continue":{"type":"boolean","description":"是否 --continue 继续变基"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(super) fn params_git_stash() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "action":{"type":"string","description":"操作：push（默认）/pop/apply/list/drop/clear","enum":["push","pop","apply","list","drop","clear"]},
            "message":{"type":"string","description":"可选：push 时的描述消息"},
            "confirm":{"type":"boolean","description":"仅 clear 需要 confirm=true"}
        },
        "required":[]
    })
}

pub(super) fn params_git_tag() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "action":{"type":"string","description":"操作：list（默认）/create/delete","enum":["list","create","delete"]},
            "name":{"type":"string","description":"create/delete 时的标签名"},
            "message":{"type":"string","description":"create 时的注释消息（传入即创建 annotated tag）"},
            "pattern":{"type":"string","description":"list 时的 glob 过滤（如 v*）"},
            "confirm":{"type":"boolean","description":"仅 delete 需要 confirm=true"}
        },
        "required":[]
    })
}

pub(super) fn params_git_reset() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{"type":"string","description":"重置模式：soft/mixed（默认）/hard","enum":["soft","mixed","hard"]},
            "target":{"type":"string","description":"目标 commit/ref，默认 HEAD"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(super) fn params_git_cherry_pick() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "commit":{"type":"string","description":"要挑选的单个 commit SHA"},
            "commits":{"type":"array","items":{"type":"string"},"description":"要挑选的多个 commit SHA"},
            "no_commit":{"type":"boolean","description":"是否 --no-commit 仅暂存不提交，默认 false"},
            "abort":{"type":"boolean","description":"是否 --abort"},
            "continue":{"type":"boolean","description":"是否 --continue"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(super) fn params_git_revert() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "commit":{"type":"string","description":"要回滚的 commit SHA（必填，除 abort/continue）"},
            "no_commit":{"type":"boolean","description":"是否 --no-commit 仅暂存不提交，默认 false"},
            "abort":{"type":"boolean","description":"是否 --abort"},
            "continue":{"type":"boolean","description":"是否 --continue"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

// ── Node.js / npm / npx ─────────────────────────────────────

pub(super) fn params_npm_install() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "ci":{"type":"boolean","description":"使用 npm ci（默认 false）"},
            "production":{"type":"boolean","description":"仅安装生产依赖，默认 false"}
        },
        "required":[]
    })
}

pub(super) fn params_npm_run() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "script":{"type":"string","description":"npm script 名（必填）"},
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "args":{"type":"array","items":{"type":"string"},"description":"传递给 script 的额外参数（-- 之后）"}
        },
        "required":["script"]
    })
}

pub(super) fn params_npx_run() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "package":{"type":"string","description":"npx 要执行的包名（必填），如 prettier、eslint"},
            "subdir":{"type":"string","description":"工作子目录（默认 .）"},
            "args":{"type":"array","items":{"type":"string"},"description":"传递给包命令的参数"}
        },
        "required":["package"]
    })
}

pub(super) fn params_tsc_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "subdir":{"type":"string","description":"前端子目录（默认 .），如 frontend"},
            "project":{"type":"string","description":"可选：tsconfig 路径（-p），默认使用 -b"},
            "strict":{"type":"boolean","description":"是否 --strict，默认 false"}
        },
        "required":[]
    })
}

// ── Go 补充：golangci-lint ──────────────────────────────────

pub(super) fn params_golangci_lint() -> serde_json::Value {
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

pub(super) fn params_port_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "port":{"type":"integer","description":"要检查的端口号（1-65535，必填）","minimum":1,"maximum":65535}
        },
        "required":["port"]
    })
}

pub(super) fn params_process_list() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "filter":{"type":"string","description":"可选：按进程名/命令行关键词过滤（不区分大小写）"},
            "user_only":{"type":"boolean","description":"是否仅当前用户进程，默认 true"},
            "max_count":{"type":"integer","description":"最多返回条数，默认 100","minimum":1,"maximum":500}
        },
        "required":[]
    })
}
