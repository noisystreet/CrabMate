//! rust-analyzer 单次请求：参数解析、子进程启动、handshake 与 JSON-RPC（从 **`rust_ide.rs`** 拆出以降低单文件行数）。

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use crate::cm_tools::tool_result::ToolError;

use super::{
    DEFAULT_MAX_DOCUMENT_SYMBOLS, DEFAULT_RA_WAIT_MS, KillRaChild, LSP_IO_TIMEOUT,
    LspRustAnalyzerHandshake, MAX_DID_OPEN_BYTES, RaLspOp, lsp_io_error, lsp_rust_analyzer_handshake,
    lsp_rust_analyzer_run, path_to_file_uri, ra_lsp_request_method_params, read_response_until_id,
    resolve_rel_file, write_lsp,
};

#[allow(clippy::result_large_err)]
pub(super) fn lsp_rust_analyzer_request(
    args_json: &str,
    workspace_root: &Path,
    op: RaLspOp,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let inputs = parse_ra_lsp_json_inputs(&v, op)?;
    let RaWorkspaceSources {
        root,
        file_uri,
        root_uri,
        text,
    } = load_ra_workspace_sources(workspace_root, &inputs.path_rel)?;

    let mut guard = spawn_rust_analyzer_server(&inputs.server_path)?;
    let (mut stdin, stdout) = take_ra_child_io(&mut guard)?;
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + LSP_IO_TIMEOUT;

    let req_id = 2u64;
    let req = build_ra_request(op, &file_uri, &inputs, &v, req_id);
    let resp = perform_ra_roundtrip(
        &mut stdin,
        &mut reader,
        &LspRustAnalyzerHandshake {
            root_uri: &root_uri,
            root: &root,
            file_uri: &file_uri,
            text: &text,
            wait_ms: inputs.wait_ms,
            deadline,
        },
        &req,
        req_id,
    )?;
    drop(stdin);
    lsp_rust_analyzer_run(op, &resp, inputs.max_symbols)
}

/// 从已启动的子进程上取下 stdin/stdout 管道句柄。
///
/// 走到这里的 `None` 只可能是 `spawn_rust_analyzer_server` 没按 `Stdio::piped()` 装配，
/// 属内部不变量破坏（调用方无从修正），故归 `Internal` 而非 `invalid_args`。
#[allow(clippy::result_large_err)]
fn take_ra_child_io(
    guard: &mut KillRaChild,
) -> Result<(std::process::ChildStdin, std::process::ChildStdout), ToolError> {
    fn missing(what: &str) -> ToolError {
        ToolError::internal_code(
            "rust_analyzer_internal",
            format!("错误：rust-analyzer 子进程缺少 {what} 管道（内部缺陷，请报告）"),
        )
    }
    let c = guard.0.as_mut().ok_or_else(|| missing("子进程"))?;
    let stdin = c.stdin.take().ok_or_else(|| missing("stdin"))?;
    let stdout = c.stdout.take().ok_or_else(|| missing("stdout"))?;
    Ok((stdin, stdout))
}

/// 构造 LSP 请求 JSON（`jsonrpc`/`id`/`method`/`params`）。
fn build_ra_request(
    op: RaLspOp,
    file_uri: &str,
    inputs: &ParsedRaLspInputs,
    v: &Value,
    req_id: u64,
) -> Value {
    let (method, params) =
        ra_lsp_request_method_params(op, file_uri, inputs.line, inputs.character, v);
    json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params
    })
}

/// handshake → 写请求 → 读响应（`deadline` 内）。
#[allow(clippy::result_large_err)]
fn perform_ra_roundtrip(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    handshake: &LspRustAnalyzerHandshake<'_>,
    req: &Value,
    req_id: u64,
) -> Result<Value, ToolError> {
    lsp_rust_analyzer_handshake(stdin, reader, handshake)?;
    write_lsp(stdin, &req.to_string()).map_err(|e| lsp_io_error("写 LSP 请求", e))?;
    read_response_until_id(reader, req_id, handshake.deadline)
}

struct ParsedRaLspInputs {
    path_rel: String,
    line: u32,
    character: u32,
    max_symbols: usize,
    server_path: String,
    wait_ms: u64,
}

struct RaWorkspaceSources {
    root: PathBuf,
    file_uri: String,
    root_uri: String,
    text: String,
}

#[allow(clippy::result_large_err)]
fn parse_ra_lsp_json_inputs(v: &Value, op: RaLspOp) -> Result<ParsedRaLspInputs, ToolError> {
    let path_rel = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolError::invalid_args("错误：缺少 path".to_string()))?;
    let (line, character) = if matches!(op, RaLspOp::DocumentSymbol | RaLspOp::WorkspaceSymbol) {
        (0u32, 0u32)
    } else {
        let line = v
            .get("line")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| ToolError::invalid_args("错误：缺少 line（0-based）".to_string()))? as u32;
        let character = v.get("character").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        (line, character)
    };
    let max_symbols = match op {
        RaLspOp::DocumentSymbol => v
            .get("max_symbols")
            .and_then(|x| x.as_u64())
            .unwrap_or(DEFAULT_MAX_DOCUMENT_SYMBOLS)
            .clamp(1, 5000) as usize,
        RaLspOp::WorkspaceSymbol => {
            if v.get("query")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(ToolError::invalid_args(
                    "错误：缺少 query（workspace/symbol 搜索串）".to_string(),
                ));
            }
            v.get("max_results")
                .and_then(|x| x.as_u64())
                .unwrap_or(64)
                .clamp(1, 500) as usize
        }
        _ => 0usize,
    };
    let server_path = v
        .get("server_path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("rust-analyzer")
        .to_string();
    let wait_ms = v
        .get("wait_after_open_ms")
        .and_then(|x| x.as_u64())
        .unwrap_or(DEFAULT_RA_WAIT_MS)
        .min(5000);

    Ok(ParsedRaLspInputs {
        path_rel: path_rel.to_string(),
        line,
        character,
        max_symbols,
        server_path,
        wait_ms,
    })
}

/// 解析工作区根、目标文件 URI 与文件正文（`didOpen` 需要全文）。
///
/// 错误码划分：工作区根自身不可解析归 `rust_analyzer_workspace_resolve_failed`（`Workspace`，
/// 环境问题，改 `args` 无用）；读文件失败归 `rust_analyzer_read_failed`；文件过大是调用方
/// 可以换小文件的参数问题，归 `invalid_args`。
#[allow(clippy::result_large_err)]
fn load_ra_workspace_sources(
    workspace_root: &Path,
    path_rel: &str,
) -> Result<RaWorkspaceSources, ToolError> {
    let root = workspace_root.canonicalize().map_err(|e| {
        ToolError::workspace(
            "rust_analyzer_workspace_resolve_failed",
            format!("错误：工作区路径无法解析: {e}"),
        )
    })?;
    let file_path = resolve_rel_file(&root, path_rel)?;
    let uri = path_to_file_uri(&file_path)?;
    let text = fs::read_to_string(&file_path).map_err(|e| {
        ToolError::external_code(
            "rust_analyzer_read_failed",
            format!("错误：读取源文件失败: {e}"),
        )
    })?;
    if text.len() > MAX_DID_OPEN_BYTES {
        return Err(ToolError::invalid_args(format!(
            "错误：文件过大 (>{MAX_DID_OPEN_BYTES})，请用 read_file 分段或缩小文件"
        )));
    }

    let root_uri = path_to_file_uri(&root)?;
    Ok(RaWorkspaceSources {
        root,
        file_uri: uri,
        root_uri,
        text,
    })
}

/// 启动 `rust-analyzer` 子进程（stdio）。
///
/// 找不到可执行文件、权限不足都只能归为「起不来」：工具层分辨不出是哪一种，
/// 归 `rust_analyzer_spawn_failed`（`External`）；正文保留安装提示便于模型自纠。
#[allow(clippy::result_large_err)]
fn spawn_rust_analyzer_server(server_path: &str) -> Result<KillRaChild, ToolError> {
    let child = Command::new(server_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            ToolError::external_code(
                "rust_analyzer_spawn_failed",
                format!(
                    "错误：无法启动 rust-analyzer（{server_path}）：{e}。请安装 rust-analyzer 或设置 server_path。"
                ),
            )
        })?;
    Ok(KillRaChild(Some(child)))
}
