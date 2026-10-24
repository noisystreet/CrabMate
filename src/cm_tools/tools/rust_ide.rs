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

// ---------- cargo / rustc JSON（compiler-message）----------

struct CargoCheckArgs<'a> {
    all_targets: bool,
    package: Option<&'a str>,
    features: Option<&'a str>,
    all_features: bool,
    max_diag: usize,
    format_kind: &'a str,
}

fn parse_cargo_check_args(v: &Value) -> CargoCheckArgs<'_> {
    CargoCheckArgs {
        all_targets: v
            .get("all_targets")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        package: v
            .get("package")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        features: v
            .get("features")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        all_features: v
            .get("all_features")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        max_diag: v
            .get("max_diagnostics")
            .and_then(|x| x.as_u64())
            .map(|n| n.max(1) as usize)
            .unwrap_or(DEFAULT_MAX_DIAGNOSTICS)
            .min(500),
        format_kind: v
            .get("message_format")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("json"),
    }
}

fn build_cargo_check_command(args: &CargoCheckArgs<'_>, workspace_root: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--message-format")
        .arg(args.format_kind)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if args.all_targets {
        cmd.arg("--all-targets");
    }
    if let Some(p) = args.package {
        cmd.arg("--package").arg(p);
    }
    if let Some(f) = args.features {
        cmd.arg("--features").arg(f);
    }
    if args.all_features {
        cmd.arg("--all-features");
    }
    cmd
}

fn try_compiler_message_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let msg = serde_json::from_str::<Value>(line).ok()?;
    if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
        return None;
    }
    let inner = msg.get("message")?;
    Some(format_compiler_message(inner))
}

fn collect_compiler_messages(stdout: &str, max_diag: usize) -> Vec<String> {
    let mut diags: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if diags.len() >= max_diag {
            break;
        }
        if let Some(d) = try_compiler_message_line(line) {
            diags.push(d);
        }
    }
    diags
}

fn format_rust_compiler_json_report(
    format_kind: &str,
    exit: i32,
    stderr: &str,
    diags: &[String],
    max_diag: usize,
    max_output_len: usize,
) -> String {
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

/// 运行 `cargo check --message-format=json`，解析 `compiler-message` 行并汇总为可读文本（不整段原始 JSON 灌给模型）。
pub fn rust_compiler_json(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：工作区根目录未找到 Cargo.toml".to_string();
    }

    let args = parse_cargo_check_args(&v);
    let output = match build_cargo_check_command(&args, workspace_root).output() {
        Ok(o) => o,
        Err(e) => return format!("无法执行 cargo check: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit = output.status.code().unwrap_or(-1);
    let diags = collect_compiler_messages(&stdout, args.max_diag);
    format_rust_compiler_json_report(
        args.format_kind,
        exit,
        &stderr,
        &diags,
        args.max_diag,
        max_output_len,
    )
}

fn format_compiler_span_line(sp: &Value) -> String {
    let file = sp.get("file_name").and_then(|x| x.as_str()).unwrap_or("?");
    let line = sp.get("line_start").and_then(|x| x.as_u64()).unwrap_or(0);
    let col = sp.get("column_start").and_then(|x| x.as_u64()).unwrap_or(0);
    let is_primary = sp
        .get("is_primary")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let label = sp.get("label").and_then(|x| x.as_str()).unwrap_or("");
    let p = if is_primary { " (primary)" } else { "" };
    format!(
        "  {}{}:{}:{}{} {}\n",
        file,
        p,
        line,
        col,
        if label.is_empty() { "" } else { " — " },
        label
    )
}

fn append_compiler_spans(s: &mut String, spans: &[Value]) {
    for sp in spans.iter().take(5) {
        s.push_str(&format_compiler_span_line(sp));
    }
    if spans.len() > 5 {
        s.push_str(&format!("  … 另有 {} 个 span\n", spans.len() - 5));
    }
}

fn append_compiler_children(s: &mut String, children: &[Value]) {
    for ch in children.iter().take(8) {
        s.push_str(&format_compiler_message(ch));
    }
    if children.len() > 8 {
        s.push_str(&format!("… 另有 {} 条子诊断\n", children.len() - 8));
    }
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
        append_compiler_spans(&mut s, spans);
    }

    if let Some(children) = m.get("children").and_then(|x| x.as_array()) {
        append_compiler_children(&mut s, children);
    }
    s.trim_end().to_string()
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
/// 因此错误文案与逐工具时代保持一致。
pub fn rust_analyzer(args_json: &str, workspace_root: &Path) -> String {
    let op = match ra_lsp_op_from_json(args_json) {
        Ok(op) => op,
        Err(msg) => return msg,
    };
    match lsp_rust_analyzer_request(args_json, workspace_root, op) {
        Ok(s) => s,
        Err(e) => e,
    }
}

/// 从入参 JSON 的 `action` 解析出 [`RaLspOp`]；缺失与未知都给出可选值清单。
fn ra_lsp_op_from_json(args_json: &str) -> Result<RaLspOp, String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    let action = match v.get("action").and_then(|x| x.as_str()).map(str::trim) {
        Some(a) if !a.is_empty() => a,
        _ => return Err(format!("错误：缺少 action（可选值：{RA_ACTIONS}）")),
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
        other => Err(format!(
            "错误：未知 action `{other}`（可选值：{RA_ACTIONS}）"
        )),
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
            assert_eq!(
                ra_lsp_op_from_json(&args),
                Ok(want),
                "action={action} 应映射到 {want:?}"
            );
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
            assert!(err.contains("缺少 action"), "{err}");
            assert!(err.contains("workspace_symbol"), "应列出可选值: {err}");
        }

        let err = ra_lsp_op_from_json(r#"{"action":"nope","path":"src/lib.rs"}"#)
            .expect_err("未知 action 须失败");
        assert!(err.contains("未知 action `nope`"), "{err}");
        assert!(err.contains("goto_definition"), "应列出可选值: {err}");
    }

    #[test]
    fn ra_invalid_json_is_reported() {
        let err = ra_lsp_op_from_json("{").expect_err("非法 JSON 须失败");
        assert!(err.starts_with("参数 JSON 无效"), "{err}");
    }

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
