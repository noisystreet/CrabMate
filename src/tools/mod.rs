//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod command;
mod exec;
mod file;
mod lint;
mod format;
mod grep;
mod schedule;
mod time;
mod weather;

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

fn runner_create_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_file(args, ctx.working_dir)
}

fn runner_modify_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::modify_file(args, ctx.working_dir)
}

fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
}

fn runner_format_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run(args, ctx.working_dir)
}

fn runner_run_lints(args: &str, ctx: &ToolContext<'_>) -> String {
    lint::run(args, ctx.working_dir, ctx.command_max_output_len)
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
            name: "create_file",
            description: "在工作区内创建新文件。仅当文件不存在时创建；若路径已存在则报错。路径相对于工作目录，不能包含 .. 超出工作目录。用于用户要求「新建文件」「创建 xx」等。参数 path 为相对路径，content 为文件内容。",
            category: ToolCategory::File,
            parameters: params_file_write,
            runner: runner_create_file,
        },
        ToolSpec {
            name: "modify_file",
            description: "在工作区内修改已有文件内容（覆盖写入）。仅当文件已存在时修改；若路径不存在或不是文件则报错。用于用户要求「修改 xx」「把 xx 改成」「编辑文件」等。参数 path 为相对路径，content 为新的文件内容。",
            category: ToolCategory::File,
            parameters: params_file_write,
            runner: runner_modify_file,
        },
        ToolSpec {
            name: "search_in_files",
            description: "在当前工作区内搜索文件内容。支持按正则或普通关键词搜索，返回匹配的文件路径、行号和包含匹配的行片段。适合回答「某个函数/类型/常量在哪定义」「有哪些地方包含 TODO」等问题。",
            category: ToolCategory::Search,
            parameters: params_search_in_files,
            runner: runner_search_in_files,
        },
        ToolSpec {
            name: "format_file",
            description: "对工作区内的文件进行代码格式化。根据文件扩展名自动选择合适的本地格式化器，例如 Rust 文件使用 rustfmt，前端 TypeScript/JavaScript 文件使用项目内的 Prettier。适合在修改代码后统一整理缩进和风格。注意：需要本地已安装相应格式化工具（如 rustfmt、npm 项目内的 prettier）。",
            category: ToolCategory::Format,
            parameters: params_format_file,
            runner: runner_format_file,
        },
        ToolSpec {
            name: "run_lints",
            description: "运行项目的静态检查工具并聚合结果。目前包括：后端的 cargo clippy 和（若存在 frontend 目录与 package.json）前端的 npm run lint。可用于在改动后检查潜在问题。",
            category: ToolCategory::Lint,
            parameters: params_run_lints,
            runner: runner_run_lints,
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
/// `command_max_output_len`、`allowed_commands`、`run_command_working_dir` 仅用于 run_command；`weather_timeout_secs` 仅用于 get_weather。
pub fn run_tool(
    name: &str,
    args_json: &str,
    command_max_output_len: usize,
    weather_timeout_secs: u64,
    allowed_commands: &[String],
    run_command_working_dir: &std::path::Path,
) -> String {
    let ctx = ToolContext {
        command_max_output_len,
        weather_timeout_secs,
        allowed_commands,
        working_dir: run_command_working_dir,
    };
    match tool_specs().iter().find(|s| s.name == name) {
        Some(spec) => (spec.runner)(args_json, &ctx),
        None => format!("未知工具：{}", name),
    }
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
        .map_or(false, |c| matches!(c, "gcc" | "g++" | "make" | "cmake"));
    if !is_compile_cmd {
        return false;
    }
    // run_command 输出的第一行形如：退出码：0
    let first_line = result.lines().next().unwrap_or("");
    if let Some(rest) = first_line.strip_prefix("退出码：") {
        if let Ok(code) = rest.trim().parse::<i32>() {
            return code == 0;
        }
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
        "create_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("新建文件：{}", path))
        }
        "modify_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("修改文件：{}", path))
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const TEST_COMMAND_MAX_OUTPUT_LEN: usize = 8192;
    const TEST_WEATHER_TIMEOUT_SECS: u64 = 15;
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
        let out = run_tool(
            "unknown_tool",
            "{}",
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert_eq!(out, "未知工具：unknown_tool");
    }

    #[test]
    fn test_run_tool_calc_missing_expression() {
        let out = run_tool(
            "calc",
            "{}",
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert_eq!(out, "错误：缺少 expression 参数");
    }

    #[test]
    fn test_run_tool_calc_expression() {
        let out = run_tool(
            "calc",
            r#"{"expression":"1+1"}"#,
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert!(out.contains("2"), "calc 1+1 应得到 2，得到: {}", out);
    }

    #[test]
    fn test_run_tool_get_current_time() {
        let out = run_tool(
            "get_current_time",
            "{}",
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert!(out.contains("当前时间"), "时间工具应包含「当前时间」，得到: {}", out);
    }

    #[test]
    fn test_run_tool_run_command_pwd() {
        let out = run_tool(
            "run_command",
            r#"{"command":"pwd"}"#,
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert!(out.contains("退出码：0"), "pwd 应成功，得到: {}", out);
    }

    #[test]
    fn test_run_tool_run_command_disallowed() {
        let out = run_tool(
            "run_command",
            r#"{"command":"rm"}"#,
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert!(out.contains("不允许的命令"), "应拒绝 rm，得到: {}", out);
    }

    #[test]
    fn test_run_tool_get_weather_missing_param() {
        let out = run_tool(
            "get_weather",
            "{}",
            TEST_COMMAND_MAX_OUTPUT_LEN,
            TEST_WEATHER_TIMEOUT_SECS,
            &test_allowed_commands(),
            test_work_dir(),
        );
        assert!(out.contains("city") || out.contains("location"), "缺少参数应提示，得到: {}", out);
    }

    #[test]
    fn test_build_tools_names() {
        let tools = build_tools();
        let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"get_current_time"));
        assert!(names.contains(&"calc"));
        assert!(names.contains(&"get_weather"));
        assert!(names.contains(&"run_command"));
        assert!(names.contains(&"create_file"));
        assert!(names.contains(&"modify_file"));
        assert!(names.contains(&"run_executable"));
    }
}
