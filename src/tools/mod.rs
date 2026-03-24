//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod cargo_tools;
mod ci_tools;
mod code_nav;
mod command;
mod debug_tools;
mod diagnostics;
mod exec;
mod file;
mod format;
mod frontend_tools;
mod git;
mod grep;
pub mod http_fetch;
mod lint;
mod markdown_links;
mod package_query;
mod patch;
mod precommit_tools;
mod python_tools;
mod quality_tools;
mod release_docs;
mod rust_ide;
mod schedule;
mod security_tools;
mod spell_astgrep_tools;
mod structured_data;
mod symbol;
mod table_text;
mod text_diff;
mod text_transform;
mod time;
mod tool_summary;
mod unit_convert;
mod weather;
mod web_search;

pub mod dev_tag;

use crate::config::AgentConfig;
use crate::tool_result::ToolResult;
use crate::types::{FunctionDef, Tool};

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：时间/计算/天气、联网搜索与受控 HTTP、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

pub struct ToolContext<'a> {
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    pub allowed_commands: &'a [String],
    pub working_dir: &'a std::path::Path,
    pub web_search_timeout_secs: u64,
    pub web_search_provider: crate::config::WebSearchProvider,
    pub web_search_api_key: &'a str,
    pub web_search_max_results: u32,
    pub http_fetch_allowed_prefixes: &'a [String],
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
pub fn tool_context_for<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
) -> ToolContext<'a> {
    ToolContext {
        command_max_output_len: cfg.command_max_output_len,
        weather_timeout_secs: cfg.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search_timeout_secs,
        web_search_provider: cfg.web_search_provider,
        web_search_api_key: cfg.web_search_api_key.as_str(),
        web_search_max_results: cfg.web_search_max_results,
        http_fetch_allowed_prefixes: cfg.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch_max_response_bytes,
    }
}

type ToolRunner = fn(args_json: &str, ctx: &ToolContext<'_>) -> String;
type ParamBuilder = fn() -> serde_json::Value;

struct ToolSpec {
    name: &'static str,
    description: &'static str,
    category: ToolCategory,
    parameters: ParamBuilder,
    runner: ToolRunner,
}

fn params_get_current_time() -> serde_json::Value {
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

fn params_calc() -> serde_json::Value {
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

fn params_convert_units() -> serde_json::Value {
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

fn params_weather() -> serde_json::Value {
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

fn params_web_search() -> serde_json::Value {
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

fn params_http_fetch() -> serde_json::Value {
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

fn params_http_request() -> serde_json::Value {
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

fn params_run_command() -> serde_json::Value {
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

fn params_run_executable() -> serde_json::Value {
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

fn params_package_query() -> serde_json::Value {
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

fn params_cargo_common() -> serde_json::Value {
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

fn params_cargo_test() -> serde_json::Value {
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

fn params_cargo_run() -> serde_json::Value {
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

fn params_rust_test_one() -> serde_json::Value {
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

fn params_cargo_metadata() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "no_deps": { "type": "boolean", "description": "可选：是否添加 --no-deps，默认 true" },
            "format_version": { "type": "integer", "description": "可选：metadata 格式版本，默认 1", "minimum": 1 }
        },
        "required": []
    })
}

fn params_cargo_tree() -> serde_json::Value {
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

fn params_cargo_clean() -> serde_json::Value {
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

fn params_cargo_doc() -> serde_json::Value {
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

fn params_cargo_nextest() -> serde_json::Value {
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

fn params_cargo_fmt_check() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}

fn params_cargo_outdated() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "workspace": { "type":"boolean", "description":"可选：是否传 --workspace（检查整个 workspace）" },
            "depth": { "type":"integer", "description":"可选：依赖树深度（--depth）", "minimum":0 }
        },
        "required":[]
    })
}

fn params_cargo_machete() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "with_metadata": { "type":"boolean", "description":"可选：传 --with-metadata（调用 cargo metadata，更准但更慢，可能改动 Cargo.lock）" },
            "path": { "type":"string", "description":"可选：相对工作区的子目录，传给 cargo machete <path>；不可含 .." }
        },
        "required":[]
    })
}

fn params_cargo_udeps() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "nightly": { "type":"boolean", "description":"可选：为 true 时执行 cargo +nightly udeps（cargo-udeps 通常需要 nightly）" }
        },
        "required":[]
    })
}

fn params_cargo_publish_dry_run() -> serde_json::Value {
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

fn params_rust_compiler_json() -> serde_json::Value {
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

fn params_rust_analyzer_position() -> serde_json::Value {
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

fn params_rust_analyzer_references() -> serde_json::Value {
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

fn params_cargo_fix() -> serde_json::Value {
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

fn params_frontend_lint() -> serde_json::Value {
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
fn params_ruff_check() -> serde_json::Value {
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
fn params_pytest_run() -> serde_json::Value {
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
fn params_mypy_check() -> serde_json::Value {
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

fn params_python_install_editable() -> serde_json::Value {
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

fn params_uv_sync() -> serde_json::Value {
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

fn params_uv_run() -> serde_json::Value {
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

fn params_pre_commit_run() -> serde_json::Value {
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

fn params_typos_check() -> serde_json::Value {
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

fn params_codespell_check() -> serde_json::Value {
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

fn params_ast_grep_run() -> serde_json::Value {
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

fn params_ast_grep_rewrite() -> serde_json::Value {
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

fn params_cargo_audit() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "deny_warnings": { "type": "boolean", "description": "可选：是否添加 --deny warnings" },
            "json": { "type": "boolean", "description": "可选：是否使用 --json 输出" }
        },
        "required": []
    })
}

fn params_cargo_deny() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "checks": { "type": "string", "description": "可选：检查项，默认 \"advisories licenses bans sources\"" },
            "all_features": { "type": "boolean", "description": "可选：是否启用 --all-features" }
        },
        "required": []
    })
}

fn params_backtrace_analyze() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "backtrace": { "type": "string", "description": "panic/backtrace 原文（必填）" },
            "crate_hint": { "type": "string", "description": "可选：业务 crate 名提示，用于过滤调用栈" }
        },
        "required": ["backtrace"]
    })
}

fn params_ci_pipeline_local() -> serde_json::Value {
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

fn params_release_ready_check() -> serde_json::Value {
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

fn params_workflow_execute() -> serde_json::Value {
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

fn params_git_status() -> serde_json::Value {
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

fn params_git_clean_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

fn params_git_diff() -> serde_json::Value {
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

fn params_git_diff_stat() -> serde_json::Value {
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

fn params_git_diff_names() -> serde_json::Value {
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

fn params_git_log() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 20","minimum":1},
            "oneline":{"type":"boolean","description":"可选：是否使用单行展示，默认 true"}
        },
        "required":[]
    })
}

fn params_changelog_draft() -> serde_json::Value {
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

fn params_license_notice() -> serde_json::Value {
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

fn params_git_show() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "rev":{"type":"string","description":"可选：提交号/引用，默认 HEAD"}
        },
        "required":[]
    })
}

fn params_git_diff_base() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "base":{"type":"string","description":"可选：基准分支，默认 main（对比 base...HEAD）"},
            "context_lines":{"type":"integer","description":"可选：上下文行数，默认 3","minimum":0}
        },
        "required":[]
    })
}

fn params_git_blame() -> serde_json::Value {
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

fn params_git_file_history() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path":{"type":"string","description":"相对路径（必填）"},
            "max_count":{"type":"integer","description":"可选：最多返回提交条数，默认 30","minimum":1}
        },
        "required":["path"]
    })
}

fn params_git_branch_list() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "include_remote":{"type":"boolean","description":"可选：是否包含远程分支，默认 true"}
        },
        "required":[]
    })
}

fn params_git_stage_files() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "paths":{"type":"array","items":{"type":"string"},"description":"要暂存的相对路径列表（必填）"}
        },
        "required":["paths"]
    })
}

fn params_git_commit() -> serde_json::Value {
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

fn params_git_fetch() -> serde_json::Value {
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

fn params_git_remote_set_url() -> serde_json::Value {
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

fn params_git_apply() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "patch_path":{"type":"string","description":"补丁文件相对路径（必填）"},
            "check_only":{"type":"boolean","description":"是否仅检查可应用性，默认 true"}
        },
        "required":["patch_path"]
    })
}

fn params_git_clone() -> serde_json::Value {
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

fn params_empty_object() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{},
        "required":[]
    })
}

fn params_diagnostic_summary() -> serde_json::Value {
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

fn params_file_write() -> serde_json::Value {
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

fn params_modify_file() -> serde_json::Value {
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

fn params_file_from_to_overwrite() -> serde_json::Value {
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

fn params_read_file() -> serde_json::Value {
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

fn params_read_dir() -> serde_json::Value {
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

fn params_glob_files() -> serde_json::Value {
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

fn params_list_tree() -> serde_json::Value {
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

fn params_file_exists() -> serde_json::Value {
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

fn params_read_binary_meta() -> serde_json::Value {
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

fn params_hash_file() -> serde_json::Value {
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

fn params_extract_in_file() -> serde_json::Value {
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

fn params_find_symbol() -> serde_json::Value {
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

fn params_find_references() -> serde_json::Value {
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

fn params_rust_file_outline() -> serde_json::Value {
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

fn params_format_check_file() -> serde_json::Value {
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

fn params_quality_workspace() -> serde_json::Value {
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

fn params_apply_patch() -> serde_json::Value {
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

fn params_search_in_files() -> serde_json::Value {
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

fn params_markdown_check_links() -> serde_json::Value {
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

fn params_structured_validate() -> serde_json::Value {
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

fn params_structured_query() -> serde_json::Value {
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

fn params_structured_diff() -> serde_json::Value {
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

fn params_structured_patch() -> serde_json::Value {
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

fn params_text_transform() -> serde_json::Value {
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

fn params_table_text() -> serde_json::Value {
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

fn params_text_diff() -> serde_json::Value {
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

fn params_format_file() -> serde_json::Value {
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

fn params_run_lints() -> serde_json::Value {
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

fn params_add_reminder() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "提醒内容" },
            "due_at": { "type": "string", "description": "可选：到期时间（支持 RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD）" }
        },
        "required": ["title"]
    })
}

fn params_list_reminders() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "include_done": { "type": "boolean", "description": "是否包含已完成提醒，默认 false" },
            "future_days": { "type": "integer", "description": "可选：仅显示未来 N 天内到期的提醒（只筛选有 due_at 的提醒）", "minimum": 0 }
        },
        "required": []
    })
}

fn params_update_reminder() -> serde_json::Value {
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

fn params_id_only() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "id": { "type": "string", "description": "条目 id" } },
        "required": ["id"]
    })
}

fn params_add_event() -> serde_json::Value {
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

fn params_list_events() -> serde_json::Value {
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

fn params_update_event() -> serde_json::Value {
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

fn runner_get_current_time(args: &str, _ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Object(Default::default()),
    };
    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .and_then(time::TimeOutputMode::from_str)
        .unwrap_or(time::TimeOutputMode::Time);
    let year = v.get("year").and_then(|y| y.as_i64()).map(|y| y as i32);
    let month = v
        .get("month")
        .and_then(|m| m.as_u64())
        .and_then(|m| u32::try_from(m).ok());
    time::run(mode, year, month)
}

fn runner_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    let expr = match serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| {
            v.get("expression")
                .and_then(|e| e.as_str())
                .map(String::from)
        }) {
        Some(s) => s,
        None => return "错误：缺少 expression 参数".to_string(),
    };
    calc::run(&expr)
}

fn runner_convert_units(args: &str, _ctx: &ToolContext<'_>) -> String {
    unit_convert::run(args)
}

fn runner_get_weather(args: &str, ctx: &ToolContext<'_>) -> String {
    weather::run(args, ctx.weather_timeout_secs)
}

fn runner_web_search(args: &str, ctx: &ToolContext<'_>) -> String {
    web_search::run(args, ctx)
}

fn runner_http_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_direct(args, ctx)
}

fn runner_http_request(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_request_direct(args, ctx)
}

fn runner_run_command(args: &str, ctx: &ToolContext<'_>) -> String {
    command::run(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_run_executable(args: &str, ctx: &ToolContext<'_>) -> String {
    exec::run(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_package_query(args: &str, ctx: &ToolContext<'_>) -> String {
    package_query::run(args, ctx.command_max_output_len)
}

fn runner_cargo_check(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_test(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_clippy(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clippy(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_metadata(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_metadata(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_tree(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_clean(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clean(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_doc(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_doc(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_nextest(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_nextest(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_fmt_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::cargo_fmt_check_tool(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_outdated(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_outdated(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_machete(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_machete(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_udeps(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_udeps(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_publish_dry_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_publish_dry_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_compiler_json(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_compiler_json(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_analyzer_goto_definition(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_goto_definition(args, ctx.working_dir)
}

fn runner_rust_analyzer_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_find_references(args, ctx.working_dir)
}

fn runner_cargo_fix(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_fix(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_test_one(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::rust_test_one(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ruff_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::ruff_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_pytest_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::pytest_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_mypy_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::mypy_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_python_install_editable(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::python_install_editable(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_uv_sync(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_sync(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_uv_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_pre_commit_run(args: &str, ctx: &ToolContext<'_>) -> String {
    precommit_tools::pre_commit_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_typos_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::typos_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_codespell_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::codespell_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ast_grep_run(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ast_grep_rewrite(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_rewrite(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_lint(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_lint(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_build(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_build(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_test(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_audit(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_audit(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_deny(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_deny(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_backtrace_analyze(args: &str, _ctx: &ToolContext<'_>) -> String {
    debug_tools::rust_backtrace_analyze(args)
}

fn runner_diagnostic_summary(args: &str, ctx: &ToolContext<'_>) -> String {
    diagnostics::diagnostic_summary(args, ctx.working_dir)
}

fn runner_changelog_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::changelog_draft(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_license_notice(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::license_notice(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ci_pipeline_local(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::ci_pipeline_local(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_release_ready_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::release_ready_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_workflow_execute(_args: &str, _ctx: &ToolContext<'_>) -> String {
    // 由 runtime 在 run_agent_turn / run_agent_turn_tui 中拦截实际执行。
    "workflow_execute：由运行时引擎执行（若你看到这条，说明拦截未生效）。".to_string()
}

fn runner_git_status(args: &str, ctx: &ToolContext<'_>) -> String {
    git::status(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_clean_check(args: &str, ctx: &ToolContext<'_>) -> String {
    git::clean_check(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff_stat(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_stat(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff_names(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_names(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_log(args: &str, ctx: &ToolContext<'_>) -> String {
    git::log(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_show(args: &str, ctx: &ToolContext<'_>) -> String {
    git::show(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_diff_base(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_base(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_blame(args: &str, ctx: &ToolContext<'_>) -> String {
    git::blame(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_file_history(args: &str, ctx: &ToolContext<'_>) -> String {
    git::file_history(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_branch_list(args: &str, ctx: &ToolContext<'_>) -> String {
    git::branch_list(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_status(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_status(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_stage_files(args: &str, ctx: &ToolContext<'_>) -> String {
    git::stage_files(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_commit(args: &str, ctx: &ToolContext<'_>) -> String {
    git::commit(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    git::fetch(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_list(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_list(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_set_url(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_set_url(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_apply(args: &str, ctx: &ToolContext<'_>) -> String {
    git::apply(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_clone(args: &str, ctx: &ToolContext<'_>) -> String {
    git::clone_repo(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_create_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_file(args, ctx.working_dir)
}

fn runner_modify_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::modify_file(args, ctx.working_dir)
}

fn runner_copy_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::copy_file(args, ctx.working_dir)
}

fn runner_move_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::move_file(args, ctx.working_dir)
}

fn runner_read_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_file(args, ctx.working_dir)
}

fn runner_read_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_dir(args, ctx.working_dir)
}

fn runner_glob_files(args: &str, ctx: &ToolContext<'_>) -> String {
    file::glob_files(args, ctx.working_dir)
}

fn runner_list_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    file::list_tree(args, ctx.working_dir)
}

fn runner_file_exists(args: &str, ctx: &ToolContext<'_>) -> String {
    file::file_exists(args, ctx.working_dir)
}

fn runner_read_binary_meta(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_binary_meta(args, ctx.working_dir)
}

fn runner_hash_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::hash_file(args, ctx.working_dir)
}

fn runner_extract_in_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::extract_in_file(args, ctx.working_dir)
}

fn runner_apply_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    patch::run(args, ctx.working_dir)
}

fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
}

fn runner_markdown_check_links(args: &str, ctx: &ToolContext<'_>) -> String {
    markdown_links::markdown_check_links(args, ctx.working_dir)
}

fn runner_structured_validate(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_validate(args, ctx.working_dir)
}

fn runner_structured_query(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_query(args, ctx.working_dir)
}

fn runner_structured_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_diff(args, ctx.working_dir)
}

fn runner_structured_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_patch(args, ctx.working_dir)
}

fn runner_text_transform(args: &str, _ctx: &ToolContext<'_>) -> String {
    text_transform::run(args)
}

fn runner_text_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    text_diff::run(args, ctx.working_dir)
}

fn runner_table_text(args: &str, ctx: &ToolContext<'_>) -> String {
    table_text::run(args, ctx.working_dir)
}

fn runner_find_symbol(args: &str, ctx: &ToolContext<'_>) -> String {
    symbol::run(args, ctx.working_dir)
}

fn runner_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::find_references(args, ctx.working_dir)
}

fn runner_rust_file_outline(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::rust_file_outline(args, ctx.working_dir)
}

fn runner_format_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run(args, ctx.working_dir)
}

fn runner_format_check_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run_check(args, ctx.working_dir)
}

fn runner_run_lints(args: &str, ctx: &ToolContext<'_>) -> String {
    lint::run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_quality_workspace(args: &str, ctx: &ToolContext<'_>) -> String {
    quality_tools::quality_workspace(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_add_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_reminder(args, ctx.working_dir)
}

fn runner_list_reminders(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_reminders(args, ctx.working_dir)
}

fn runner_complete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::complete_reminder(args, ctx.working_dir)
}

fn runner_delete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_reminder(args, ctx.working_dir)
}

fn runner_update_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_reminder(args, ctx.working_dir)
}

fn runner_add_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_event(args, ctx.working_dir)
}

fn runner_list_events(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_events(args, ctx.working_dir)
}

fn runner_delete_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_event(args, ctx.working_dir)
}

fn runner_update_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_event(args, ctx.working_dir)
}

fn tool_specs() -> &'static [ToolSpec] {
    &[
        ToolSpec {
            name: "get_current_time",
            description: "获取当前时间或打印日历。支持 mode=time|calendar|both（默认 time）。用于回答「现在几点」「今天几号」「打印本月日历」「打印 2026 年 3 月日历」等问题。",
            category: ToolCategory::Basic,
            parameters: params_get_current_time,
            runner: runner_get_current_time,
        },
        ToolSpec {
            name: "calc",
            description: "使用 Linux 的 bc -l 计算器执行数学表达式。支持：四则 + - * / %、乘方 ^；sqrt(x)、s(x)=sin、c(x)=cos、a(x)=atan、l(x)=ln、e(x)=exp；常量 pi、e（在 bc 中为 pi=4*a(1), e=e(1)）。可写 math::sqrt(2)、math::sin(pi/2)、math::log10(100) 等，会转为 bc 语法后执行。示例：1+2*3、2^10、sqrt(2)、s(3.14159/2)。参数 expression 为单个数学表达式。",
            category: ToolCategory::Basic,
            parameters: params_calc,
            runner: runner_calc,
        },
        ToolSpec {
            name: "convert_units",
            description: "物理量与数据量单位换算（Rust uom 库，不调用外部程序）。category：length|mass|temperature|data|time|area|pressure|speed（或中文如 温度、数据量）。参数 value、from、to：如 {\"category\":\"length\",\"value\":5,\"from\":\"km\",\"to\":\"mile\"}；数据量区分十进制 KB/MB/GB 与二进制 KiB/MiB/GiB。",
            category: ToolCategory::Basic,
            parameters: params_convert_units,
            runner: runner_convert_units,
        },
        ToolSpec {
            name: "text_transform",
            description: "纯内存字符串变换（不落盘）：Base64 编解码、URL 百分号编解码、短哈希（sha256/blake3 各取 16 位十六进制前缀）、lines_join（按行拆开后以 delimiter 连接，默认空格）、lines_split（按 delimiter 切分，段数上限 50000）。输入 text 单次上限 256KiB，输出上限 512KiB。",
            category: ToolCategory::Basic,
            parameters: params_text_transform,
            runner: runner_text_transform,
        },
        ToolSpec {
            name: "get_weather",
            description: "获取指定城市或地区的当前天气（使用 Open-Meteo，无需 API Key）。用于回答「某地天气怎么样」「北京今天天气」等问题。参数 city 或 location 为城市/地区名，如北京、上海、Tokyo、New York。",
            category: ToolCategory::Basic,
            parameters: params_weather,
            runner: runner_get_weather,
        },
        ToolSpec {
            name: "web_search",
            description: "联网搜索网页：根据关键词返回若干条结果的标题、URL 与摘要。需在配置中设置 web_search_api_key，并选择 web_search_provider 为 brave（Brave Search API）或 tavily（Tavily）。适合查新闻、文档、事实类问题；代码仓库内查找请优先用 search_in_files。",
            category: ToolCategory::Basic,
            parameters: params_web_search,
            runner: runner_web_search,
        },
        ToolSpec {
            name: "http_fetch",
            description: "对 **http/https** URL 发起 **GET**（默认）或 **HEAD**。GET 返回状态、Content-Type、**重定向链**与正文（按配置截断）；HEAD 不下载 body，仅元数据与重定向链，省流量。**Web**：仅当 URL 匹配 `http_fetch_allowed_prefixes` 的同源 + 路径前缀边界时执行。**TUI**：未匹配前缀时与 `run_command` 相同审批；GET/HEAD 共用白名单键 `http_fetch:<归一化URL>`。勿在 URL 中放真实密钥。`workflow_execute` 节点内仅白名单 URL 可成功。",
            category: ToolCategory::Basic,
            parameters: params_http_fetch,
            runner: runner_http_fetch,
        },
        ToolSpec {
            name: "http_request",
            description: "对 **http/https** URL 发起 **POST/PUT/PATCH/DELETE**（可选 JSON body）。仅允许匹配 `http_fetch_allowed_prefixes` 的同源 + 路径前缀边界；响应含状态、Content-Type、重定向链与正文（按配置截断）。默认建议 dry-run 先验证，勿在 body 中放真实密钥。",
            category: ToolCategory::Basic,
            parameters: params_http_request,
            runner: runner_http_request,
        },
        ToolSpec {
            name: "run_command",
            description: "在服务器上执行**白名单内**的 Linux 系统命令（见配置 allowed_commands：如 ls、gcc、cmake、make、file 等）。用于列目录、读文件、**编译/链接**（gcc/clang/make/ninja/cmake）、Autotools、c++filt 等。**不要**用本工具去「运行当前工作区里已生成的可执行文件」（./main、./a.out、./build/…）；那种情况必须用 **run_executable**。参数 args 为字符串数组；禁止含 \"..\" 或以 \"/\" 开头的实参。不要执行 rm、mv、chmod 等未在白名单中的命令。",
            category: ToolCategory::Development,
            parameters: params_run_command,
            runner: runner_run_command,
        },
        ToolSpec {
            name: "run_executable",
            description: "在工作区内按**相对路径**执行可执行文件（path 如 ./main、./a.out、./build/app、target/release/foo）。**编译或构建完成后要运行产物时，必须用本工具**，不要用 run_command 拼 shell、也不要把本地程序名当成白名单命令。args 为传给该程序的参数（可选）；路径不得为绝对路径，不得含 .. 逃出工作区。",
            category: ToolCategory::Development,
            parameters: params_run_executable,
            runner: runner_run_executable,
        },
        ToolSpec {
            name: "package_query",
            description: "只读查询 Linux 包信息（apt/rpm 统一抽象）：是否安装、版本、来源。默认 manager=auto（优先 dpkg-query，再尝试 rpm）；不执行安装/卸载操作。",
            category: ToolCategory::Development,
            parameters: params_package_query,
            runner: runner_package_query,
        },
        ToolSpec {
            name: "cargo_check",
            description: "运行 cargo check（结构化参数）。用于快速检查 Rust 项目编译问题。",
            category: ToolCategory::Development,
            parameters: params_cargo_common,
            runner: runner_cargo_check,
        },
        ToolSpec {
            name: "cargo_test",
            description: "运行 cargo test（支持 package/bin/filter/nocapture）。用于执行 Rust 测试。",
            category: ToolCategory::Development,
            parameters: params_cargo_test,
            runner: runner_cargo_test,
        },
        ToolSpec {
            name: "cargo_clippy",
            description: "运行 cargo clippy（结构化参数）。用于检查 Rust 代码潜在问题。",
            category: ToolCategory::Development,
            parameters: params_cargo_common,
            runner: runner_cargo_clippy,
        },
        ToolSpec {
            name: "cargo_metadata",
            description: "运行 cargo metadata 并返回包/依赖元数据（JSON）。用于理解 workspace 与 crate 关系。",
            category: ToolCategory::Development,
            parameters: params_cargo_metadata,
            runner: runner_cargo_metadata,
        },
        ToolSpec {
            name: "cargo_tree",
            description: "运行 cargo tree 查看依赖树。支持反向依赖、深度和边类型过滤。",
            category: ToolCategory::Development,
            parameters: params_cargo_tree,
            runner: runner_cargo_tree,
        },
        ToolSpec {
            name: "cargo_clean",
            description: "运行 cargo clean 清理构建产物。默认 dry_run=true，仅预览。",
            category: ToolCategory::Development,
            parameters: params_cargo_clean,
            runner: runner_cargo_clean,
        },
        ToolSpec {
            name: "cargo_doc",
            description: "运行 cargo doc 生成文档。可选 no_deps/open/package。",
            category: ToolCategory::Development,
            parameters: params_cargo_doc,
            runner: runner_cargo_doc,
        },
        ToolSpec {
            name: "cargo_run",
            description: "运行 cargo run（结构化参数）。用于启动 Rust 可执行程序。",
            category: ToolCategory::Development,
            parameters: params_cargo_run,
            runner: runner_cargo_run,
        },
        ToolSpec {
            name: "cargo_nextest",
            description: "运行 cargo nextest run（需要已安装 cargo-nextest）。用于更快的测试执行。",
            category: ToolCategory::Development,
            parameters: params_cargo_nextest,
            runner: runner_cargo_nextest,
        },
        ToolSpec {
            name: "cargo_fmt_check",
            description: "运行 cargo fmt --check（代码格式检查）。",
            category: ToolCategory::Development,
            parameters: params_cargo_fmt_check,
            runner: runner_cargo_fmt_check,
        },
        ToolSpec {
            name: "cargo_outdated",
            description: "运行 cargo outdated（检查依赖是否过期/可升级）。",
            category: ToolCategory::Development,
            parameters: params_cargo_outdated,
            runner: runner_cargo_outdated,
        },
        ToolSpec {
            name: "cargo_machete",
            description: "运行 cargo machete（需 cargo-machete）：快速扫描 **声明但未在源码中引用** 的依赖；与 cargo_outdated（版本可升级）互补。可选 with_metadata、path。",
            category: ToolCategory::Development,
            parameters: params_cargo_machete,
            runner: runner_cargo_machete,
        },
        ToolSpec {
            name: "cargo_udeps",
            description: "运行 cargo udeps（需 cargo-udeps，通常需 nightly：传 nightly=true 使用 cargo +nightly udeps）：基于构建的未使用依赖检查，与 machete/outdated 互补。",
            category: ToolCategory::Development,
            parameters: params_cargo_udeps,
            runner: runner_cargo_udeps,
        },
        ToolSpec {
            name: "cargo_publish_dry_run",
            description: "运行 cargo publish --dry-run：验证打包与发布检查，**不会**上传到 registry。可选 package、allow_dirty、no_verify、features。",
            category: ToolCategory::Development,
            parameters: params_cargo_publish_dry_run,
            runner: runner_cargo_publish_dry_run,
        },
        ToolSpec {
            name: "rust_compiler_json",
            description: "运行 `cargo check --message-format=<json>`，解析 **compiler-message** 行，输出结构化诊断摘要（级别、错误码、rendered、span）。等价于对接 rustc 的 JSON 诊断流，无需 rust-analyzer。",
            category: ToolCategory::Development,
            parameters: params_rust_compiler_json,
            runner: runner_rust_compiler_json,
        },
        ToolSpec {
            name: "rust_analyzer_goto_definition",
            description: "启动 **rust-analyzer**（stdio LSP），对指定 **path + 0-based line/character** 执行 `textDocument/definition`。需本机已安装 rust-analyzer；单文件 didOpen，适合跳转定义。大文件 >512KiB 请换 read_file 分段。",
            category: ToolCategory::Development,
            parameters: params_rust_analyzer_position,
            runner: runner_rust_analyzer_goto_definition,
        },
        ToolSpec {
            name: "rust_analyzer_find_references",
            description: "同上，执行 `textDocument/references`（语义引用）。参数含 include_declaration。",
            category: ToolCategory::Development,
            parameters: params_rust_analyzer_references,
            runner: runner_rust_analyzer_find_references,
        },
        ToolSpec {
            name: "cargo_fix",
            description: "执行 cargo fix 应用编译器/诊断建议（受控写入，需 confirm=true）。",
            category: ToolCategory::Development,
            parameters: params_cargo_fix,
            runner: runner_cargo_fix,
        },
        ToolSpec {
            name: "rust_test_one",
            description: "运行单个 Rust 测试（按 test_name 过滤）。用于快速调试具体测试。",
            category: ToolCategory::Development,
            parameters: params_rust_test_one,
            runner: runner_rust_test_one,
        },
        ToolSpec {
            name: "ruff_check",
            description: "在工作区运行 `ruff check`（需已安装 ruff）。默认检查 `.`；可通过 paths 指定相对路径。无 pyproject.toml/setup/requirements 等标记时跳过。",
            category: ToolCategory::Development,
            parameters: params_ruff_check,
            runner: runner_ruff_check,
        },
        ToolSpec {
            name: "pytest_run",
            description: "在工作区运行 `python3 -m pytest`（需已安装 pytest）。可选 test_path、keyword（-k）、markers（-m）、quiet、maxfail、nocapture。",
            category: ToolCategory::Development,
            parameters: params_pytest_run,
            runner: runner_pytest_run,
        },
        ToolSpec {
            name: "mypy_check",
            description: "在工作区运行 `mypy`（需已安装 mypy）。默认检查 `.`；可选 paths 与 strict。无 Python 项目标记时跳过。",
            category: ToolCategory::Development,
            parameters: params_mypy_check,
            runner: runner_mypy_check,
        },
        ToolSpec {
            name: "python_install_editable",
            description: "在工作区根执行可编辑安装：`backend=uv` 时 `uv pip install -e .`，`backend=pip` 时 `python3 -m pip install -e .`。须存在 pyproject.toml 或 setup.py。",
            category: ToolCategory::Development,
            parameters: params_python_install_editable,
            runner: runner_python_install_editable,
        },
        ToolSpec {
            name: "uv_sync",
            description: "在工作区根运行 `uv sync`（须存在 pyproject.toml）。可选 frozen（--frozen）、no_dev（--no-dev）、all_packages（--all-packages）。需本机已安装 uv。",
            category: ToolCategory::Development,
            parameters: params_uv_sync,
            runner: runner_uv_sync,
        },
        ToolSpec {
            name: "uv_run",
            description: "在工作区根运行 `uv run`：`args` 为非空字符串数组（如 [\"pytest\",\"-q\"]），逐项作为子进程参数、不经 shell。须存在 pyproject.toml。",
            category: ToolCategory::Development,
            parameters: params_uv_run,
            runner: runner_uv_run,
        },
        ToolSpec {
            name: "pre_commit_run",
            description: "在工作区根运行 `pre-commit run`。须存在 .pre-commit-config.yaml（或 .yml）。可选 hook、all_files、files（--files 相对路径）、verbose。默认检查暂存文件。",
            category: ToolCategory::Development,
            parameters: params_pre_commit_run,
            runner: runner_pre_commit_run,
        },
        ToolSpec {
            name: "typos_check",
            description: "运行 [typos](https://github.com/crate-ci/typos) 拼写检查（**只读**）。默认检查存在的 `README.md` 与 `docs/`；可用 `paths` 指定更多相对路径。需本机已安装 `typos` CLI。适合文档与注释中的常见错别字。",
            category: ToolCategory::Development,
            parameters: params_typos_check,
            runner: runner_typos_check,
        },
        ToolSpec {
            name: "codespell_check",
            description: "运行 [codespell](https://github.com/codespell-project/codespell)（**只读**，不传入 `-w`）。默认路径策略同 `typos_check`；可选 `skip` 传给 `--skip`。需本机已安装 `codespell`。",
            category: ToolCategory::Development,
            parameters: params_codespell_check,
            runner: runner_codespell_check,
        },
        ToolSpec {
            name: "ast_grep_run",
            description: "运行 [ast-grep](https://ast-grep.github.io/) `run` 做**结构化**代码搜索（非纯文本 grep）。必填 `pattern` 与 `lang`；默认仅在存在的 `src` 下搜索，并附加 `--globs` 排除 target、node_modules、.git、vendor、dist、build。可用 `paths` 收窄/改写根路径，`globs` 追加排除规则。需本机已安装 `ast-grep` 命令（`cargo install ast-grep`）。",
            category: ToolCategory::Development,
            parameters: params_ast_grep_run,
            runner: runner_ast_grep_run,
        },
        ToolSpec {
            name: "ast_grep_rewrite",
            description: "运行 `ast-grep run --rewrite` 做结构化改写。默认 `dry_run=true` 仅预览；当 `dry_run=false` 时需 `confirm=true` 才会写盘（等价 `--update-all`）。路径与 globs 安全策略同 `ast_grep_run`。",
            category: ToolCategory::Development,
            parameters: params_ast_grep_rewrite,
            runner: runner_ast_grep_rewrite,
        },
        ToolSpec {
            name: "frontend_lint",
            description: "运行前端 npm lint（结构化参数）。支持指定前端子目录和 script 名称。",
            category: ToolCategory::Development,
            parameters: params_frontend_lint,
            runner: runner_frontend_lint,
        },
        ToolSpec {
            name: "frontend_build",
            description: "运行前端 npm build（结构化参数）。支持指定前端子目录和 script 名称（默认 build）。",
            category: ToolCategory::Development,
            parameters: params_frontend_lint,
            runner: runner_frontend_build,
        },
        ToolSpec {
            name: "frontend_test",
            description: "运行前端 npm test（结构化参数）。支持指定前端子目录和 script 名称（默认 test）。",
            category: ToolCategory::Development,
            parameters: params_frontend_lint,
            runner: runner_frontend_test,
        },
        ToolSpec {
            name: "cargo_audit",
            description: "运行 cargo audit 做依赖漏洞扫描（需要已安装 cargo-audit）。",
            category: ToolCategory::Development,
            parameters: params_cargo_audit,
            runner: runner_cargo_audit,
        },
        ToolSpec {
            name: "cargo_deny",
            description: "运行 cargo deny check（需要已安装 cargo-deny），做许可证/安全策略检查。",
            category: ToolCategory::Development,
            parameters: params_cargo_deny,
            runner: runner_cargo_deny,
        },
        ToolSpec {
            name: "ci_pipeline_local",
            description: "本地一键执行 CI 关键检查（cargo fmt/clippy/test、frontend lint、可选 ruff/pytest/mypy）。",
            category: ToolCategory::Development,
            parameters: params_ci_pipeline_local,
            runner: runner_ci_pipeline_local,
        },
        ToolSpec {
            name: "release_ready_check",
            description: "发布前一键检查：CI + audit + deny + 工作区干净检查。",
            category: ToolCategory::Development,
            parameters: params_release_ready_check,
            runner: runner_release_ready_check,
        },
        ToolSpec {
            name: "workflow_execute",
            description: "执行 DAG 工作流：并行/串行调度 + 人工审批节点 + SLA 超时 + 失败补偿。",
            category: ToolCategory::Development,
            parameters: params_workflow_execute,
            runner: runner_workflow_execute,
        },
        ToolSpec {
            name: "rust_backtrace_analyze",
            description: "分析 Rust panic/backtrace 文本，提取首个可疑业务帧和模块命中统计。",
            category: ToolCategory::Development,
            parameters: params_backtrace_analyze,
            runner: runner_backtrace_analyze,
        },
        ToolSpec {
            name: "diagnostic_summary",
            description: "只读排障摘要：**Rust 工具链**（rustc/cargo -V、rustc -vV 的 host/release、rustup default、bc 是否可用）、**工作区**（根路径、`target/` 是否存在、`Cargo.toml` / `frontend/package.json` / `frontend/dist` 是否存在）、**环境变量仅状态**（`API_KEY`、常见 `AGENT_*`、`RUST_LOG` 等：未设置/空/非空；**永不输出变量值**；密钥类亦不输出长度）。可选 `extra_env_vars`（大写安全名）。与 AGENTS.md 排障场景一致。",
            category: ToolCategory::Development,
            parameters: params_diagnostic_summary,
            runner: runner_diagnostic_summary,
        },
        ToolSpec {
            name: "changelog_draft",
            description: "根据 **git log** 生成 **Markdown 变更说明草稿**（**不写仓库**）。支持按提交日聚合 subject、`flat` 平铺、或 `tag_ranges` 按 semver 降序相邻 tag 分段（`--no-merges`）。可选 since/until 与 max_commits。",
            category: ToolCategory::Development,
            parameters: params_changelog_draft,
            runner: runner_changelog_draft,
        },
        ToolSpec {
            name: "license_notice",
            description: "运行 **cargo metadata** 解析依赖图，生成 **crate → license** 的 Markdown 表（**只读**；未在 Cargo.toml 声明的显示占位说明）。可选仅工作区成员、限制行数。非法律意见，发版前需人工核对。",
            category: ToolCategory::Development,
            parameters: params_license_notice,
            runner: runner_license_notice,
        },
        ToolSpec {
            name: "git_status",
            description: "读取当前工作区的 Git 状态（只读）。可查看分支、已暂存/未暂存变更和未跟踪文件，帮助在改动前后自检变更范围，避免覆盖未提交内容。",
            category: ToolCategory::Development,
            parameters: params_git_status,
            runner: runner_git_status,
        },
        ToolSpec {
            name: "git_diff",
            description: "读取当前工作区的 Git diff（只读）。支持查看 working、staged 或 all 模式，并可按 path 过滤，便于精确确认具体改动。",
            category: ToolCategory::Development,
            parameters: params_git_diff,
            runner: runner_git_diff,
        },
        ToolSpec {
            name: "git_clean_check",
            description: "检查当前工作区是否干净（git status --porcelain）。",
            category: ToolCategory::Development,
            parameters: params_git_clean_check,
            runner: runner_git_clean_check,
        },
        ToolSpec {
            name: "git_diff_stat",
            description: "读取当前工作区的 Git diff 统计（只读）。支持 working/staged/all 与可选 path 过滤。",
            category: ToolCategory::Development,
            parameters: params_git_diff_stat,
            runner: runner_git_diff_stat,
        },
        ToolSpec {
            name: "git_diff_names",
            description: "读取当前工作区的 Git diff 变更文件名列表（只读）。支持 working/staged/all 与可选 path 过滤。",
            category: ToolCategory::Development,
            parameters: params_git_diff_names,
            runner: runner_git_diff_names,
        },
        ToolSpec {
            name: "git_log",
            description: "读取 Git 提交历史（只读）。支持条数和单行模式。",
            category: ToolCategory::Development,
            parameters: params_git_log,
            runner: runner_git_log,
        },
        ToolSpec {
            name: "git_show",
            description: "读取指定提交详情（只读），默认 HEAD。",
            category: ToolCategory::Development,
            parameters: params_git_show,
            runner: runner_git_show,
        },
        ToolSpec {
            name: "git_diff_base",
            description: "读取 base...HEAD 范围 diff（只读），默认 main...HEAD。",
            category: ToolCategory::Development,
            parameters: params_git_diff_base,
            runner: runner_git_diff_base,
        },
        ToolSpec {
            name: "git_blame",
            description: "查看文件行级 blame（只读）。可选行范围。",
            category: ToolCategory::Development,
            parameters: params_git_blame,
            runner: runner_git_blame,
        },
        ToolSpec {
            name: "git_file_history",
            description: "查看单文件历史（只读，--follow）。",
            category: ToolCategory::Development,
            parameters: params_git_file_history,
            runner: runner_git_file_history,
        },
        ToolSpec {
            name: "git_branch_list",
            description: "查看分支列表（只读），可含远程分支。",
            category: ToolCategory::Development,
            parameters: params_git_branch_list,
            runner: runner_git_branch_list,
        },
        ToolSpec {
            name: "git_remote_status",
            description: "查看本地分支与远程跟踪关系（只读，git status -sb）。",
            category: ToolCategory::Development,
            parameters: params_git_status,
            runner: runner_git_remote_status,
        },
        ToolSpec {
            name: "git_stage_files",
            description: "将指定相对路径加入暂存区（受控写入）。",
            category: ToolCategory::Development,
            parameters: params_git_stage_files,
            runner: runner_git_stage_files,
        },
        ToolSpec {
            name: "git_commit",
            description: "执行 git commit（受控写入，需 confirm=true）。可选先 stage_all。",
            category: ToolCategory::Development,
            parameters: params_git_commit,
            runner: runner_git_commit,
        },
        ToolSpec {
            name: "git_fetch",
            description: "执行 git fetch（可选 remote/branch/prune）。",
            category: ToolCategory::Development,
            parameters: params_git_fetch,
            runner: runner_git_fetch,
        },
        ToolSpec {
            name: "git_remote_list",
            description: "查看远程仓库列表（git remote -v）。",
            category: ToolCategory::Development,
            parameters: params_empty_object,
            runner: runner_git_remote_list,
        },
        ToolSpec {
            name: "git_remote_set_url",
            description: "设置远程仓库 URL（受控写入，需 confirm=true）。",
            category: ToolCategory::Development,
            parameters: params_git_remote_set_url,
            runner: runner_git_remote_set_url,
        },
        ToolSpec {
            name: "git_apply",
            description: "执行 git apply。默认 check_only=true 先检查可应用性。",
            category: ToolCategory::Development,
            parameters: params_git_apply,
            runner: runner_git_apply,
        },
        ToolSpec {
            name: "git_clone",
            description: "执行 git clone 到工作区内目标目录（受控写入，需 confirm=true）。",
            category: ToolCategory::Development,
            parameters: params_git_clone,
            runner: runner_git_clone,
        },
        ToolSpec {
            name: "create_file",
            description: "在工作区内创建新文件。仅当文件不存在时创建；若路径已存在则报错。路径相对于工作目录，不能包含 .. 超出工作目录。用于用户要求「新建文件」「创建 xx」等。参数 path 为相对路径，content 为文件内容。",
            category: ToolCategory::Development,
            parameters: params_file_write,
            runner: runner_create_file,
        },
        ToolSpec {
            name: "modify_file",
            description: "在工作区内修改已有文件。mode=full（默认）：整文件覆盖。mode=replace_lines：仅替换 start_line..=end_line 为 content，流式读写不写全文件进内存，适合大文件局部修改。",
            category: ToolCategory::Development,
            parameters: params_modify_file,
            runner: runner_modify_file,
        },
        ToolSpec {
            name: "copy_file",
            description: "在工作区内复制**文件**（非目录）。路径校验与 create/read 一致（禁止绝对路径与 `..` 越界、借助 symlink 逃逸）。目标为已存在文件时须 `overwrite=true` 才覆盖；目标为已存在目录会报错。适合批量整理而无需把内容读进对话。",
            category: ToolCategory::Development,
            parameters: params_file_from_to_overwrite,
            runner: runner_copy_file,
        },
        ToolSpec {
            name: "move_file",
            description: "在工作区内移动/重命名**文件**。`overwrite` 语义同 `copy_file`。跨文件系统时 `rename` 失败会自动回退为复制后删除源文件。",
            category: ToolCategory::Development,
            parameters: params_file_from_to_overwrite,
            runner: runner_move_file,
        },
        ToolSpec {
            name: "read_file",
            description: "按行流式读取文件（不把整文件载入内存）。默认单次最多返回 max_lines=500 行（可调到 8000）；未指定 end_line 时自动分段。输出提示下一段 start_line。可选 count_total_lines 统计总行数（大文件慎用）。",
            category: ToolCategory::Development,
            parameters: params_read_file,
            runner: runner_read_file,
        },
        ToolSpec {
            name: "read_dir",
            description: "在工作区内读取目录下的文件/子目录列表（受控只读）。可选包含隐藏项与最大条数。",
            category: ToolCategory::Development,
            parameters: params_read_dir,
            runner: runner_read_dir,
        },
        ToolSpec {
            name: "glob_files",
            description: "在工作区内从指定子目录起递归扫描，按 glob 模式匹配**文件**相对路径（如 **/*.rs）。带 max_depth、max_results 上限；路径均在工作区内解析，禁止 ..。优先于 run_command find。",
            category: ToolCategory::Development,
            parameters: params_glob_files,
            runner: runner_glob_files,
        },
        ToolSpec {
            name: "list_tree",
            description: "在工作区内从指定目录起递归列出子路径（先序、字典序），前缀 dir:/file:；带 max_depth、max_entries。用于快速看目录树而不用 find。",
            category: ToolCategory::Development,
            parameters: params_list_tree,
            runner: runner_list_tree,
        },
        ToolSpec {
            name: "file_exists",
            description: "检查工作区内某路径（文件或目录）是否存在，并可按 kind=file|dir|any 过滤。",
            category: ToolCategory::Development,
            parameters: params_file_exists,
            runner: runner_file_exists,
        },
        ToolSpec {
            name: "read_binary_meta",
            description: "读取任意文件的元数据（大小、可选修改时间）及**文件头一段的 SHA256**，不把整文件读入上下文；适合二进制/大文件比对。prefix_hash_bytes 默认 8192，0 表示跳过哈希。",
            category: ToolCategory::Development,
            parameters: params_read_binary_meta,
            runner: runner_read_binary_meta,
        },
        ToolSpec {
            name: "hash_file",
            description: "对工作区内**常规文件**做只读哈希（流式读取，不占满内存）：**sha256**（默认）、**sha512**、**blake3**。可选 `max_bytes` 仅哈希前缀（用于大文件抽样或对齐外部工具）；路径解析与 `read_file` 相同。",
            category: ToolCategory::Development,
            parameters: params_hash_file,
            runner: runner_hash_file,
        },
        ToolSpec {
            name: "extract_in_file",
            description: "在指定文件内按正则抽取匹配行（只读）。返回带行号的匹配行，并支持截断。",
            category: ToolCategory::Development,
            parameters: params_extract_in_file,
            runner: runner_extract_in_file,
        },
        ToolSpec {
            name: "apply_patch",
            description: "应用 **unified diff**。路径：--- src/…（strip=0）或 --- a/src/…（strip=1）；hunk **带 2～3 行上下文**；**小步**单主题；可 **patch -R** / **git checkout** 回滚。先 dry-run。",
            category: ToolCategory::Development,
            parameters: params_apply_patch,
            runner: runner_apply_patch,
        },
        ToolSpec {
            name: "search_in_files",
            description: "在当前工作区内搜索文件内容。支持按正则或普通关键词搜索，返回匹配的文件路径、行号和包含匹配的行片段。适合回答「某个函数/类型/常量在哪定义」「有哪些地方包含 TODO」等问题。",
            category: ToolCategory::Development,
            parameters: params_search_in_files,
            runner: runner_search_in_files,
        },
        ToolSpec {
            name: "markdown_check_links",
            description: "扫描工作区内 Markdown（默认 README.md 与 docs/）：校验**相对路径**链接目标是否存在，并可校验 `#fragment` 是否命中目标 Markdown 标题锚点。支持 `output_format=text|json|sarif`。`http(s)://` 与 `//` 外链默认**不联网**；仅当提供 `allowed_external_prefixes` 时对匹配前缀做 HEAD 探测（失败时 GET Range 回退，且同 URL 去重缓存）。`mailto:`/`tel:` 等跳过。",
            category: ToolCategory::Development,
            parameters: params_markdown_check_links,
            runner: runner_markdown_check_links,
        },
        ToolSpec {
            name: "structured_validate",
            description: "解析并校验工作区内的 **JSON / YAML / TOML / CSV / TSV**（按扩展名或 `format`）。JSON 系用于 `package.json`、CI、`Cargo.toml` 等；**CSV/TSV** 会解析为 JSON 数组（`has_header=true` 时为对象数组，列名来自首行；`false` 时为字符串数组的数组），再输出顶层摘要。单文件上限 4MiB。与 `table_text`（按行预览/筛选/聚合、不落 JSON 模型）互补。",
            category: ToolCategory::Development,
            parameters: params_structured_validate,
            runner: runner_structured_validate,
        },
        ToolSpec {
            name: "structured_query",
            description: "在解析后的 JSON 模型上按 **JSON Pointer**（`/a/b`）或 **点号路径**（`a.b.0`）取值，返回类型与格式化 JSON。**CSV/TSV** 先整表解析为数组（有表头时 `/0/col` 为第 0 行某列）。比整文件 `read_file` 更省上下文。",
            category: ToolCategory::Development,
            parameters: params_structured_query,
            runner: runner_structured_query,
        },
        ToolSpec {
            name: "structured_diff",
            description: "将两份 **JSON / YAML / TOML / CSV / TSV** 解析为同一结构化模型后做**键路径级**差异（缺失键、数组项、标量不等），非文本行 diff；与 `git_diff` 互补（如两份导出表、两份 `openapi.json`）。CSV/TSV 使用相同 `has_header` 语义解析两边。",
            category: ToolCategory::Development,
            parameters: params_structured_diff,
            runner: runner_structured_diff,
        },
        ToolSpec {
            name: "structured_patch",
            description: "对 **JSON / YAML / TOML** 做结构化补丁（`set/remove`），路径支持 JSON Pointer（`/a/b`）或点号（`a.b.0`）。默认 `dry_run=true` 仅预览；写盘需 `confirm=true`。适合精确修改配置而非整段文本替换。",
            category: ToolCategory::Development,
            parameters: params_structured_patch,
            runner: runner_structured_patch,
        },
        ToolSpec {
            name: "text_diff",
            description: "任意两段 **UTF-8 纯文本**的行级 unified diff（与 Git 无关）。mode=inline 时比较 left/right 字符串（各 256KiB）；mode=paths 时比较工作区内两文件（各 4MiB 内）。可调 context_lines 与 max_output_bytes。与 `structured_diff`（结构化键）互补。",
            category: ToolCategory::Development,
            parameters: params_text_diff,
            runner: runner_text_diff,
        },
        ToolSpec {
            name: "table_text",
            description: "工作区内 **CSV / TSV / 简单分隔** 纯文本：`preview` 抽样预览，`validate` 检查列数是否一致，`select_columns` 按列下标导出 TSV，`filter_rows` 按列 equals/contains 筛选，`aggregate` 对列做 sum/mean/min/max/count。单文件 4MiB；内联 `text` 256KiB；扫描/输出行数有上限。",
            category: ToolCategory::Development,
            parameters: params_table_text,
            runner: runner_table_text,
        },
        ToolSpec {
            name: "find_symbol",
            description: "在当前工作区递归定位 Rust 符号的潜在定义位置（如 fn/struct/enum/trait/const/static/type/mod）。返回匹配行与上下文。",
            category: ToolCategory::Development,
            parameters: params_find_symbol,
            runner: runner_find_symbol,
        },
        ToolSpec {
            name: "find_references",
            description: "在 .rs 源文件中按词边界搜索某标识符的引用位置；默认排除与 find_symbol 一致的「疑似定义」行。适合在改名、删函数前快速扫一遍使用处。",
            category: ToolCategory::Development,
            parameters: params_find_references,
            runner: runner_find_references,
        },
        ToolSpec {
            name: "rust_file_outline",
            description: "读取单个 Rust 源文件，列出其中常见的顶层结构行摘要（mod/fn/struct/enum/trait/impl/use 等），便于大文件导航与拆分任务。",
            category: ToolCategory::Development,
            parameters: params_rust_file_outline,
            runner: runner_rust_file_outline,
        },
        ToolSpec {
            name: "format_file",
            description: "对工作区内的文件进行代码格式化。根据文件扩展名自动选择合适的本地格式化器，例如 Rust 使用 rustfmt，C/C++ 使用 clang-format，前端 TypeScript/JavaScript 使用项目内的 Prettier，Python 使用 ruff format。适合在修改代码后统一整理缩进和风格。注意：需要本地已安装相应格式化工具。",
            category: ToolCategory::Development,
            parameters: params_format_file,
            runner: runner_format_file,
        },
        ToolSpec {
            name: "format_check_file",
            description: "对单个文件做格式检查（不修改磁盘）：Rust 使用 rustfmt --check，C/C++ 使用 clang-format --dry-run --Werror，前端类文件使用 prettier --check，Python 使用 ruff format --check。适合在提交前确认风格一致。",
            category: ToolCategory::Development,
            parameters: params_format_check_file,
            runner: runner_format_check_file,
        },
        ToolSpec {
            name: "run_lints",
            description: "运行项目的静态检查工具并聚合结果。目前包括：后端的 cargo clippy 和（若存在 frontend 目录与 package.json）前端的 npm run lint。可用于在改动后检查潜在问题。",
            category: ToolCategory::Development,
            parameters: params_run_lints,
            runner: runner_run_lints,
        },
        ToolSpec {
            name: "quality_workspace",
            description: "按开关组合运行质量检查：默认 cargo fmt --check + cargo clippy（轻量）；可选 cargo test、frontend npm lint、frontend prettier --check。适合「改完一轮后」快速拉齐格式与静态分析。",
            category: ToolCategory::Development,
            parameters: params_quality_workspace,
            runner: runner_quality_workspace,
        },
        ToolSpec {
            name: "add_reminder",
            description: "添加一个提醒事项，并持久化到工作区的 .crabmate/reminders.json。可选 due_at 支持 RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD。",
            category: ToolCategory::Basic,
            parameters: params_add_reminder,
            runner: runner_add_reminder,
        },
        ToolSpec {
            name: "list_reminders",
            description: "列出提醒事项（默认不包含已完成）。数据来自工作区的 .crabmate/reminders.json。",
            category: ToolCategory::Basic,
            parameters: params_list_reminders,
            runner: runner_list_reminders,
        },
        ToolSpec {
            name: "complete_reminder",
            description: "将指定 id 的提醒标记为完成。",
            category: ToolCategory::Basic,
            parameters: params_id_only,
            runner: runner_complete_reminder,
        },
        ToolSpec {
            name: "delete_reminder",
            description: "删除指定 id 的提醒。",
            category: ToolCategory::Basic,
            parameters: params_id_only,
            runner: runner_delete_reminder,
        },
        ToolSpec {
            name: "update_reminder",
            description: "更新提醒（title/due_at/done 任意字段）。due_at 传空字符串表示清空到期时间。",
            category: ToolCategory::Basic,
            parameters: params_update_reminder,
            runner: runner_update_reminder,
        },
        ToolSpec {
            name: "add_event",
            description: "添加一个日程事件，并持久化到工作区的 .crabmate/events.json。start_at 必填，end_at/location/notes 可选。",
            category: ToolCategory::Basic,
            parameters: params_add_event,
            runner: runner_add_event,
        },
        ToolSpec {
            name: "list_events",
            description: "列出日程事件；可选按 year/month 过滤。数据来自工作区的 .crabmate/events.json。",
            category: ToolCategory::Basic,
            parameters: params_list_events,
            runner: runner_list_events,
        },
        ToolSpec {
            name: "delete_event",
            description: "删除指定 id 的日程事件。",
            category: ToolCategory::Basic,
            parameters: params_id_only,
            runner: runner_delete_event,
        },
        ToolSpec {
            name: "update_event",
            description: "更新日程事件（title/start_at/end_at/location/notes 任意字段）。end_at/location/notes 传空字符串表示清空。",
            category: ToolCategory::Basic,
            parameters: params_update_event,
            runner: runner_update_event,
        },
    ]
}

/// 构建工具列表时的分类与开发子域标签过滤。
#[derive(Clone, Copy, Default)]
pub struct ToolsBuildOptions<'a> {
    /// `None` 或 `Some(&[])`：不按顶层分类过滤（`Basic` 与 `Development` 均保留）。
    pub categories: Option<&'a [ToolCategory]>,
    /// `None` 或 `Some(&[])`：不按标签过滤。`Some(non-empty)`：仅保留 **Development** 工具中
    /// [`dev_tag::tags_for_tool_name`] 与列表 **有交集** 者；`Basic` 仍只受 `categories` 约束。
    pub dev_tags: Option<&'a [&'a str]>,
}

fn tool_passes_filters(spec: &ToolSpec, opts: ToolsBuildOptions<'_>) -> bool {
    let cats = opts.categories.unwrap_or(&[]);
    if !cats.is_empty() && !cats.contains(&spec.category) {
        return false;
    }
    let Some(wanted) = opts.dev_tags.and_then(|t| (!t.is_empty()).then_some(t)) else {
        return true;
    };
    if spec.category != ToolCategory::Development {
        return true;
    }
    dev_tag::tags_for_tool_name(spec.name)
        .iter()
        .any(|tag| wanted.contains(tag))
}

/// 构建传给 API 的工具列表（表驱动注册）。
pub fn build_tools() -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions::default())
}

/// 构建传给 API 的工具列表：可按顶层分类过滤（[`ToolCategory::Basic`] / [`ToolCategory::Development`]）。
pub fn build_tools_filtered(allowed: Option<&[ToolCategory]>) -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions {
        categories: allowed,
        dev_tags: None,
    })
}

/// 同时支持顶层分类与 Development 子域标签过滤（见 [`ToolsBuildOptions`]）。
pub fn build_tools_with_options(opts: ToolsBuildOptions<'_>) -> Vec<Tool> {
    tool_specs()
        .iter()
        .filter(|s| tool_passes_filters(s, opts))
        .map(|s| Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: (s.parameters)(),
            },
        })
        .collect()
}

/// 执行本地工具并返回结果字符串。
/// `ToolContext` 聚合 `run_command`、`get_weather`、`web_search` 等工具所需的配置项。
pub fn run_tool(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    match tool_specs().iter().find(|s| s.name == name) {
        Some(spec) => (spec.runner)(args_json, ctx),
        None => format!("未知工具：{}", name),
    }
}

/// 执行本地工具并返回结构化结果（兼容既有字符串输出）。
pub fn run_tool_result(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> ToolResult {
    let output = run_tool(name, args_json, ctx);
    ToolResult::from_legacy_output(name, output)
}

/// 判断本次 run_command 是否为“成功的编译命令”（常见 C/C++ 构建工具且退出码为 0）
pub(crate) fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let cmd = v
        .get("command")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_lowercase());
    let is_compile_cmd = cmd.as_deref().is_some_and(|c| {
        matches!(
            c,
            "gcc" | "g++" | "clang" | "clang++" | "make" | "cmake" | "ninja"
        )
    });
    if !is_compile_cmd {
        return false;
    }
    // run_command 输出的第一行形如：退出码：0
    let first_line = result.lines().next().unwrap_or("");
    if let Some(rest) = first_line.strip_prefix("退出码：")
        && let Ok(code) = rest.trim().parse::<i32>()
    {
        return code == 0;
    }
    false
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub(crate) fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    tool_summary::summarize_tool_call(name, args_json)
}

#[cfg(test)]
#[path = "mod/tests.rs"]
mod tests;
