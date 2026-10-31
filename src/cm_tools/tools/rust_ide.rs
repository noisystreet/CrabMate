//! 可选的 **rust-analyzer** LSP（stdio）请求封装。
//!
//! rust-analyzer 需已安装并在 PATH 中（或由 `server_path` 指定）。LSP 为最小实现：initialize → didOpen → 单次请求（definition / references / hover / documentSymbol / implementation / typeDefinition / documentHighlight / workspace/symbol）。

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use crate::cm_tools::tool_result::ToolError;

struct KillRaChild(Option<Child>);
impl Drop for KillRaChild {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

const MAX_DID_OPEN_BYTES: usize = 512 * 1024;
const DEFAULT_RA_WAIT_MS: u64 = 500;
const DEFAULT_MAX_DOCUMENT_SYMBOLS: u64 = 500;
const LSP_IO_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RaLspOp {
    Definition,
    References,
    Hover,
    DocumentSymbol,
    Implementation,
    TypeDefinition,
    DocumentHighlight,
    WorkspaceSymbol,
}

#[path = "rust_ide_lsp_response_format.rs"]
mod lsp_response_format;

#[path = "rust_ide_lsp_request.rs"]
mod lsp_request;

// ---------- rust-analyzer LSP（stdio）----------

/// `rust_analyzer` 的 action 可选值（错误提示与 `description` 共用一份）。
const RA_ACTIONS: &str = "goto_definition / find_references / hover / document_symbol / \
goto_implementation / goto_type_definition / document_highlight / workspace_symbol";

/// 统一入口：按入参 `action` 分发到对应 LSP 请求。
///
/// 合并自原 8 个 `rust_analyzer_*` 工具——它们本来都只是
/// `lsp_rust_analyzer_request(.., RaLspOp::X)` 的薄包装，合并只改了「从哪里拿到 `RaLspOp`」。
/// 参数校验仍留在 [`lsp_rust_analyzer_request`] 内（`path` 必填、position 类另需 0-based `line`），
/// 因此缺参正文与逐工具时代保持一致。
///
/// Typed 迁移：失败一律以 [`ToolError`] 返回，成功正文保持不变，
/// 判定改由 `e.code` 承载。此前「路径无法解析: …」与「… : LSP error: …」两类正文既不含
/// 「错误」也不含「失败」，`parse_tool_output` 会把它们判成 `ok: true`——模型拿到空结果，
/// 还以为「该位置确实没有信息」。错误码划分见 [`resolve_rel_file`] 与 [`lsp_io_error`]。
#[allow(clippy::result_large_err)]
pub fn rust_analyzer_try(args_json: &str, workspace_root: &Path) -> Result<String, ToolError> {
    let op = ra_lsp_op_from_json(args_json)?;
    lsp_rust_analyzer_request(args_json, workspace_root, op)
}

/// 从入参 JSON 的 `action` 解析出 [`RaLspOp`]；缺失与未知都给出可选值清单。
#[allow(clippy::result_large_err)]
fn ra_lsp_op_from_json(args_json: &str) -> Result<RaLspOp, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let action = match v.get("action").and_then(|x| x.as_str()).map(str::trim) {
        Some(a) if !a.is_empty() => a,
        _ => {
            return Err(ToolError::invalid_args(format!(
                "错误：缺少 action（可选值：{RA_ACTIONS}）"
            )));
        }
    };
    match action {
        "goto_definition" => Ok(RaLspOp::Definition),
        "find_references" => Ok(RaLspOp::References),
        "hover" => Ok(RaLspOp::Hover),
        "document_symbol" => Ok(RaLspOp::DocumentSymbol),
        "goto_implementation" => Ok(RaLspOp::Implementation),
        "goto_type_definition" => Ok(RaLspOp::TypeDefinition),
        "document_highlight" => Ok(RaLspOp::DocumentHighlight),
        "workspace_symbol" => Ok(RaLspOp::WorkspaceSymbol),
        other => Err(ToolError::invalid_args(format!(
            "错误：未知 action `{other}`（可选值：{RA_ACTIONS}）"
        ))),
    }
}

/// LSP 读写失败的统一出口：`rust-analyzer` 子进程侧的 IO / 协议问题（`External`）。
///
/// 与参数错误分开：调用方无法通过改 `args` 修正，重试通常也无意义。
#[allow(clippy::result_large_err)]
fn lsp_io_error(what: &str, e: impl std::fmt::Display) -> ToolError {
    ToolError::external_code("rust_analyzer_lsp_failed", format!("错误：{what}失败：{e}"))
}

fn ra_lsp_request_method_params(
    op: RaLspOp,
    uri: &str,
    line: u32,
    character: u32,
    args: &Value,
) -> (&'static str, Value) {
    match op {
        RaLspOp::Definition => (
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        ),
        RaLspOp::References => (
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": args.get("include_declaration").and_then(|x| x.as_bool()).unwrap_or(true) }
            }),
        ),
        RaLspOp::Hover => (
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        ),
        RaLspOp::DocumentSymbol => (
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        ),
        RaLspOp::Implementation => (
            "textDocument/implementation",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        ),
        RaLspOp::TypeDefinition => (
            "textDocument/typeDefinition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        ),
        RaLspOp::DocumentHighlight => (
            "textDocument/documentHighlight",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        ),
        RaLspOp::WorkspaceSymbol => {
            let q = args
                .get("query")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            ("workspace/symbol", json!({ "query": q }))
        }
    }
}

struct LspRustAnalyzerHandshake<'a> {
    root_uri: &'a str,
    root: &'a Path,
    file_uri: &'a str,
    text: &'a str,
    wait_ms: u64,
    deadline: Instant,
}

#[allow(clippy::result_large_err)]
fn lsp_rust_analyzer_handshake(
    stdin: &mut impl Write,
    reader: &mut BufReader<impl std::io::Read>,
    h: &LspRustAnalyzerHandshake<'_>,
) -> Result<(), ToolError> {
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1u64,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootPath": h.root.to_string_lossy(),
            "rootUri": h.root_uri,
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {},
                    "definition": { "linkSupport": true },
                    "references": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    "implementation": { "linkSupport": true },
                    "typeDefinition": { "linkSupport": true },
                    "documentHighlight": {}
                },
                "workspace": { "symbol": { "dynamicRegistration": false } }
            },
            "workspaceFolders": [{
                "uri": h.root_uri,
                "name": "workspace"
            }]
        }
    });
    write_lsp(stdin, &init.to_string()).map_err(|e| lsp_io_error("写 initialize 请求", e))?;
    let _ = read_response_until_id(reader, 1, h.deadline)?;

    let notif_init = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    write_lsp(stdin, &notif_init.to_string()).map_err(|e| lsp_io_error("写 initialized 通知", e))?;

    let did_open = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": h.file_uri,
                "languageId": "rust",
                "version": 1,
                "text": h.text
            }
        }
    });
    write_lsp(stdin, &did_open.to_string()).map_err(|e| lsp_io_error("写 didOpen 通知", e))?;

    std::thread::sleep(Duration::from_millis(h.wait_ms));
    Ok(())
}

#[allow(clippy::result_large_err)]
fn lsp_rust_analyzer_run(
    op: RaLspOp,
    resp: &Value,
    max_symbols: usize,
) -> Result<String, ToolError> {
    match op {
        RaLspOp::Definition => {
            lsp_response_format::format_lsp_locations(resp, "rust_analyzer goto definition")
        }
        RaLspOp::References => {
            lsp_response_format::format_lsp_locations(resp, "rust_analyzer find references")
        }
        RaLspOp::Hover => lsp_response_format::format_lsp_hover(resp),
        RaLspOp::DocumentSymbol => {
            lsp_response_format::format_lsp_document_symbols(resp, max_symbols)
        }
        RaLspOp::Implementation => {
            lsp_response_format::format_lsp_locations(resp, "rust_analyzer goto implementation")
        }
        RaLspOp::TypeDefinition => {
            lsp_response_format::format_lsp_locations(resp, "rust_analyzer goto type definition")
        }
        RaLspOp::DocumentHighlight => lsp_response_format::format_lsp_document_highlights(resp),
        RaLspOp::WorkspaceSymbol => {
            lsp_response_format::format_workspace_symbols(resp, max_symbols)
        }
    }
}

#[allow(clippy::result_large_err)]
fn lsp_rust_analyzer_request(
    args_json: &str,
    workspace_root: &Path,
    op: RaLspOp,
) -> Result<String, ToolError> {
    lsp_request::lsp_rust_analyzer_request(args_json, workspace_root, op)
}

/// 把入参 `path` 解析为工作区内的真实文件路径。
///
/// 错误码划分：绝对路径 / `..` / 越界 / 非文件都是**调用方可直接修正**的参数问题，
/// 归 `invalid_args`；`canonicalize` 失败（绝大多数是文件不存在）归
/// `rust_analyzer_file_not_found`。后者正是本次迁移修掉的静默成功：原正文
/// 「路径无法解析: No such file or directory」不含「错误 / 失败」，被判 `ok: true`。
#[allow(clippy::result_large_err)]
fn resolve_rel_file(root: &Path, rel: &str) -> Result<PathBuf, ToolError> {
    if Path::new(rel).is_absolute() || rel.contains("..") {
        return Err(ToolError::invalid_args(
            "错误：path 必须为工作区内相对路径且不含 ..".to_string(),
        ));
    }
    let joined = root.join(rel);
    let canon = joined.canonicalize().map_err(|e| {
        ToolError::external_code(
            "rust_analyzer_file_not_found",
            format!("错误：路径无法解析: {e}"),
        )
    })?;
    if !canon.starts_with(root) {
        return Err(ToolError::invalid_args(
            "错误：路径超出工作区".to_string(),
        ));
    }
    if !canon.is_file() {
        return Err(ToolError::invalid_args(
            "错误：不是已存在的文件".to_string(),
        ));
    }
    Ok(canon)
}

#[allow(clippy::result_large_err)]
fn path_to_file_uri(p: &Path) -> Result<String, ToolError> {
    let canon = p.canonicalize().map_err(|e| {
        ToolError::external_code(
            "rust_analyzer_file_not_found",
            format!("错误：路径无法解析: {e}"),
        )
    })?;
    let raw = canon.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        let t = raw.trim_start_matches(r"\\?\").replace('\\', "/");
        Ok(format!("file:///{}", t))
    }
    #[cfg(not(windows))]
    {
        Ok(format!("file://{}", raw))
    }
}

fn write_lsp(w: &mut impl Write, body: &str) -> std::io::Result<()> {
    let b = body.as_bytes();
    write!(w, "Content-Length: {}\r\n\r\n", b.len())?;
    w.write_all(b)?;
    w.flush()?;
    Ok(())
}

#[allow(clippy::result_large_err)]
fn read_response_until_id<R: BufRead>(
    reader: &mut R,
    expected_id: u64,
    deadline: Instant,
) -> Result<Value, ToolError> {
    loop {
        if Instant::now() > deadline {
            // 复用既有 `timeout` 码（分类 `Timeout`），与 `run_command` 等口径一致。
            return Err(ToolError::with_code(
                "timeout",
                "错误：LSP 读取超时".to_string(),
            ));
        }
        let buf = read_one_lsp_message(reader)?;
        let v: Value =
            serde_json::from_slice(&buf).map_err(|e| lsp_io_error("解析 LSP 响应", e))?;
        if v.get("id").and_then(|x| x.as_u64()) == Some(expected_id) {
            return Ok(v);
        }
    }
}

#[allow(clippy::result_large_err)]
fn read_one_lsp_message<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, ToolError> {
    let mut content_len: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        reader
            .read_line(&mut line)
            .map_err(|e| lsp_io_error("读 LSP 头", e))?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = Some(
                rest.trim()
                    .parse::<usize>()
                    .map_err(|_| lsp_io_error("解析 Content-Length", "非整数值"))?,
            );
        }
    }
    let len = content_len.ok_or_else(|| lsp_io_error("读 LSP 头", "缺少 Content-Length"))?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|e| lsp_io_error("读 LSP 体", e))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 个 `action` 都能解析到对应 LSP 操作（合并前它们是 8 个独立工具）。
    #[test]
    fn ra_action_maps_to_every_lsp_op() {
        let cases = [
            ("goto_definition", RaLspOp::Definition),
            ("find_references", RaLspOp::References),
            ("hover", RaLspOp::Hover),
            ("document_symbol", RaLspOp::DocumentSymbol),
            ("goto_implementation", RaLspOp::Implementation),
            ("goto_type_definition", RaLspOp::TypeDefinition),
            ("document_highlight", RaLspOp::DocumentHighlight),
            ("workspace_symbol", RaLspOp::WorkspaceSymbol),
        ];
        for (action, want) in cases {
            let args = format!(r#"{{"action":"{action}","path":"src/lib.rs","line":3}}"#);
            let got = ra_lsp_op_from_json(&args).expect("8 个 action 都应能解析");
            assert_eq!(got, want, "action={action} 应映射到 {want:?}");
        }
    }

    /// 缺 `action` / 未知 `action` 都把可选值清单写进错误，便于模型自纠。
    #[test]
    fn ra_missing_or_unknown_action_lists_choices() {
        for args in [
            r#"{"path":"src/lib.rs","line":1}"#,
            r#"{"action":"","path":"src/lib.rs"}"#,
            r#"{"action":"   ","path":"src/lib.rs"}"#,
        ] {
            let err = ra_lsp_op_from_json(args).expect_err("缺 action 须失败");
            assert_eq!(err.code, "invalid_args", "缺 action 属参数问题");
            assert!(err.message.contains("缺少 action"), "{}", err.message);
            assert!(
                err.message.contains("workspace_symbol"),
                "应列出可选值: {}",
                err.message
            );
        }
        let err = ra_lsp_op_from_json(r#"{"action":"nope","path":"src/lib.rs"}"#)
            .expect_err("未知 action 须失败");
        assert_eq!(err.code, "invalid_args", "未知 action 属参数问题");
        assert!(err.message.contains("未知 action `nope`"), "{}", err.message);
        assert!(
            err.message.contains("goto_definition"),
            "应列出可选值: {}",
            err.message
        );
    }

    #[test]
    fn ra_invalid_json_is_reported() {
        let err = ra_lsp_op_from_json("{").expect_err("非法 JSON 须失败");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.starts_with("参数 JSON 无效"), "{}", err.message);
    }

    #[test]
    fn format_lsp_hover_smoke() {
        let resp = json!({
            "result": {
                "contents": { "kind": "markdown", "value": "`x`: i32" }
            }
        });
        let s = lsp_response_format::format_lsp_hover(&resp).unwrap();
        assert!(s.contains("markdown"));
        assert!(s.contains("`x`"));
    }

    #[test]
    fn format_lsp_document_symbol_tree_smoke() {
        let resp = json!({
            "result": [{
                "name": "foo",
                "kind": 12,
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 3 }
                },
                "selectionRange": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 3 }
                },
                "children": []
            }]
        });
        let s = lsp_response_format::format_lsp_document_symbols(&resp, 10).unwrap();
        assert!(s.contains("foo"));
        assert!(s.contains("Function"));
    }

    #[test]
    fn format_lsp_document_highlight_smoke() {
        let resp = json!({
            "result": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 3 }
                },
                "kind": 2
            }]
        });
        let s = lsp_response_format::format_lsp_document_highlights(&resp).unwrap();
        assert!(s.contains("Read"));
        assert!(s.contains("1:1"));
    }

    #[test]
    fn format_workspace_symbols_smoke() {
        let resp = json!({
            "result": [{
                "name": "foo",
                "kind": 12,
                "location": {
                    "uri": "file:///tmp/a.rs",
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 3 }
                    }
                }
            }]
        });
        let s = lsp_response_format::format_workspace_symbols(&resp, 10).unwrap();
        assert!(s.contains("foo"));
        assert!(s.contains("Function"));
    }

    /// 8 个 `action` 在「目标文件不存在」时都必须显式失败。
    ///
    /// 这是迁移前最危险的一处静默成功：正文「路径无法解析: No such file or directory」
    /// 既不以「错误」开头、也不含「失败 / 超时」，`parse_tool_output` 判 `ok: true`，
    /// 模型于是把「文件根本不存在」读成「这个位置没有信息」。
    #[test]
    fn ra_every_action_fails_on_missing_file() {
        let root = std::env::temp_dir();
        let mut cases: Vec<(String, &str)> = [
            "goto_definition",
            "find_references",
            "hover",
            "goto_implementation",
            "goto_type_definition",
            "document_highlight",
        ]
        .iter()
        .map(|action| {
            (
                format!(r#"{{"action":"{action}","path":"no_such_file.rs","line":0}}"#),
                *action,
            )
        })
        .collect();
        cases.push((
            r#"{"action":"document_symbol","path":"no_such_file.rs"}"#.to_string(),
            "document_symbol",
        ));
        cases.push((
            r#"{"action":"workspace_symbol","path":"no_such_file.rs","query":"x"}"#.to_string(),
            "workspace_symbol",
        ));

        for (args, action) in cases {
            let err = rust_analyzer_try(&args, &root)
                .expect_err("目标文件不存在时必须失败（原先是静默成功）");
            assert_eq!(
                err.code, "rust_analyzer_file_not_found",
                "action={action} 错误码不对：{}",
                err.message
            );
            assert!(
                err.message.starts_with("错误："),
                "action={action} 正文需自解释：{}",
                err.message
            );
        }
    }

    /// LSP 响应自带 `error` 字段时必须失败。
    ///
    /// 原正文「rust_analyzer hover: LSP error: …」同样既不含「错误」也不含「失败」，
    /// 被判 `ok: true` —— 与「文件不存在」是同一故障模式，只是触发点不同。
    #[test]
    fn ra_lsp_error_response_is_a_failure() {
        let resp = json!({ "error": { "code": -32601, "message": "method not found" } });
        let errs = [
            lsp_response_format::format_lsp_hover(&resp).expect_err("hover 应失败"),
            lsp_response_format::format_lsp_document_symbols(&resp, 10)
                .expect_err("document_symbol 应失败"),
            lsp_response_format::format_workspace_symbols(&resp, 10)
                .expect_err("workspace_symbol 应失败"),
            lsp_response_format::format_lsp_document_highlights(&resp)
                .expect_err("document_highlight 应失败"),
            lsp_response_format::format_lsp_locations(&resp, "rust_analyzer goto definition")
                .expect_err("goto_definition 应失败"),
        ];
        for err in errs {
            assert_eq!(err.code, "rust_analyzer_lsp_failed");
            assert!(err.message.starts_with("错误："), "{}", err.message);
            assert!(err.message.contains("LSP error"), "{}", err.message);
        }
    }

    /// 绝对路径与 `..` 是调用方能直接修正的参数问题，归 `invalid_args`。
    #[test]
    fn ra_rejects_absolute_and_parent_paths() {
        let root = std::env::temp_dir();
        for path in ["/etc/passwd", "../outside.rs"] {
            let args = format!(r#"{{"action":"hover","path":"{path}","line":0}}"#);
            let err = rust_analyzer_try(&args, &root).expect_err("非法路径须失败");
            assert_eq!(err.code, "invalid_args", "path={path}：{}", err.message);
        }
    }

    /// 工作区根自身不可解析是环境问题（`Workspace`），不是调用方的参数错误。
    #[test]
    fn ra_unresolvable_workspace_is_a_workspace_error() {
        let missing = std::env::temp_dir().join("crabmate-ra-no-such-workspace-dir");
        let err = rust_analyzer_try(r#"{"action":"hover","path":"x.rs","line":0}"#, &missing)
            .expect_err("工作区不可解析须失败");
        assert_eq!(err.code, "rust_analyzer_workspace_resolve_failed");
        assert_eq!(
            err.category,
            crate::cm_tools::tool_result::ToolFailureCategory::Workspace
        );
    }
}
