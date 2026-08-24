//! 受控 HTTP：`http_fetch`（GET/HEAD）与 `http_request`（POST/PUT/PATCH/DELETE + 可选 JSON body）。
//! `http_fetch_allowed_prefixes` 含 **`"*"`** 时任意 http/https 直接执行（嵌入默认）；否则未匹配前缀时 **Web 流式**走 SSE 审批（**`tool_approval`**）。`workflow_execute` 等 **`run_tool` 同步路径**仍仅当前缀匹配（含 `"*"`）才请求。
//!
//! 响应正文解码：`Content-Type` 的 **`charset`**、HTML **`<meta charset>`** / **http-equiv**、**BOM**，否则 **`chardetng`** 嗅探。
//! 请求对齐 curl：自动发送 **`Accept: */*`** 与 **`Accept-Encoding`**（gzip/brotli/deflate，自动解压），**`User-Agent`** 默认 **`crabmate/<版本>`**、可经 `http_fetch_user_agent` / `CM_HTTP_FETCH_USER_AGENT` 覆盖。
//! 环境代理：遵循 **`ALL_PROXY` / `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`**（HTTP(S) 代理；reqwest 0.13 不支持 **SOCKS**，遇到 socks 代理会在错误中提示 unset 或改用 http 端口）。
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
    prefixes_include_allow_any, storage_key, url_matches_allowed_prefixes,
};
pub use sync_fetch::{
    default_user_agent, fetch_with_method, request_with_json_body, run_direct, run_request_direct,
};

#[cfg(test)]
mod tests;
