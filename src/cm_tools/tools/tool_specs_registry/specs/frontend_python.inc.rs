[
        builtin_tool! {
            name: tool_names::PYTEST_RUN,
            args: args::PytestRunArgs,
            category: Development,
            tags: [dev_tag::GENERAL, dev_tag::PYTHON],
            runner: runner_pytest_run_try,
            summary: static "python3 -m pytest",
            description: "在工作区运行 `python3 -m pytest`（需已安装 pytest）。可选 test_path、keyword（-k）、markers（-m）、quiet、maxfail、nocapture。",
        },
        ToolSpec {
            name: tool_names::PYTHON_SNIPPET_RUN,
            description: "在工作区根目录写入临时 .py 并执行（结束后删除）。可 import 已安装的第三方包；默认 python3 + PYTHONPATH 含工作区根；可选 use_uv 用 uv 项目环境。等同任意代码执行，仅用于可信工作区。",
            category: ToolCategory::Development,
            tags: &[dev_tag::GENERAL, dev_tag::PYTHON],
            parameters: schema_of::<args::PythonSnippetRunArgs>,
            runner: ToolRunner::Typed(runner_python_snippet_run_try),
            summary: ToolSummaryKind::Dynamic(ts::summary_python_snippet_run),
        },
]
