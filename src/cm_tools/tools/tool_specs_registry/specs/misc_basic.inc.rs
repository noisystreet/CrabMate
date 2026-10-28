[
ToolSpec {
            name: tool_names::REGEX_TEST,
            description: "纯内存正则表达式测试。输入 pattern 与 test_strings 数组，返回每条字符串的匹配结果与捕获组。用于验证搜索规则或编写正则时快速测试。",
            category: ToolCategory::Basic,
            parameters: schema_of::<args::RegexTestArgs>,
            runner: ToolRunner::Typed(runner_regex_test_try),
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: tool_names::DATE_CALC,
            description: "日期计算：mode=diff 计算两日期间隔（天/周），mode=offset 在基准日期上加减偏移（+30d/-2w/+1m）。基准默认今天。",
            category: ToolCategory::Basic,
            parameters: schema_of::<args::DateCalcArgs>,
            runner: ToolRunner::Typed(runner_date_calc_try),
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: tool_names::JSON_FORMAT,
            description: "JSON/YAML 格式化与转换（纯内存）：pretty（美化）、compact（压缩）、yaml_to_json、json_to_yaml。输入上限 512KiB。",
            category: ToolCategory::Basic,
            parameters: schema_of::<args::JsonFormatArgs>,
            runner: ToolRunner::Typed(runner_json_format_try),
            summary: ToolSummaryKind::None,
        },
        ToolSpec {
            name: tool_names::ENV_VAR_CHECK,
            description: "环境变量批量检查（只读脱敏）：输入变量名列表，返回每个变量的已设置/未设置状态。**不输出变量值**；可选显示长度和前缀字符。",
            category: ToolCategory::Basic,
            parameters: schema_of::<args::EnvVarCheckArgs>,
            runner: ToolRunner::Typed(runner_env_var_check_try),
            summary: ToolSummaryKind::None,
        },
]
