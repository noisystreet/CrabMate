//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod cargo_tools;
mod ci_tools;
mod code_nav;
mod command;
mod debug_tools;
mod exec;
mod file;
mod frontend_tools;
mod git;
mod symbol;
mod patch;
mod lint;
mod format;
mod quality_tools;
mod rust_ide;
mod grep;
mod schedule;
mod security_tools;
mod time;
mod weather;
mod web_search;

use crate::config::AgentConfig;
use crate::tool_result::ToolResult;
use crate::types::{FunctionDef, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Utility,
    File,
    Search,
    Format,
    Lint,
    Command,
    Schedule,
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

fn params_run_command() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "命令名，必须是允许列表中的一个：ls, pwd, whoami, date, echo, id, uname, env, df, du, head, tail, wc, cat, cmake, gcc, g++, make"
            },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "命令参数（可选），如 [\"-l\"], [\"-n\", \"5\", \"file.txt\"]"
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
                "description": "相对工作目录的可执行文件路径，如 ./main、./build/app"
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
                "description": "相对工作区根目录的文件路径；支持 .rs 与 ts/tsx/js/jsx/json（prettier --check）"
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

fn params_format_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作区根目录的文件路径，如 src/main.rs、frontend/src/App.tsx"
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
        .and_then(|v| v.get("expression").and_then(|e| e.as_str()).map(String::from))
    {
        Some(s) => s,
        None => return "错误：缺少 expression 参数".to_string(),
    };
    calc::run(&expr)
}

fn runner_get_weather(args: &str, ctx: &ToolContext<'_>) -> String {
    weather::run(args, ctx.weather_timeout_secs)
}

fn runner_web_search(args: &str, ctx: &ToolContext<'_>) -> String {
    web_search::run(args, ctx)
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

fn runner_extract_in_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::extract_in_file(args, ctx.working_dir)
}

fn runner_apply_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    patch::run(args, ctx.working_dir)
}

fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
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
            category: ToolCategory::Utility,
            parameters: params_get_current_time,
            runner: runner_get_current_time,
        },
        ToolSpec {
            name: "calc",
            description: "使用 Linux 的 bc -l 计算器执行数学表达式。支持：四则 + - * / %、乘方 ^；sqrt(x)、s(x)=sin、c(x)=cos、a(x)=atan、l(x)=ln、e(x)=exp；常量 pi、e（在 bc 中为 pi=4*a(1), e=e(1)）。可写 math::sqrt(2)、math::sin(pi/2)、math::log10(100) 等，会转为 bc 语法后执行。示例：1+2*3、2^10、sqrt(2)、s(3.14159/2)。参数 expression 为单个数学表达式。",
            category: ToolCategory::Utility,
            parameters: params_calc,
            runner: runner_calc,
        },
        ToolSpec {
            name: "get_weather",
            description: "获取指定城市或地区的当前天气（使用 Open-Meteo，无需 API Key）。用于回答「某地天气怎么样」「北京今天天气」等问题。参数 city 或 location 为城市/地区名，如北京、上海、Tokyo、New York。",
            category: ToolCategory::Utility,
            parameters: params_weather,
            runner: runner_get_weather,
        },
        ToolSpec {
            name: "web_search",
            description: "联网搜索网页：根据关键词返回若干条结果的标题、URL 与摘要。需在配置中设置 web_search_api_key，并选择 web_search_provider 为 brave（Brave Search API）或 tavily（Tavily）。适合查新闻、文档、事实类问题；代码仓库内查找请优先用 search_in_files。",
            category: ToolCategory::Search,
            parameters: params_web_search,
            runner: runner_web_search,
        },
        ToolSpec {
            name: "run_command",
            description: "在服务器上执行有限的 Linux 命令。允许的命令：ls, pwd, whoami, date, echo, id, uname, env, df, du, head, tail, wc, cat, cmake, gcc, g++, make。用于列出目录、查看文件、编译 C/C++ 项目（gcc/g++/make）、CMake 配置与构建等。参数 args 为字符串数组，如 [\"-l\"], [\"-n\", \"5\", \"file.txt\"], [\"..\"], [\"--build\", \".\"]。不要执行 rm、mv、chmod 等未在白名单中的命令。",
            category: ToolCategory::Command,
            parameters: params_run_command,
            runner: runner_run_command,
        },
        ToolSpec {
            name: "run_executable",
            description: "在工作目录下执行可执行程序。path 为相对于工作目录的可执行文件路径（如 ./main、./build/app、out/run_tests），不能使用绝对路径或 .. 超出工作目录。用于运行编译产物、脚本等。参数 path 为相对路径，args 为传给程序的参数数组（可选）。",
            category: ToolCategory::Command,
            parameters: params_run_executable,
            runner: runner_run_executable,
        },
        ToolSpec {
            name: "cargo_check",
            description: "运行 cargo check（结构化参数）。用于快速检查 Rust 项目编译问题。",
            category: ToolCategory::Command,
            parameters: params_cargo_common,
            runner: runner_cargo_check,
        },
        ToolSpec {
            name: "cargo_test",
            description: "运行 cargo test（支持 package/bin/filter/nocapture）。用于执行 Rust 测试。",
            category: ToolCategory::Command,
            parameters: params_cargo_test,
            runner: runner_cargo_test,
        },
        ToolSpec {
            name: "cargo_clippy",
            description: "运行 cargo clippy（结构化参数）。用于检查 Rust 代码潜在问题。",
            category: ToolCategory::Command,
            parameters: params_cargo_common,
            runner: runner_cargo_clippy,
        },
        ToolSpec {
            name: "cargo_metadata",
            description: "运行 cargo metadata 并返回包/依赖元数据（JSON）。用于理解 workspace 与 crate 关系。",
            category: ToolCategory::Command,
            parameters: params_cargo_metadata,
            runner: runner_cargo_metadata,
        },
        ToolSpec {
            name: "cargo_tree",
            description: "运行 cargo tree 查看依赖树。支持反向依赖、深度和边类型过滤。",
            category: ToolCategory::Command,
            parameters: params_cargo_tree,
            runner: runner_cargo_tree,
        },
        ToolSpec {
            name: "cargo_clean",
            description: "运行 cargo clean 清理构建产物。默认 dry_run=true，仅预览。",
            category: ToolCategory::Command,
            parameters: params_cargo_clean,
            runner: runner_cargo_clean,
        },
        ToolSpec {
            name: "cargo_doc",
            description: "运行 cargo doc 生成文档。可选 no_deps/open/package。",
            category: ToolCategory::Command,
            parameters: params_cargo_doc,
            runner: runner_cargo_doc,
        },
        ToolSpec {
            name: "cargo_run",
            description: "运行 cargo run（结构化参数）。用于启动 Rust 可执行程序。",
            category: ToolCategory::Command,
            parameters: params_cargo_run,
            runner: runner_cargo_run,
        },
        ToolSpec {
            name: "cargo_nextest",
            description: "运行 cargo nextest run（需要已安装 cargo-nextest）。用于更快的测试执行。",
            category: ToolCategory::Command,
            parameters: params_cargo_nextest,
            runner: runner_cargo_nextest,
        },
        ToolSpec {
            name: "cargo_fmt_check",
            description: "运行 cargo fmt --check（代码格式检查）。",
            category: ToolCategory::Lint,
            parameters: params_cargo_fmt_check,
            runner: runner_cargo_fmt_check,
        },
        ToolSpec {
            name: "cargo_outdated",
            description: "运行 cargo outdated（检查依赖是否过期/可升级）。",
            category: ToolCategory::Lint,
            parameters: params_cargo_outdated,
            runner: runner_cargo_outdated,
        },
        ToolSpec {
            name: "cargo_publish_dry_run",
            description: "运行 cargo publish --dry-run：验证打包与发布检查，**不会**上传到 registry。可选 package、allow_dirty、no_verify、features。",
            category: ToolCategory::Command,
            parameters: params_cargo_publish_dry_run,
            runner: runner_cargo_publish_dry_run,
        },
        ToolSpec {
            name: "rust_compiler_json",
            description: "运行 `cargo check --message-format=<json>`，解析 **compiler-message** 行，输出结构化诊断摘要（级别、错误码、rendered、span）。等价于对接 rustc 的 JSON 诊断流，无需 rust-analyzer。",
            category: ToolCategory::Lint,
            parameters: params_rust_compiler_json,
            runner: runner_rust_compiler_json,
        },
        ToolSpec {
            name: "rust_analyzer_goto_definition",
            description: "启动 **rust-analyzer**（stdio LSP），对指定 **path + 0-based line/character** 执行 `textDocument/definition`。需本机已安装 rust-analyzer；单文件 didOpen，适合跳转定义。大文件 >512KiB 请换 read_file 分段。",
            category: ToolCategory::Search,
            parameters: params_rust_analyzer_position,
            runner: runner_rust_analyzer_goto_definition,
        },
        ToolSpec {
            name: "rust_analyzer_find_references",
            description: "同上，执行 `textDocument/references`（语义引用）。参数含 include_declaration。",
            category: ToolCategory::Search,
            parameters: params_rust_analyzer_references,
            runner: runner_rust_analyzer_find_references,
        },
        ToolSpec {
            name: "cargo_fix",
            description: "执行 cargo fix 应用编译器/诊断建议（受控写入，需 confirm=true）。",
            category: ToolCategory::Command,
            parameters: params_cargo_fix,
            runner: runner_cargo_fix,
        },
        ToolSpec {
            name: "rust_test_one",
            description: "运行单个 Rust 测试（按 test_name 过滤）。用于快速调试具体测试。",
            category: ToolCategory::Command,
            parameters: params_rust_test_one,
            runner: runner_rust_test_one,
        },
        ToolSpec {
            name: "frontend_lint",
            description: "运行前端 npm lint（结构化参数）。支持指定前端子目录和 script 名称。",
            category: ToolCategory::Lint,
            parameters: params_frontend_lint,
            runner: runner_frontend_lint,
        },
        ToolSpec {
            name: "frontend_build",
            description: "运行前端 npm build（结构化参数）。支持指定前端子目录和 script 名称（默认 build）。",
            category: ToolCategory::Command,
            parameters: params_frontend_lint,
            runner: runner_frontend_build,
        },
        ToolSpec {
            name: "frontend_test",
            description: "运行前端 npm test（结构化参数）。支持指定前端子目录和 script 名称（默认 test）。",
            category: ToolCategory::Command,
            parameters: params_frontend_lint,
            runner: runner_frontend_test,
        },
        ToolSpec {
            name: "cargo_audit",
            description: "运行 cargo audit 做依赖漏洞扫描（需要已安装 cargo-audit）。",
            category: ToolCategory::Lint,
            parameters: params_cargo_audit,
            runner: runner_cargo_audit,
        },
        ToolSpec {
            name: "cargo_deny",
            description: "运行 cargo deny check（需要已安装 cargo-deny），做许可证/安全策略检查。",
            category: ToolCategory::Lint,
            parameters: params_cargo_deny,
            runner: runner_cargo_deny,
        },
        ToolSpec {
            name: "ci_pipeline_local",
            description: "本地一键执行 CI 关键检查（fmt/clippy/test/frontend_lint）。",
            category: ToolCategory::Lint,
            parameters: params_ci_pipeline_local,
            runner: runner_ci_pipeline_local,
        },
        ToolSpec {
            name: "release_ready_check",
            description: "发布前一键检查：CI + audit + deny + 工作区干净检查。",
            category: ToolCategory::Lint,
            parameters: params_release_ready_check,
            runner: runner_release_ready_check,
        },
        ToolSpec {
            name: "workflow_execute",
            description: "执行 DAG 工作流：并行/串行调度 + 人工审批节点 + SLA 超时 + 失败补偿。",
            category: ToolCategory::Command,
            parameters: params_workflow_execute,
            runner: runner_workflow_execute,
        },
        ToolSpec {
            name: "rust_backtrace_analyze",
            description: "分析 Rust panic/backtrace 文本，提取首个可疑业务帧和模块命中统计。",
            category: ToolCategory::Utility,
            parameters: params_backtrace_analyze,
            runner: runner_backtrace_analyze,
        },
        ToolSpec {
            name: "git_status",
            description: "读取当前工作区的 Git 状态（只读）。可查看分支、已暂存/未暂存变更和未跟踪文件，帮助在改动前后自检变更范围，避免覆盖未提交内容。",
            category: ToolCategory::Command,
            parameters: params_git_status,
            runner: runner_git_status,
        },
        ToolSpec {
            name: "git_diff",
            description: "读取当前工作区的 Git diff（只读）。支持查看 working、staged 或 all 模式，并可按 path 过滤，便于精确确认具体改动。",
            category: ToolCategory::Command,
            parameters: params_git_diff,
            runner: runner_git_diff,
        },
        ToolSpec {
            name: "git_clean_check",
            description: "检查当前工作区是否干净（git status --porcelain）。",
            category: ToolCategory::Command,
            parameters: params_git_clean_check,
            runner: runner_git_clean_check,
        },
        ToolSpec {
            name: "git_diff_stat",
            description: "读取当前工作区的 Git diff 统计（只读）。支持 working/staged/all 与可选 path 过滤。",
            category: ToolCategory::Command,
            parameters: params_git_diff_stat,
            runner: runner_git_diff_stat,
        },
        ToolSpec {
            name: "git_diff_names",
            description: "读取当前工作区的 Git diff 变更文件名列表（只读）。支持 working/staged/all 与可选 path 过滤。",
            category: ToolCategory::Command,
            parameters: params_git_diff_names,
            runner: runner_git_diff_names,
        },
        ToolSpec {
            name: "git_log",
            description: "读取 Git 提交历史（只读）。支持条数和单行模式。",
            category: ToolCategory::Command,
            parameters: params_git_log,
            runner: runner_git_log,
        },
        ToolSpec {
            name: "git_show",
            description: "读取指定提交详情（只读），默认 HEAD。",
            category: ToolCategory::Command,
            parameters: params_git_show,
            runner: runner_git_show,
        },
        ToolSpec {
            name: "git_diff_base",
            description: "读取 base...HEAD 范围 diff（只读），默认 main...HEAD。",
            category: ToolCategory::Command,
            parameters: params_git_diff_base,
            runner: runner_git_diff_base,
        },
        ToolSpec {
            name: "git_blame",
            description: "查看文件行级 blame（只读）。可选行范围。",
            category: ToolCategory::Command,
            parameters: params_git_blame,
            runner: runner_git_blame,
        },
        ToolSpec {
            name: "git_file_history",
            description: "查看单文件历史（只读，--follow）。",
            category: ToolCategory::Command,
            parameters: params_git_file_history,
            runner: runner_git_file_history,
        },
        ToolSpec {
            name: "git_branch_list",
            description: "查看分支列表（只读），可含远程分支。",
            category: ToolCategory::Command,
            parameters: params_git_branch_list,
            runner: runner_git_branch_list,
        },
        ToolSpec {
            name: "git_remote_status",
            description: "查看本地分支与远程跟踪关系（只读，git status -sb）。",
            category: ToolCategory::Command,
            parameters: params_git_status,
            runner: runner_git_remote_status,
        },
        ToolSpec {
            name: "git_stage_files",
            description: "将指定相对路径加入暂存区（受控写入）。",
            category: ToolCategory::Command,
            parameters: params_git_stage_files,
            runner: runner_git_stage_files,
        },
        ToolSpec {
            name: "git_commit",
            description: "执行 git commit（受控写入，需 confirm=true）。可选先 stage_all。",
            category: ToolCategory::Command,
            parameters: params_git_commit,
            runner: runner_git_commit,
        },
        ToolSpec {
            name: "git_fetch",
            description: "执行 git fetch（可选 remote/branch/prune）。",
            category: ToolCategory::Command,
            parameters: params_git_fetch,
            runner: runner_git_fetch,
        },
        ToolSpec {
            name: "git_remote_list",
            description: "查看远程仓库列表（git remote -v）。",
            category: ToolCategory::Command,
            parameters: params_empty_object,
            runner: runner_git_remote_list,
        },
        ToolSpec {
            name: "git_remote_set_url",
            description: "设置远程仓库 URL（受控写入，需 confirm=true）。",
            category: ToolCategory::Command,
            parameters: params_git_remote_set_url,
            runner: runner_git_remote_set_url,
        },
        ToolSpec {
            name: "git_apply",
            description: "执行 git apply。默认 check_only=true 先检查可应用性。",
            category: ToolCategory::Command,
            parameters: params_git_apply,
            runner: runner_git_apply,
        },
        ToolSpec {
            name: "git_clone",
            description: "执行 git clone 到工作区内目标目录（受控写入，需 confirm=true）。",
            category: ToolCategory::Command,
            parameters: params_git_clone,
            runner: runner_git_clone,
        },
        ToolSpec {
            name: "create_file",
            description: "在工作区内创建新文件。仅当文件不存在时创建；若路径已存在则报错。路径相对于工作目录，不能包含 .. 超出工作目录。用于用户要求「新建文件」「创建 xx」等。参数 path 为相对路径，content 为文件内容。",
            category: ToolCategory::File,
            parameters: params_file_write,
            runner: runner_create_file,
        },
        ToolSpec {
            name: "modify_file",
            description: "在工作区内修改已有文件。mode=full（默认）：整文件覆盖。mode=replace_lines：仅替换 start_line..=end_line 为 content，流式读写不写全文件进内存，适合大文件局部修改。",
            category: ToolCategory::File,
            parameters: params_modify_file,
            runner: runner_modify_file,
        },
        ToolSpec {
            name: "read_file",
            description: "按行流式读取文件（不把整文件载入内存）。默认单次最多返回 max_lines=500 行（可调到 8000）；未指定 end_line 时自动分段。输出提示下一段 start_line。可选 count_total_lines 统计总行数（大文件慎用）。",
            category: ToolCategory::File,
            parameters: params_read_file,
            runner: runner_read_file,
        },
        ToolSpec {
            name: "read_dir",
            description: "在工作区内读取目录下的文件/子目录列表（受控只读）。可选包含隐藏项与最大条数。",
            category: ToolCategory::File,
            parameters: params_read_dir,
            runner: runner_read_dir,
        },
        ToolSpec {
            name: "glob_files",
            description: "在工作区内从指定子目录起递归扫描，按 glob 模式匹配**文件**相对路径（如 **/*.rs）。带 max_depth、max_results 上限；路径均在工作区内解析，禁止 ..。优先于 run_command find。",
            category: ToolCategory::File,
            parameters: params_glob_files,
            runner: runner_glob_files,
        },
        ToolSpec {
            name: "list_tree",
            description: "在工作区内从指定目录起递归列出子路径（先序、字典序），前缀 dir:/file:；带 max_depth、max_entries。用于快速看目录树而不用 find。",
            category: ToolCategory::File,
            parameters: params_list_tree,
            runner: runner_list_tree,
        },
        ToolSpec {
            name: "file_exists",
            description: "检查工作区内某路径（文件或目录）是否存在，并可按 kind=file|dir|any 过滤。",
            category: ToolCategory::Utility,
            parameters: params_file_exists,
            runner: runner_file_exists,
        },
        ToolSpec {
            name: "read_binary_meta",
            description: "读取任意文件的元数据（大小、可选修改时间）及**文件头一段的 SHA256**，不把整文件读入上下文；适合二进制/大文件比对。prefix_hash_bytes 默认 8192，0 表示跳过哈希。",
            category: ToolCategory::Utility,
            parameters: params_read_binary_meta,
            runner: runner_read_binary_meta,
        },
        ToolSpec {
            name: "extract_in_file",
            description: "在指定文件内按正则抽取匹配行（只读）。返回带行号的匹配行，并支持截断。",
            category: ToolCategory::File,
            parameters: params_extract_in_file,
            runner: runner_extract_in_file,
        },
        ToolSpec {
            name: "apply_patch",
            description: "应用 **unified diff**。路径：--- src/…（strip=0）或 --- a/src/…（strip=1）；hunk **带 2～3 行上下文**；**小步**单主题；可 **patch -R** / **git checkout** 回滚。先 dry-run。",
            category: ToolCategory::File,
            parameters: params_apply_patch,
            runner: runner_apply_patch,
        },
        ToolSpec {
            name: "search_in_files",
            description: "在当前工作区内搜索文件内容。支持按正则或普通关键词搜索，返回匹配的文件路径、行号和包含匹配的行片段。适合回答「某个函数/类型/常量在哪定义」「有哪些地方包含 TODO」等问题。",
            category: ToolCategory::Search,
            parameters: params_search_in_files,
            runner: runner_search_in_files,
        },
        ToolSpec {
            name: "find_symbol",
            description: "在当前工作区递归定位 Rust 符号的潜在定义位置（如 fn/struct/enum/trait/const/static/type/mod）。返回匹配行与上下文。",
            category: ToolCategory::Search,
            parameters: params_find_symbol,
            runner: runner_find_symbol,
        },
        ToolSpec {
            name: "find_references",
            description: "在 .rs 源文件中按词边界搜索某标识符的引用位置；默认排除与 find_symbol 一致的「疑似定义」行。适合在改名、删函数前快速扫一遍使用处。",
            category: ToolCategory::Search,
            parameters: params_find_references,
            runner: runner_find_references,
        },
        ToolSpec {
            name: "rust_file_outline",
            description: "读取单个 Rust 源文件，列出其中常见的顶层结构行摘要（mod/fn/struct/enum/trait/impl/use 等），便于大文件导航与拆分任务。",
            category: ToolCategory::Search,
            parameters: params_rust_file_outline,
            runner: runner_rust_file_outline,
        },
        ToolSpec {
            name: "format_file",
            description: "对工作区内的文件进行代码格式化。根据文件扩展名自动选择合适的本地格式化器，例如 Rust 文件使用 rustfmt，前端 TypeScript/JavaScript 文件使用项目内的 Prettier。适合在修改代码后统一整理缩进和风格。注意：需要本地已安装相应格式化工具（如 rustfmt、npm 项目内的 prettier）。",
            category: ToolCategory::Format,
            parameters: params_format_file,
            runner: runner_format_file,
        },
        ToolSpec {
            name: "format_check_file",
            description: "对单个文件做格式检查（不修改磁盘）：Rust 使用 rustfmt --check，前端类文件使用 prettier --check。适合在提交前确认风格一致。",
            category: ToolCategory::Format,
            parameters: params_format_check_file,
            runner: runner_format_check_file,
        },
        ToolSpec {
            name: "run_lints",
            description: "运行项目的静态检查工具并聚合结果。目前包括：后端的 cargo clippy 和（若存在 frontend 目录与 package.json）前端的 npm run lint。可用于在改动后检查潜在问题。",
            category: ToolCategory::Lint,
            parameters: params_run_lints,
            runner: runner_run_lints,
        },
        ToolSpec {
            name: "quality_workspace",
            description: "按开关组合运行质量检查：默认 cargo fmt --check + cargo clippy（轻量）；可选 cargo test、frontend npm lint、frontend prettier --check。适合「改完一轮后」快速拉齐格式与静态分析。",
            category: ToolCategory::Lint,
            parameters: params_quality_workspace,
            runner: runner_quality_workspace,
        },
        ToolSpec {
            name: "add_reminder",
            description: "添加一个提醒事项，并持久化到工作区的 .crabmate/reminders.json。可选 due_at 支持 RFC3339 或 YYYY-MM-DD HH:MM / YYYY-MM-DD。",
            category: ToolCategory::Schedule,
            parameters: params_add_reminder,
            runner: runner_add_reminder,
        },
        ToolSpec {
            name: "list_reminders",
            description: "列出提醒事项（默认不包含已完成）。数据来自工作区的 .crabmate/reminders.json。",
            category: ToolCategory::Schedule,
            parameters: params_list_reminders,
            runner: runner_list_reminders,
        },
        ToolSpec {
            name: "complete_reminder",
            description: "将指定 id 的提醒标记为完成。",
            category: ToolCategory::Schedule,
            parameters: params_id_only,
            runner: runner_complete_reminder,
        },
        ToolSpec {
            name: "delete_reminder",
            description: "删除指定 id 的提醒。",
            category: ToolCategory::Schedule,
            parameters: params_id_only,
            runner: runner_delete_reminder,
        },
        ToolSpec {
            name: "update_reminder",
            description: "更新提醒（title/due_at/done 任意字段）。due_at 传空字符串表示清空到期时间。",
            category: ToolCategory::Schedule,
            parameters: params_update_reminder,
            runner: runner_update_reminder,
        },
        ToolSpec {
            name: "add_event",
            description: "添加一个日程事件，并持久化到工作区的 .crabmate/events.json。start_at 必填，end_at/location/notes 可选。",
            category: ToolCategory::Schedule,
            parameters: params_add_event,
            runner: runner_add_event,
        },
        ToolSpec {
            name: "list_events",
            description: "列出日程事件；可选按 year/month 过滤。数据来自工作区的 .crabmate/events.json。",
            category: ToolCategory::Schedule,
            parameters: params_list_events,
            runner: runner_list_events,
        },
        ToolSpec {
            name: "delete_event",
            description: "删除指定 id 的日程事件。",
            category: ToolCategory::Schedule,
            parameters: params_id_only,
            runner: runner_delete_event,
        },
        ToolSpec {
            name: "update_event",
            description: "更新日程事件（title/start_at/end_at/location/notes 任意字段）。end_at/location/notes 传空字符串表示清空。",
            category: ToolCategory::Schedule,
            parameters: params_update_event,
            runner: runner_update_event,
        },
    ]
}

/// 构建传给 API 的工具列表（表驱动注册）。
pub fn build_tools() -> Vec<Tool> {
    build_tools_filtered(None)
}

/// 构建传给 API 的工具列表：可按分类过滤（便于未来精确禁用某类工具）。
pub fn build_tools_filtered(allowed: Option<&[ToolCategory]>) -> Vec<Tool> {
    let allowed = allowed.unwrap_or(&[]);
    tool_specs()
        .iter()
        .filter(|s| allowed.is_empty() || allowed.contains(&s.category))
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

/// 判断本次 run_command 是否为“成功的编译命令”（gcc/g++/make/cmake 且退出码为 0）
pub(crate) fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let cmd = v
        .get("command")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_lowercase());
    let is_compile_cmd = cmd
        .as_deref()
        .is_some_and(|c| matches!(c, "gcc" | "g++" | "make" | "cmake"));
    if !is_compile_cmd {
        return false;
    }
    // run_command 输出的第一行形如：退出码：0
    let first_line = result.lines().next().unwrap_or("");
    if let Some(rest) = first_line.strip_prefix("退出码：")
        && let Ok(code) = rest.trim().parse::<i32>() {
            return code == 0;
        }
    false
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub(crate) fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    match name {
        "run_command" => {
            let cmd = v.get("command")?.as_str()?.trim();
            let args = v
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let s = if args.is_empty() {
                format!("执行命令：{}", cmd)
            } else {
                format!("执行命令：{} {}", cmd, args)
            };
            Some(s)
        }
        "cargo_check" => Some("运行 cargo check".to_string()),
        "cargo_test" => Some("运行 cargo test".to_string()),
        "cargo_clippy" => Some("运行 cargo clippy".to_string()),
        "cargo_metadata" => Some("读取 cargo metadata".to_string()),
        "cargo_tree" => Some("查看 cargo 依赖树".to_string()),
        "cargo_clean" => Some("运行 cargo clean".to_string()),
        "cargo_doc" => Some("生成 cargo 文档".to_string()),
        "cargo_run" => Some("运行 cargo run".to_string()),
        "cargo_nextest" => Some("运行 cargo nextest".to_string()),
        "cargo_fmt_check" => Some("运行 cargo fmt --check".to_string()),
        "cargo_outdated" => Some("运行 cargo outdated".to_string()),
        "cargo_publish_dry_run" => Some("cargo publish --dry-run".to_string()),
        "rust_compiler_json" => Some("cargo check JSON 诊断".to_string()),
        "rust_analyzer_goto_definition" => {
            let path = v.get("path")?.as_str()?.trim();
            let line = v.get("line").and_then(|x| x.as_u64());
            Some(format!("RA 跳转定义：{}:{}", path, line.unwrap_or(0)))
        },
        "rust_analyzer_find_references" => {
            let path = v.get("path")?.as_str()?.trim();
            let line = v.get("line").and_then(|x| x.as_u64());
            Some(format!("RA 查找引用：{}:{}", path, line.unwrap_or(0)))
        },
        "cargo_fix" => Some("运行 cargo fix（受控写入）".to_string()),
        "rust_test_one" => Some("运行单个 Rust 测试".to_string()),
        "frontend_lint" => Some("运行前端 lint".to_string()),
        "frontend_build" => Some("运行前端 build".to_string()),
        "frontend_test" => Some("运行前端 test".to_string()),
        "cargo_audit" => Some("运行 cargo audit".to_string()),
        "cargo_deny" => Some("运行 cargo deny".to_string()),
        "ci_pipeline_local" => Some("运行本地 CI 流水线".to_string()),
        "release_ready_check" => Some("运行发布前一键检查".to_string()),
        "workflow_execute" => Some("执行 DAG 工作流".to_string()),
        "rust_backtrace_analyze" => Some("分析 Rust backtrace".to_string()),
        "git_status" => Some("查看 Git 状态".to_string()),
        "git_clean_check" => Some("检查 Git 工作区是否干净".to_string()),
        "git_diff" => {
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.trim().is_empty() {
                Some(format!("查看 Git diff（{}）", mode))
            } else {
                Some(format!("查看 Git diff（{}）：{}", mode, path.trim()))
            }
        }
        "git_diff_stat" => {
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.trim().is_empty() {
                Some(format!("查看 Git diff 统计（{}）", mode))
            } else {
                Some(format!("查看 Git diff 统计（{}）：{}", mode, path.trim()))
            }
        }
        "git_diff_names" => {
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("working");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.trim().is_empty() {
                Some(format!("查看 Git diff 变更文件名（{}）", mode))
            } else {
                Some(format!("查看 Git diff 变更文件名（{}）：{}", mode, path.trim()))
            }
        }
        "git_log" => Some("查看 Git 提交历史".to_string()),
        "git_show" => Some("查看 Git 提交详情".to_string()),
        "git_diff_base" => Some("查看 base...HEAD 差异".to_string()),
        "git_blame" => Some("查看 Git blame".to_string()),
        "git_file_history" => Some("查看文件 Git 历史".to_string()),
        "git_branch_list" => Some("查看分支列表".to_string()),
        "git_remote_status" => Some("查看远程跟踪状态".to_string()),
        "git_stage_files" => Some("暂存文件".to_string()),
        "git_commit" => Some("提交变更".to_string()),
        "git_fetch" => Some("拉取远程更新".to_string()),
        "git_remote_list" => Some("查看远程仓库".to_string()),
        "git_remote_set_url" => Some("设置远程 URL".to_string()),
        "git_apply" => Some("应用 Git 补丁".to_string()),
        "git_clone" => Some("克隆仓库".to_string()),
        "create_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("新建文件：{}", path))
        }
        "modify_file" => {
            let path = v.get("path")?.as_str()?.trim();
            let mode = v.get("mode").and_then(|x| x.as_str()).unwrap_or("full");
            if mode == "replace_lines" {
                let s = v.get("start_line").and_then(|x| x.as_u64());
                let e = v.get("end_line").and_then(|x| x.as_u64());
                Some(format!(
                    "修改文件（行替换 {}-{}）：{}",
                    s.unwrap_or(0),
                    e.unwrap_or(0),
                    path
                ))
            } else {
                Some(format!("修改文件：{}", path))
            }
        }
        "read_file" => {
            let path = v.get("path")?.as_str()?.trim();
            let start = v.get("start_line").and_then(|x| x.as_u64());
            let end = v.get("end_line").and_then(|x| x.as_u64());
            let ml = v.get("max_lines").and_then(|x| x.as_u64());
            let suffix = match (start, end, ml) {
                (Some(s), Some(e), _) => format!(" [{}-{}]", s, e),
                (Some(s), None, Some(m)) => format!(" [{}~ max_lines={}]", s, m),
                (Some(s), None, None) => format!(" [{}~]", s),
                (None, Some(e), _) => format!(" [1-{}]", e),
                (None, None, Some(m)) => format!(" [分段 max_lines={}]", m),
                (None, None, None) => String::new(),
            };
            Some(format!("读取文件：{}{}", path, suffix))
        }
        "read_dir" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            if path.is_empty() {
                Some("读取目录".to_string())
            } else {
                Some(format!("读取目录：{}", path))
            }
        }
        "web_search" => {
            let q = v.get("query")?.as_str()?.trim();
            Some(format!("联网搜索：{}", q))
        }
        "glob_files" => {
            let pat = v.get("pattern")?.as_str()?.trim();
            let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            Some(format!("glob 匹配：{} @ {}", pat, if root.is_empty() { "." } else { root }))
        }
        "list_tree" => {
            let root = v.get("path").and_then(|x| x.as_str()).unwrap_or(".").trim();
            Some(format!(
                "递归列目录：{}",
                if root.is_empty() { "." } else { root }
            ))
        }
        "file_exists" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("检查是否存在：{}", path))
        }
        "read_binary_meta" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("二进制元数据：{}", path))
        }
        "extract_in_file" => {
            let path = v.get("path")?.as_str()?.trim();
            let pattern = v.get("pattern")?.as_str()?.trim();
            Some(format!("从文件提取匹配：{} / {}", path, pattern))
        }
        "apply_patch" => {
            let patch = v.get("patch")?.as_str()?;
            let files = patch
                .lines()
                .filter_map(|line| line.strip_prefix("+++ "))
                .map(|s| s.split_whitespace().next().unwrap_or(""))
                .filter(|s| !s.is_empty() && *s != "/dev/null")
                .map(|s| s.trim_start_matches("b/").trim_start_matches("a/").to_string())
                .collect::<Vec<_>>();
            if files.is_empty() {
                Some("应用补丁".to_string())
            } else {
                Some(format!("应用补丁：{}", files.join(", ")))
            }
        }
        "run_executable" => {
            let path = v.get("path")?.as_str()?.trim();
            let args = v
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let s = if args.is_empty() {
                format!("运行可执行：{}", path)
            } else {
                format!("运行可执行：{} {}", path, args)
            };
            Some(s)
        }
        "find_symbol" => {
            let symbol = v.get("symbol")?.as_str()?.trim();
            Some(format!("查找符号：{}", symbol))
        }
        "find_references" => {
            let symbol = v.get("symbol")?.as_str()?.trim();
            Some(format!("查找引用：{}", symbol))
        }
        "rust_file_outline" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("Rust 大纲：{}", path))
        }
        "format_check_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("格式检查：{}", path))
        }
        "quality_workspace" => Some("工作区质量检查".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const TEST_COMMAND_MAX_OUTPUT_LEN: usize = 8192;
    const TEST_WEATHER_TIMEOUT_SECS: u64 = 15;
    fn test_ctx<'a>(allowed_commands: &'a [String]) -> ToolContext<'a> {
        ToolContext {
            command_max_output_len: TEST_COMMAND_MAX_OUTPUT_LEN,
            weather_timeout_secs: TEST_WEATHER_TIMEOUT_SECS,
            allowed_commands,
            working_dir: test_work_dir(),
            web_search_timeout_secs: 15,
            web_search_provider: crate::config::WebSearchProvider::Brave,
            web_search_api_key: "",
            web_search_max_results: 5,
        }
    }
    fn test_allowed_commands() -> Vec<String> {
        vec![
            "ls".into(),
            "pwd".into(),
            "whoami".into(),
            "date".into(),
            "echo".into(),
            "id".into(),
            "uname".into(),
            "env".into(),
            "df".into(),
            "du".into(),
            "head".into(),
            "tail".into(),
            "wc".into(),
            "cat".into(),
            "cmake".into(),
            "gcc".into(),
            "g++".into(),
            "make".into(),
        ]
    }
    fn test_work_dir() -> &'static Path {
        Path::new(".")
    }

    #[test]
    fn test_run_tool_unknown() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("unknown_tool", "{}", &ctx);
        assert_eq!(out, "未知工具：unknown_tool");
    }

    #[test]
    fn test_run_tool_calc_missing_expression() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("calc", "{}", &ctx);
        assert_eq!(out, "错误：缺少 expression 参数");
    }

    #[test]
    fn test_run_tool_calc_expression() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("calc", r#"{"expression":"1+1"}"#, &ctx);
        assert!(out.contains("2"), "calc 1+1 应得到 2，得到: {}", out);
    }

    #[test]
    fn test_run_tool_get_current_time() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("get_current_time", "{}", &ctx);
        assert!(out.contains("当前时间"), "时间工具应包含「当前时间」，得到: {}", out);
    }

    #[test]
    fn test_run_tool_run_command_pwd() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("run_command", r#"{"command":"pwd"}"#, &ctx);
        assert!(out.contains("退出码：0"), "pwd 应成功，得到: {}", out);
    }

    #[test]
    fn test_run_tool_run_command_disallowed() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("run_command", r#"{"command":"rm"}"#, &ctx);
        assert!(out.contains("不允许的命令"), "应拒绝 rm，得到: {}", out);
    }

    #[test]
    fn test_run_tool_get_weather_missing_param() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool("get_weather", "{}", &ctx);
        assert!(out.contains("city") || out.contains("location"), "缺少参数应提示，得到: {}", out);
    }

    #[test]
    fn test_run_tool_web_search_no_api_key() {
        let allowed = test_allowed_commands();
        let ctx = test_ctx(&allowed);
        let out = run_tool(
            "web_search",
            r#"{"query":"Rust programming"}"#,
            &ctx,
        );
        assert!(
            out.contains("未配置") && out.contains("web_search"),
            "无 Key 时应提示配置，得到: {}",
            out
        );
    }

    #[test]
    fn test_build_tools_names() {
        let tools = build_tools();
        let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"get_current_time"));
        assert!(names.contains(&"calc"));
        assert!(names.contains(&"get_weather"));
        assert!(names.contains(&"web_search"));
        assert!(names.contains(&"run_command"));
        assert!(names.contains(&"cargo_check"));
        assert!(names.contains(&"cargo_test"));
        assert!(names.contains(&"cargo_clippy"));
        assert!(names.contains(&"cargo_metadata"));
        assert!(names.contains(&"cargo_tree"));
        assert!(names.contains(&"cargo_clean"));
        assert!(names.contains(&"cargo_doc"));
        assert!(names.contains(&"cargo_run"));
        assert!(names.contains(&"cargo_nextest"));
        assert!(names.contains(&"cargo_fmt_check"));
        assert!(names.contains(&"cargo_outdated"));
        assert!(names.contains(&"cargo_publish_dry_run"));
        assert!(names.contains(&"rust_compiler_json"));
        assert!(names.contains(&"rust_analyzer_goto_definition"));
        assert!(names.contains(&"rust_analyzer_find_references"));
        assert!(names.contains(&"cargo_fix"));
        assert!(names.contains(&"rust_test_one"));
        assert!(names.contains(&"frontend_lint"));
        assert!(names.contains(&"frontend_build"));
        assert!(names.contains(&"frontend_test"));
        assert!(names.contains(&"cargo_audit"));
        assert!(names.contains(&"cargo_deny"));
        assert!(names.contains(&"ci_pipeline_local"));
        assert!(names.contains(&"release_ready_check"));
        assert!(names.contains(&"workflow_execute"));
        assert!(names.contains(&"rust_backtrace_analyze"));
        assert!(names.contains(&"git_status"));
        assert!(names.contains(&"git_clean_check"));
        assert!(names.contains(&"git_diff"));
        assert!(names.contains(&"git_diff_stat"));
        assert!(names.contains(&"git_diff_names"));
        assert!(names.contains(&"git_log"));
        assert!(names.contains(&"git_show"));
        assert!(names.contains(&"git_diff_base"));
        assert!(names.contains(&"git_blame"));
        assert!(names.contains(&"git_file_history"));
        assert!(names.contains(&"git_branch_list"));
        assert!(names.contains(&"git_remote_status"));
        assert!(names.contains(&"git_stage_files"));
        assert!(names.contains(&"git_commit"));
        assert!(names.contains(&"git_fetch"));
        assert!(names.contains(&"git_remote_list"));
        assert!(names.contains(&"git_remote_set_url"));
        assert!(names.contains(&"git_apply"));
        assert!(names.contains(&"git_clone"));
        assert!(names.contains(&"create_file"));
        assert!(names.contains(&"modify_file"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"read_dir"));
        assert!(names.contains(&"glob_files"));
        assert!(names.contains(&"list_tree"));
        assert!(names.contains(&"file_exists"));
        assert!(names.contains(&"read_binary_meta"));
        assert!(names.contains(&"extract_in_file"));
        assert!(names.contains(&"find_symbol"));
        assert!(names.contains(&"find_references"));
        assert!(names.contains(&"rust_file_outline"));
        assert!(names.contains(&"format_file"));
        assert!(names.contains(&"format_check_file"));
        assert!(names.contains(&"run_lints"));
        assert!(names.contains(&"quality_workspace"));
        assert!(names.contains(&"apply_patch"));
        assert!(names.contains(&"run_executable"));
    }
}
