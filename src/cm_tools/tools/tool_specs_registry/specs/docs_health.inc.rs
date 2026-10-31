[
        builtin_tool! {
            name: tool_names::DOCS_HEALTH_SWEEP,
            args: args::DocsHealthSweepArgs,
            category: Development,
            tags: [dev_tag::GENERAL, dev_tag::QUALITY],
            runner: runner_docs_health_sweep_try,
            summary: static "docs spell + md link sweep",
            description: "只读聚合：主文档头预览（默认 README/AGENTS/docs 等，与 repo_overview_sweep 列表一致）+ 拼写检查（typos / codespell）+ `markdown_check_links`。**Markdown 外链**：仅当 `md_allowed_external_prefixes` 非空时由 `markdown_check_links` 内置 HTTP 发 HEAD，**不经过** `http_fetch`/`http_request` 与 `http_fetch_allowed_prefixes`，**无** Web SSE 审批；默认不填前缀则外链仅计数、不联网。缺 typos/codespell CLI 时对应步记为 skipped。",
        },
]
