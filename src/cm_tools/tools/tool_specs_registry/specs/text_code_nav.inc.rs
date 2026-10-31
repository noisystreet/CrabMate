[
        builtin_tool! {
            name: tool_names::TEXT_DIFF,
            args: args::TextDiffArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_text_diff_try,
            summary: none,
            description: "任意两段 **UTF-8 纯文本**的行级 unified diff（与 Git 无关）。mode=inline 时比较 left/right 字符串（各 256KiB）；mode=paths 时比较工作区内两文件（各 4MiB 内）。可调 context_lines 与 max_output_bytes。与 `structured_diff`（结构化键）互补。",
        },
        builtin_tool! {
            name: tool_names::TABLE_TEXT,
            args: args::TableTextArgs,
            category: Development,
            tags: [dev_tag::GENERAL],
            runner: runner_table_text_try,
            summary: none,
            description: "工作区内 **CSV / TSV / 简单分隔** 纯文本：`preview` 抽样预览，`validate` 检查列数是否一致，`select_columns` 按列下标导出 TSV，`filter_rows` 按列 equals/contains 筛选，`aggregate` 对列做 sum/mean/min/max/count。单文件 4MiB；内联 `text` 256KiB；扫描/输出行数有上限。",
        },
        builtin_tool! {
            name: tool_names::CODE_NAV,
            args: args::CodeNavArgs,
            category: Development,
            tags: [dev_tag::GENERAL, dev_tag::RUST],
            runner: runner_code_nav_try,
            summary: dynamic,
            description: "在 Rust 源码中导航。**`action` 必填**：`find_symbol`（递归定位符号定义位置，可 `kind` 过滤与 `context_lines` 上下文）、`find_references`（按词边界搜标识符引用，默认跳过疑似定义行）、`file_outline`（单个 `.rs` 文件的顶层结构大纲，需 `path`）、`call_graph_sketch`（改 API 前的**启发式**影响面草图：扫描 `use` + 非定义行词边界命中并按目录聚合；**非** rustc 真调用图）。`find_symbol` / `find_references` / `call_graph_sketch` 需 `symbol`（后者也可用 `symbols` 批量），`file_outline` 需 `path`；与 `action` 无关的字段被忽略，但 schema 的取值区间（如 `max_results` 的 1–300）对每次调用都校验。需要 LSP 级精度请用 `rust_analyzer`。",
        },
]
