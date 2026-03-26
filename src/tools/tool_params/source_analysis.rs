//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_shellcheck_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "可选：待检查路径（文件或目录），默认 [\".\"]（递归查找 .sh/.bash 等脚本），最多 24 项"},
            "severity": {"type": "string", "description": "可选：最低严重级别 error/warning/info/style", "enum": ["error","warning","info","style"]},
            "shell": {"type": "string", "description": "可选：指定 shell 方言 sh/bash/dash/ksh", "enum": ["sh","bash","dash","ksh"]},
            "format": {"type": "string", "description": "可选：输出格式 tty/gcc/json1/checkstyle/diff/quiet", "enum": ["tty","gcc","json1","checkstyle","diff","quiet"]}
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_cppcheck_analyze() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "可选：待检查路径（相对工作区），默认 [\"src\"]，最多 24 项"},
            "enable": {"type": "string", "description": "可选：启用检查项，默认 all。可选 all/style/performance/portability/information/warning/unusedFunction/missingInclude"},
            "std": {"type": "string", "description": "可选：C/C++ 标准，如 c11、c++17、c++20"},
            "platform": {"type": "string", "description": "可选：平台 unix32/unix64/win32A/win32W/win64/native", "enum": ["unix32","unix64","win32A","win32W","win64","native"]}
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_semgrep_scan() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "可选：扫描路径（相对工作区），默认 [\".\"]，最多 24 项"},
            "config": {"type": "string", "description": "可选：规则集（如 auto、p/security-audit、p/owasp-top-ten、r/python.lang），默认 auto"},
            "severity": {"type": "string", "description": "可选：过滤严重级别（逗号分隔：ERROR,WARNING,INFO）"},
            "lang": {"type": "string", "description": "可选：限定语言（如 python、java、go）"},
            "json": {"type": "boolean", "description": "可选：是否以 JSON 格式输出，默认 false"}
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_hadolint_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "可选：Dockerfile 路径（相对工作区），默认 Dockerfile"},
            "format": {"type": "string", "description": "可选：输出格式 tty/json/checkstyle/codeclimate/gnu/codacy/sonarqube/sarif", "enum": ["tty","json","checkstyle","codeclimate","gitlab_codeclimate","gnu","codacy","sonarqube","sarif"]},
            "ignore": {"type": "array", "items": {"type": "string"}, "description": "可选：忽略规则列表（如 [\"DL3008\",\"DL3009\"]）"},
            "trusted_registries": {"type": "array", "items": {"type": "string"}, "description": "可选：受信任的 Docker registry 列表"}
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_bandit_scan() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "可选：待扫描路径（相对工作区），默认 [\".\"]，最多 24 项"},
            "severity": {"type": "string", "description": "可选：最低严重级别 low/medium/high"},
            "confidence": {"type": "string", "description": "可选：最低置信度 low/medium/high"},
            "skip": {"type": "string", "description": "可选：跳过的测试 ID（逗号分隔，如 B101,B102）"},
            "format": {"type": "string", "description": "可选：输出格式 txt/json/csv/xml/html/yaml/screen", "enum": ["txt","json","csv","xml","html","yaml","screen","custom"]}
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_lizard_complexity() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "paths": {"type": "array", "items": {"type": "string"}, "description": "可选：分析路径（相对工作区），默认 [\".\"]，最多 24 项"},
            "threshold": {"type": "integer", "description": "可选：圈复杂度阈值（仅显示超过此值的函数），1-200","minimum":1,"maximum":200},
            "language": {"type": "string", "description": "可选：限定语言（逗号分隔，如 python,cpp,java,javascript,rust,go）"},
            "sort": {"type": "string", "description": "可选：排序方式 cyclomatic_complexity/length/token_count/parameter_count/nloc", "enum": ["cyclomatic_complexity","length","token_count","parameter_count","nloc"]},
            "warnings_only": {"type": "boolean", "description": "可选：仅显示超过阈值的函数，默认 false"},
            "exclude": {"type": "array", "items": {"type": "string"}, "description": "可选：排除目录名列表（如 [\"vendor\",\"node_modules\"]）"}
        },
        "required": [],
        "additionalProperties": false
    })
}
