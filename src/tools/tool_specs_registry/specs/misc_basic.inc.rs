[
ToolSpec {
            name: "regex_test",
            description: "纯内存正则表达式测试。输入 pattern 与 test_strings 数组，返回每条字符串的匹配结果与捕获组。用于验证搜索规则或编写正则时快速测试。",
            category: ToolCategory::Basic,
            parameters: tool_params::params_regex_test,
            runner: runner_regex_test,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "date_calc",
            description: "日期计算：mode=diff 计算两日期间隔（天/周），mode=offset 在基准日期上加减偏移（+30d/-2w/+1m）。基准默认今天。",
            category: ToolCategory::Basic,
            parameters: tool_params::params_date_calc,
            runner: runner_date_calc,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "json_format",
            description: "JSON/YAML 格式化与转换（纯内存）：pretty（美化）、compact（压缩）、yaml_to_json、json_to_yaml。输入上限 512KiB。",
            category: ToolCategory::Basic,
            parameters: tool_params::params_json_format,
            runner: runner_json_format,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "env_var_check",
            description: "环境变量批量检查（只读脱敏）：输入变量名列表，返回每个变量的已设置/未设置状态。**不输出变量值**；可选显示长度和前缀字符。",
            category: ToolCategory::Basic,
            parameters: tool_params::params_env_var_check,
            runner: runner_env_var_check,
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: "todo_scan",
            description: "扫描工作区代码中的 TODO/FIXME/HACK/XXX 等标记，返回文件路径、行号和内容预览。可自定义标记列表和排除目录。",
            category: ToolCategory::Development,
            parameters: tool_params::params_todo_scan,
            runner: runner_todo_scan,
            summary: ToolSummaryKind::None,
        },
        // ── 源码分析工具 ──────────────────────────────────────────
]
