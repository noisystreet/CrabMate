[
        ToolSpec {
            name: "self_config_info",
            description: "只读查询**运行时合并后的自身配置**（环境变量覆盖、热重载、Web 密钥注入等均已生效）：LLM 网关（model/api_base/auth_mode/planner/executor）、采样（max_tokens/temperature/context_tokens/seed）、供应商开关、HTTP 重试、命令执行（超时/输出上限/白名单条数）、天气、联网搜索（provider/api_key 是否已设置）、受控 HTTP、规划策略、角色（默认人格/会话模式/角色数）、工作区根（允许根/池/当前会话）、Web API（Bearer 是否已设置/是否必需/CORS）、上下文管线、回合预算、长期记忆、会话库路径、MCP、工具注册表策略、沙盒。**密钥类字段一律只报是否已设置，不输出值**（web_search_api_key、web_api_bearer_token 等）。可选 `sections` 数组限定小节：llm、sampling、vendor_flags、http_retry、command_exec、weather、web_search、http_fetch、per_plan_policy、roles、workspace、web_api、context_pipeline、turn_budget、long_term_memory、conversation、mcp、tool_registry、sandbox；缺省输出全部。回答用户关于自身配置的问题（用什么模型、api_base、超时、工作区等）时调用本工具，不要凭记忆编造。",
            category: ToolCategory::Development,
            parameters: tool_params::params_self_config_info,
            runner: runner_self_config_info,
            summary: ToolSummaryKind::Static("Runtime self-config summary (redacted)"),
        },
]
