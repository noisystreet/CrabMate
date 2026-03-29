//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_get_current_time() -> serde_json::Value {
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

pub(in crate::tools) fn params_calc() -> serde_json::Value {
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

pub(in crate::tools) fn params_convert_units() -> serde_json::Value {
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

pub(in crate::tools) fn params_weather() -> serde_json::Value {
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

pub(in crate::tools) fn params_web_search() -> serde_json::Value {
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

pub(in crate::tools) fn params_http_fetch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "完整 http(s) URL。Web 仅允许匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）；CLI 未匹配时可终端审批（与 run_command 相同）。"
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

pub(in crate::tools) fn params_http_request() -> serde_json::Value {
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

pub(in crate::tools) fn params_text_transform() -> serde_json::Value {
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

pub(in crate::tools) fn params_regex_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {"type": "string", "description": "正则表达式"},
            "test_strings": {"type": "array", "items": {"type": "string"}, "description": "待测试字符串数组（上限100条）"}
        },
        "required": ["pattern", "test_strings"]
    })
}

pub(in crate::tools) fn params_date_calc() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "mode": {"type": "string", "enum": ["diff", "offset"], "description": "diff=两日期间隔，offset=基准+偏移（默认offset）"},
            "from": {"type": "string", "description": "diff模式：起始日期 YYYY-MM-DD"},
            "to": {"type": "string", "description": "diff模式：结束日期 YYYY-MM-DD"},
            "base": {"type": "string", "description": "offset模式：基准日期 YYYY-MM-DD（默认今天）"},
            "offset": {"type": "string", "description": "offset模式：偏移量（如 +30d, -2w, +1m）"}
        },
        "required": []
    })
}

pub(in crate::tools) fn params_json_format() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "text": {"type": "string", "description": "JSON 或 YAML 文本（上限512KiB）"},
            "mode": {"type": "string", "enum": ["pretty", "compact", "yaml_to_json", "json_to_yaml"], "description": "模式（默认 pretty）"}
        },
        "required": ["text"]
    })
}

pub(in crate::tools) fn params_env_var_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "names": {"type": "array", "items": {"type": "string"}, "description": "环境变量名列表（上限50个）"},
            "show_length": {"type": "boolean", "description": "是否显示值长度（默认false）"},
            "show_prefix_chars": {"type": "integer", "description": "显示值的前N个字符（0=不显示，上限8）"}
        },
        "required": ["names"]
    })
}
