//! 受控 HTTP：`http_fetch`（GET/HEAD）与 `http_request`（POST/PUT/PATCH/DELETE + 可选 JSON body）。**CLI / Web 流式**下 URL 未匹配 `http_fetch_allowed_prefixes` 时走与 `run_command` 同类的审批（**`tool_approval`**：SSE + **`cli_terminal`**）；**`--yes`** 亦跳过 CLI 提示。`workflow_execute` 等 **`run_tool` 同步路径**仍仅白名单前缀。
//!
//! 响应正文解码：`Content-Type` 的 **`charset`**、HTML **`<meta charset>`** / **http-equiv**、**BOM**，否则 **`chardetng`** 嗅探；与异步 LLM 客户端一致使用 **`crabmate/<版本>`** `User-Agent`。
//! 可选 **`text_format: html_text`**：用 **`scraper`（html5ever）** 将 HTML 转为可读纯文本（跳过 `script`/`style`/`noscript`，优先抽取 `main` / `article` / `[role=main]`，否则 `body`）。

#![allow(unused_imports)] // `pub use` 仅用于对外再导出，本模块正文不直接引用这些符号。

mod args;
mod decode;
mod policy;
mod sync_fetch;

pub use args::{
    ABS_MAX_BODY_BYTES, FetchMethod, HttpBodyTextFormat, RequestMethod, parse_http_fetch_args,
    parse_http_request_args,
};
pub use args::{HttpFetchArgs, HttpRequestArgs};
pub use decode::html_to_readable_text;
pub use policy::{
    approval_args_display, approval_args_display_request, display_redacted, request_storage_key,
    storage_key, url_matches_allowed_prefixes,
};
pub use sync_fetch::{fetch_with_method, request_with_json_body, run_direct, run_request_direct};

#[cfg(test)]
mod tests;
