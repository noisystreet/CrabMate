//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod command;
mod exec;
mod file;
mod format;
mod grep;
mod time;
mod weather;

use crate::types::{FunctionDef, Tool};

/// 构建传给 API 的工具列表
pub fn build_tools() -> Vec<Tool> {
    vec![
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "get_current_time".to_string(),
                description: "获取当前日期和时间，用于回答用户关于「现在几点」「今天几号」等问题".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "search_in_files".to_string(),
                description: "在当前工作区内搜索文件内容。支持按正则或普通关键词搜索，返回匹配的文件路径、行号和包含匹配的行片段。适合回答「某个函数/类型/常量在哪定义」「有哪些地方包含 TODO」等问题。".to_string(),
                parameters: serde_json::json!({
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
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "calc".to_string(),
                description: "使用 Linux 的 bc -l 计算器执行数学表达式。支持：四则 + - * / %、乘方 ^；sqrt(x)、s(x)=sin、c(x)=cos、a(x)=atan、l(x)=ln、e(x)=exp；常量 pi、e（在 bc 中为 pi=4*a(1), e=e(1)）。可写 math::sqrt(2)、math::sin(pi/2)、math::log10(100) 等，会转为 bc 语法后执行。示例：1+2*3、2^10、sqrt(2)、s(3.14159/2)。参数 expression 为单个数学表达式。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "expression": {
                            "type": "string",
                            "description": "数学表达式，如 1+2*3、2^10、sqrt(2)、s(pi/2)、math::log10(100)"
                        }
                    },
                    "required": ["expression"]
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "get_weather".to_string(),
                description: "获取指定城市或地区的当前天气（使用 Open-Meteo，无需 API Key）。用于回答「某地天气怎么样」「北京今天天气」等问题。参数 city 或 location 为城市/地区名，如北京、上海、Tokyo、New York。".to_string(),
                parameters: serde_json::json!({
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
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "run_command".to_string(),
                description: "在服务器上执行有限的 Linux 命令。允许的命令：ls, pwd, whoami, date, echo, id, uname, env, df, du, head, tail, wc, cat, cmake, gcc, g++, make。用于列出目录、查看文件、编译 C/C++ 项目（gcc/g++/make）、CMake 配置与构建等。参数 args 为字符串数组，如 [\"-l\"], [\"-n\", \"5\", \"file.txt\"], [\"..\"], [\"--build\", \".\"]。不要执行 rm、mv、chmod 等未在白名单中的命令。".to_string(),
                parameters: serde_json::json!({
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
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "run_executable".to_string(),
                description: "在工作目录下执行可执行程序。path 为相对于工作目录的可执行文件路径（如 ./main、./build/app、out/run_tests），不能使用绝对路径或 .. 超出工作目录。用于运行编译产物、脚本等。参数 path 为相对路径，args 为传给程序的参数数组（可选）。".to_string(),
                parameters: serde_json::json!({
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
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "create_file".to_string(),
                description: "在工作区内创建新文件。仅当文件不存在时创建；若路径已存在则报错。路径相对于工作目录，不能包含 .. 超出工作目录。用于用户要求「新建文件」「创建 xx」等。参数 path 为相对路径，content 为文件内容。".to_string(),
                parameters: serde_json::json!({
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
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "modify_file".to_string(),
                description: "在工作区内修改已有文件内容（覆盖写入）。仅当文件已存在时修改；若路径不存在或不是文件则报错。用于用户要求「修改 xx」「把 xx 改成」「编辑文件」等。参数 path 为相对路径，content 为新的文件内容。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "相对工作目录的文件路径，必须是已存在的文件"
                        },
                        "content": {
                            "type": "string",
                            "description": "要写入的新内容（会整体覆盖原内容）"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name: "format_file".to_string(),
                description: "对工作区内的文件进行代码格式化。根据文件扩展名自动选择合适的本地格式化器，例如 Rust 文件使用 rustfmt，前端 TypeScript/JavaScript 文件使用项目内的 Prettier。适合在修改代码后统一整理缩进和风格。注意：需要本地已安装相应格式化工具（如 rustfmt、npm 项目内的 prettier）。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "相对工作区根目录的文件路径，如 src/main.rs、frontend/src/App.tsx"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
    ]
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
    match name {
        "get_current_time" => time::run(),
        "calc" => {
            let expr = match serde_json::from_str::<serde_json::Value>(args_json)
                .ok()
                .and_then(|v| v.get("expression").and_then(|e| e.as_str()).map(String::from))
            {
                Some(s) => s,
                None => return "错误：缺少 expression 参数".to_string(),
            };
            calc::run(&expr)
        }
        "get_weather" => weather::run(args_json, weather_timeout_secs),
        "run_command" => command::run(
            args_json,
            command_max_output_len,
            allowed_commands,
            run_command_working_dir,
        ),
        "run_executable" => exec::run(args_json, command_max_output_len, run_command_working_dir),
        "create_file" => file::create_file(args_json, run_command_working_dir),
        "modify_file" => file::modify_file(args_json, run_command_working_dir),
        "search_in_files" => grep::run(args_json, run_command_working_dir),
        "format_file" => format::run(args_json, run_command_working_dir),
        _ => format!("未知工具：{}", name),
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
