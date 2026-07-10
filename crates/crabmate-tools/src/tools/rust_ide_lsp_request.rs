//! rust-analyzer 单次请求：参数解析、子进程启动、handshake 与 JSON-RPC（从 **`rust_ide.rs`** 拆出以降低单文件行数）。

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use super::{
    DEFAULT_MAX_DOCUMENT_SYMBOLS, DEFAULT_RA_WAIT_MS, KillRaChild, LSP_IO_TIMEOUT,
    LspRustAnalyzerHandshake, MAX_DID_OPEN_BYTES, RaLspOp, lsp_rust_analyzer_handshake,
    lsp_rust_analyzer_run, path_to_file_uri, ra_lsp_request_method_params, read_response_until_id,
    resolve_rel_file, write_lsp,
};

pub(super) fn lsp_rust_analyzer_request(
    args_json: &str,
    workspace_root: &Path,
    op: RaLspOp,
) -> Result<String, String> {
    let v = crate::tools::parse_args_json(args_json)?;
    let inputs = parse_ra_lsp_json_inputs(&v, op)?;
    let RaWorkspaceSources {
        root,
        file_uri,
        root_uri,
        text,
    } = load_ra_workspace_sources(workspace_root, &inputs.path_rel)?;

    let mut guard = spawn_rust_analyzer_server(&inputs.server_path)?;
    let c = guard.0.as_mut().ok_or("internal")?;

    let mut stdin = c.stdin.take().ok_or("stdin")?;
    let stdout = c.stdout.take().ok_or("stdout")?;
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + LSP_IO_TIMEOUT;

    lsp_rust_analyzer_handshake(
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
    )?;

    let req_id = 2u64;
    let (method, params) =
        ra_lsp_request_method_params(op, &file_uri, inputs.line, inputs.character, &v);
    let req = json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params
    });
    write_lsp(&mut stdin, &req.to_string()).map_err(|e| e.to_string())?;

    let resp = read_response_until_id(&mut reader, req_id, deadline)?;
    drop(stdin);
    lsp_rust_analyzer_run(op, &resp, inputs.max_symbols)
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

fn parse_ra_lsp_json_inputs(v: &Value, op: RaLspOp) -> Result<ParsedRaLspInputs, String> {
    let path_rel = v
        .get("path")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 path".to_string())?;
    let (line, character) = if matches!(op, RaLspOp::DocumentSymbol | RaLspOp::WorkspaceSymbol) {
        (0u32, 0u32)
    } else {
        let line = v
            .get("line")
            .and_then(|x| x.as_u64())
            .ok_or("缺少 line（0-based）")? as u32;
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
                return Err("缺少 query（workspace/symbol 搜索串）".to_string());
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

fn load_ra_workspace_sources(
    workspace_root: &Path,
    path_rel: &str,
) -> Result<RaWorkspaceSources, String> {
    let root = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区路径无法解析: {}", e))?;
    let file_path = resolve_rel_file(&root, path_rel)?;
    let uri = path_to_file_uri(&file_path)?;
    let text = fs::read_to_string(&file_path).map_err(|e| format!("读取源文件失败: {}", e))?;
    if text.len() > MAX_DID_OPEN_BYTES {
        return Err(format!(
            "文件过大 (>{})，请用 read_file 分段或缩小文件",
            MAX_DID_OPEN_BYTES
        ));
    }

    let root_uri = path_to_file_uri(&root)?;
    Ok(RaWorkspaceSources {
        root,
        file_uri: uri,
        root_uri,
        text,
    })
}

fn spawn_rust_analyzer_server(server_path: &str) -> Result<KillRaChild, String> {
    let child = Command::new(server_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "无法启动 rust-analyzer（{}）：{}。请安装 rust-analyzer 或设置 server_path。",
                server_path, e
            )
        })?;
    Ok(KillRaChild(Some(child)))
}
