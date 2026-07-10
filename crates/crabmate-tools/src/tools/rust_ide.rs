//! Rust 编译器 JSON 诊断（`cargo --message-format=json`）与可选的 **rust-analyzer** LSP（stdio）。
//!
//! rust-analyzer 需已安装并在 PATH 中（或由 `server_path` 指定）。LSP 为最小实现：initialize → didOpen → 单次请求（definition / references / hover / documentSymbol / implementation / typeDefinition / documentHighlight / workspace/symbol）。

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
const DEFAULT_MAX_DIAGNOSTICS: usize = 120;
const DEFAULT_RA_WAIT_MS: u64 = 500;
const DEFAULT_MAX_DOCUMENT_SYMBOLS: u64 = 500;
const LSP_IO_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Copy)]
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

// ---------- cargo / rustc JSON（compiler-message）----------

/// 运行 `cargo check --message-format=json`，解析 `compiler-message` 行并汇总为可读文本（不整段原始 JSON 灌给模型）。
pub fn rust_compiler_json(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：工作区根目录未找到 Cargo.toml".to_string();
    }

    let all_targets = v
        .get("all_targets")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let features = v
        .get("features")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let all_features = v
        .get("all_features")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let max_diag = v
        .get("max_diagnostics")
        .and_then(|x| x.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_DIAGNOSTICS)
        .min(500);
    let format_kind = v
        .get("message_format")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("json");

    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--message-format")
        .arg(format_kind)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if all_targets {
        cmd.arg("--all-targets");
    }
    if let Some(p) = package {
        cmd.arg("--package").arg(p);
    }
    if let Some(f) = features {
        cmd.arg("--features").arg(f);
    }
    if all_features {
        cmd.arg("--all-features");
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("无法执行 cargo check: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit = output.status.code().unwrap_or(-1);

    let mut diags: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(inner) = msg.get("message") else {
            continue;
        };
        if diags.len() >= max_diag {
            break;
        }
        diags.push(format_compiler_message(inner));
    }

    let mut out = String::new();
    out.push_str(&format!(
        "rust_compiler_json: cargo check --message-format={} (exit={})\n",
        format_kind, exit
    ));
    if !stderr.trim().is_empty() {
        out.push_str("--- cargo/stderr ---\n");
        out.push_str(&lsp_response_format::truncate_str(
            stderr.trim(),
            max_output_len.min(8192),
        ));
        out.push_str("\n---\n");
    }
    if diags.is_empty() {
        out.push_str("（未解析到 compiler-message 行；若仅有 stderr 请见上文。可能编译通过或无 JSON 行。）\n");
    } else {
        out.push_str(&format!(
            "共 {} 条诊断（最多展示 {} 条）：\n\n",
            diags.len(),
            max_diag
        ));
        for (i, d) in diags.iter().enumerate() {
            out.push_str(&format!("--- [{}] ---\n{}\n", i + 1, d));
        }
    }

    lsp_response_format::truncate_str(&out, max_output_len).to_string()
}

fn format_compiler_message(m: &Value) -> String {
    let level = m.get("level").and_then(|x| x.as_str()).unwrap_or("?");
    let code = m
        .get("code")
        .and_then(|c| c.get("code").and_then(|x| x.as_str()))
        .unwrap_or("");
    let text = m.get("message").and_then(|x| x.as_str()).unwrap_or("");
    let mut s = format!("[{}]", level);
    if !code.is_empty() {
        s.push_str(&format!(" {}", code));
    }
    s.push_str(&format!(" {}\n", text));

    if let Some(rendered) = m.get("rendered").and_then(|x| x.as_str())
        && !rendered.trim().is_empty()
    {
        s.push_str(rendered.trim_end());
        s.push('\n');
    }

    if let Some(spans) = m.get("spans").and_then(|x| x.as_array()) {
        for sp in spans.iter().take(5) {
            let file = sp.get("file_name").and_then(|x| x.as_str()).unwrap_or("?");
            let line = sp.get("line_start").and_then(|x| x.as_u64()).unwrap_or(0);
            let col = sp.get("column_start").and_then(|x| x.as_u64()).unwrap_or(0);
            let is_primary = sp
                .get("is_primary")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let label = sp.get("label").and_then(|x| x.as_str()).unwrap_or("");
            let p = if is_primary { " (primary)" } else { "" };
            s.push_str(&format!(
                "  {}{}:{}:{}{} {}\n",
                file,
                p,
                line,
                col,
                if label.is_empty() { "" } else { " — " },
                label
            ));
        }
        if spans.len() > 5 {
            s.push_str(&format!("  … 另有 {} 个 span\n", spans.len() - 5));
        }
    }

    if let Some(children) = m.get("children").and_then(|x| x.as_array()) {
        for ch in children.iter().take(8) {
            s.push_str(&format_compiler_message(ch));
        }
        if children.len() > 8 {
            s.push_str(&format!("… 另有 {} 条子诊断\n", children.len() - 8));
        }
    }
    s.trim_end().to_string()
}

#[path = "rust_ide_lsp_response_format.rs"]
mod lsp_response_format;

#[path = "rust_ide_lsp_request.rs"]
mod lsp_request;

// ---------- rust-analyzer LSP（stdio）----------

/// `textDocument/definition`：path 相对工作区，**line / character 为 0-based**（与 LSP 一致）。
pub fn rust_analyzer_goto_definition(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::Definition) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/references`（`include_declaration` 默认 true）。
pub fn rust_analyzer_find_references(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::References) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/hover`：path + 0-based line/character。
pub fn rust_analyzer_hover(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::Hover) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/documentSymbol`：整文件符号树（或 `SymbolInformation` 列表），条数由 `max_symbols` 限制。
pub fn rust_analyzer_document_symbol(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::DocumentSymbol) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/implementation`：path + 0-based line/character（trait 实现等跳转）。
pub fn rust_analyzer_goto_implementation(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::Implementation) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/typeDefinition`：path + 0-based line/character。
pub fn rust_analyzer_goto_type_definition(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::TypeDefinition) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `textDocument/documentHighlight`：path + 0-based line/character（同一符号在文件内的读/写高亮）。
pub fn rust_analyzer_document_highlight(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::DocumentHighlight) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// `workspace/symbol`：按名称模糊查工作区符号。`path` 须为工作区内一 `.rs` 源（用于 `didOpen` 与现有会话一致），`query` 为搜索串。
pub fn rust_analyzer_workspace_symbol(args_json: &str, workspace_root: &Path) -> String {
    match lsp_rust_analyzer_request(args_json, workspace_root, RaLspOp::WorkspaceSymbol) {
        Ok(s) => s,
        Err(e) => e,
    }
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

fn lsp_rust_analyzer_handshake(
    stdin: &mut impl Write,
    reader: &mut BufReader<impl std::io::Read>,
    h: &LspRustAnalyzerHandshake<'_>,
) -> Result<(), String> {
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
    write_lsp(stdin, &init.to_string()).map_err(|e| e.to_string())?;
    let _ = read_response_until_id(reader, 1, h.deadline)?;

    let notif_init = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    write_lsp(stdin, &notif_init.to_string()).map_err(|e| e.to_string())?;

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
    write_lsp(stdin, &did_open.to_string()).map_err(|e| e.to_string())?;

    std::thread::sleep(Duration::from_millis(h.wait_ms));
    Ok(())
}

fn lsp_rust_analyzer_run(op: RaLspOp, resp: &Value, max_symbols: usize) -> Result<String, String> {
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

fn lsp_rust_analyzer_request(
    args_json: &str,
    workspace_root: &Path,
    op: RaLspOp,
) -> Result<String, String> {
    lsp_request::lsp_rust_analyzer_request(args_json, workspace_root, op)
}

fn resolve_rel_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if Path::new(rel).is_absolute() || rel.contains("..") {
        return Err("path 必须为工作区内相对路径且不含 ..".to_string());
    }
    let joined = root.join(rel);
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    if !canon.starts_with(root) {
        return Err("路径超出工作区".to_string());
    }
    if !canon.is_file() {
        return Err("不是已存在的文件".to_string());
    }
    Ok(canon)
}

fn path_to_file_uri(p: &Path) -> Result<String, String> {
    let canon = p
        .canonicalize()
        .map_err(|e| format!("canonicalize: {}", e))?;
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

fn read_response_until_id<R: BufRead>(
    reader: &mut R,
    expected_id: u64,
    deadline: Instant,
) -> Result<Value, String> {
    loop {
        if Instant::now() > deadline {
            return Err("LSP 读取超时".to_string());
        }
        let buf = read_one_lsp_message(reader).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&buf).map_err(|e| format!("JSON: {}", e))?;
        if v.get("id").and_then(|x| x.as_u64()) == Some(expected_id) {
            return Ok(v);
        }
    }
}

fn read_one_lsp_message<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, String> {
    let mut content_len: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("读 LSP 头: {}", e))?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = Some(
                rest.trim()
                    .parse::<usize>()
                    .map_err(|_| "Content-Length 无效".to_string())?,
            );
        }
    }
    let len = content_len.ok_or_else(|| "缺少 Content-Length".to_string())?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("读 LSP 体: {}", e))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_compiler_message_smoke() {
        let m = json!({
            "level": "error",
            "message": "oops",
            "code": { "code": "E0000" },
            "spans": [{
                "file_name": "src/lib.rs",
                "line_start": 1,
                "column_start": 0,
                "is_primary": true,
                "label": "here"
            }]
        });
        let s = format_compiler_message(&m);
        assert!(s.contains("error"));
        assert!(s.contains("E0000"));
        assert!(s.contains("src/lib.rs"));
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
}
