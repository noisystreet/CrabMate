[
ToolSpec {
            name: "pytest_run",
            description: "在工作区运行 `python3 -m pytest`（需已安装 pytest）。可选 test_path、keyword（-k）、markers（-m）、quiet、maxfail、nocapture。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::PytestRunArgs>,
            runner: ToolRunner::Legacy(runner_pytest_run),
            summary: ToolSummaryKind::Static("python3 -m pytest"),
        },
        ToolSpec {
            name: "python_snippet_run",
            description: "在工作区根目录写入临时 .py 并执行（结束后删除）。可 import 已安装的第三方包；默认 python3 + PYTHONPATH 含工作区根；可选 use_uv 用 uv 项目环境。等同任意代码执行，仅用于可信工作区。",
            category: ToolCategory::Development,
            parameters: schema_of::<args::PythonSnippetRunArgs>,
            runner: ToolRunner::Legacy(runner_python_snippet_run),
            summary: ToolSummaryKind::Dynamic(ts::summary_python_snippet_run),
        },
]
